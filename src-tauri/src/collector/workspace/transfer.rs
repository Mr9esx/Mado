use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReview {
    draft: Draft,
    conflict: bool,
    identical: bool,
}

fn read_file(path: &Path, limit: usize) -> Result<String, String> {
    let mut text = String::new();
    open_json_file(path)
        .map_err(|error| error.to_string())?
        .take(limit as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() > limit {
        return Err("模块文件超过大小限制。".into());
    }
    Ok(text)
}

fn load_project(root: &Path) -> Result<Project, String> {
    read_project(root, false)
}

pub(super) fn read_project(root: &Path, standalone: bool) -> Result<Project, String> {
    let manifest: Manifest =
        serde_json::from_str(&read_file(&root.join("manifest.json"), 128 * 1024)?)
            .map_err(|error| format!("模块清单：{error}"))?;
    let mut files = BTreeMap::new();
    let mut remaining = MAX_PROJECT;
    for item in walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(10)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !entry.file_name().to_string_lossy().starts_with('.')
        })
    {
        let item = item.map_err(|error| error.to_string())?;
        if item.file_type().is_symlink() {
            return Err("模块目录不能包含符号链接。".into());
        }
        if !item.file_type().is_file() {
            continue;
        }
        let path = item
            .path()
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .components()
            .map(|part| part.as_os_str().to_str().ok_or("文件路径无法读取。"))
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        if !matches!(
            path.rsplit_once('.').map(|(_, extension)| extension),
            Some("js" | "mjs" | "json")
        ) || path == "manifest.json"
            || (!manifest.definition_in_script && path == "fixtures/sample.json")
        {
            continue;
        }
        if files.len() >= MAX_FILES || !path_valid(&path) {
            return Err("模块文件数量过多或路径无效。".into());
        }
        let text = read_file(item.path(), remaining)?;
        remaining -= text.len();
        files.insert(path, text);
    }
    let fixture_file = root.join("fixtures/sample.json");
    let fixture = if !manifest.definition_in_script && fixture_file.is_file() {
        serde_json::from_str(&read_file(&fixture_file, INPUT_LIMIT)?)
            .map_err(|error| format!("样例输入：{error}"))?
    } else {
        json!({})
    };
    let project = Project {
        manifest,
        files,
        fixture,
    };
    project.validate(standalone)?;
    Ok(project)
}

#[tauri::command]
pub async fn inspect_collector_import(
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<Option<ImportReview>, String> {
    command_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择采集模块目录")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|error| error.to_string())?;
        let project = load_project(&path)?;
        let draft = Draft::from_project(&project);
        let state = app.state::<Mutex<Workspace>>();
        let workspace = state.lock().map_err(|error| error.to_string())?;
        let existing = workspace.catalog.modules.get(&draft.id);
        let identical = existing.is_some_and(|module| {
            module.draft.project(false).as_ref() == Ok(&project)
                || module
                    .released
                    .as_ref()
                    .is_some_and(|release| release.project == project)
        });
        Ok(Some(ImportReview {
            draft,
            conflict: existing.is_some(),
            identical,
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn import_collector(
    window: tauri::WebviewWindow,
    state: State<'_, Mutex<Workspace>>,
    mut draft: Draft,
    mode: String,
) -> Result<Draft, String> {
    command_window(&window)?;
    draft.project(false)?;
    draft = draft.migrate()?;
    let mut workspace = state.lock().map_err(|error| error.to_string())?;
    let mut next = workspace.catalog.clone();
    if mode == "copy" {
        loop {
            draft.id = uuid::Uuid::new_v4().to_string();
            if !next.modules.contains_key(&draft.id) {
                break;
            }
        }
    }
    if let Some(module) = next.modules.get_mut(&draft.id) {
        if mode != "update" {
            return Err("模块 ID 已存在，请选择更新或导入副本。".into());
        }
        let version = module
            .released
            .as_ref()
            .map_or(module.draft.sdk_api_version, |release| {
                release.project.manifest.sdk_api_version
            });
        ensure_sdk_version(version, draft.sdk_api_version)?;
        module.draft = draft.clone();
    } else {
        next.modules.insert(
            draft.id.clone(),
            Module {
                draft: draft.clone(),
                released: None,
                previous: None,
            },
        );
    }
    workspace.save(next)?;
    workspace.tested.remove(&draft.id);
    Ok(draft)
}

fn write_project(project: &Project, root: &Path) -> Result<(), String> {
    fs::create_dir(root).map_err(|error| format!("无法建立导出目录：{error}"))?;
    let result = (|| {
        let manifest =
            serde_json::to_vec_pretty(&project.manifest).map_err(|error| error.to_string())?;
        fs::write(root.join("manifest.json"), manifest).map_err(|error| error.to_string())?;
        for (path, text) in &project.files {
            if !path_valid(path) || path == "manifest.json" {
                return Err("项目包含保留或无效路径。".into());
            }
            let target = root.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(target, text).map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(root);
    }
    result
}

#[tauri::command]
pub async fn export_collector(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
) -> Result<Option<String>, String> {
    command_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let project = {
            let state = app.state::<Mutex<Workspace>>();
            let workspace = state.lock().map_err(|error| error.to_string())?;
            workspace
                .catalog
                .modules
                .get(&id)
                .and_then(|module| module.released.as_ref())
                .map(|release| release.project.clone())
                .ok_or("请先发布要导出的模块。")?
        };
        let Some(folder) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("选择模块导出位置")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let folder = folder.into_path().map_err(|error| error.to_string())?;
        let target = folder.join(&project.manifest.module_id);
        write_project(&project, &target)?;
        Ok(Some(target.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_projects_keep_the_manifest_and_exclude_test_data() {
        let project = create_template("测试".into(), "http")
            .unwrap()
            .project(false)
            .unwrap();
        let root = std::env::temp_dir().join(format!("tdisplay-test-{}", uuid::Uuid::new_v4()));
        write_project(&project, &root).unwrap();
        let exported = load_project(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(exported.manifest, project.manifest);
        assert_eq!(exported.files, project.files);
        assert_eq!(exported.fixture, json!({}));
    }
    #[test]
    fn an_authored_fixture_file_round_trips_as_a_project_file() {
        let mut project = create_template("测试".into(), "http")
            .unwrap()
            .project(false)
            .unwrap();
        let sample = "fixtures/sample.json";
        project.files.insert(sample.into(), "{\"calls\":[]}".into());
        let root = std::env::temp_dir().join(format!("tdisplay-sample-{}", uuid::Uuid::new_v4()));
        write_project(&project, &root).unwrap();
        let exported = load_project(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(exported.files[sample], project.files[sample]);
        assert_eq!(exported.fixture, json!({}));
    }
}
