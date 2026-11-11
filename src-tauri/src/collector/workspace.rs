use super::{
    builtin, io,
    model::{
        path_valid, BindingHint, History, Manifest, Permission, Project, Setting, MAX_FILES,
        MAX_PROJECT,
    },
    now, runner, vault::KeyStore,
};
use crate::{device::Device, SNAPSHOT_EVENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

pub mod transfer;

const INPUT_LIMIT: usize = 256 * 1024;
const MAX_INSTANCES: usize = 16;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    id: String,
    pub definition: String,
    #[serde(default)]
    definition_in_script: bool,
    pub entry: String,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub directories: Vec<String>,
    pub fixture: String,
    sdk_api_version: u32,
}

impl Draft {
    fn from_project(project: &Project) -> Self {
        Self {
            id: project.manifest.module_id.clone(),
            definition: serde_json::to_string_pretty(&project.manifest.definition)
                .expect("valid module definition"),
            definition_in_script: project.manifest.definition_in_script,
            entry: project.manifest.entry.clone(),
            files: project.files.clone(),
            directories: Vec::new(),
            fixture: serde_json::to_string_pretty(&project.fixture).expect("valid fixture"),
            sdk_api_version: project.manifest.sdk_api_version,
        }
    }

    fn migrate(mut self) -> Result<Self, String> {
        if self.definition_in_script {
            return Ok(self);
        }
        let (parent, name) = self
            .entry
            .rsplit_once('/')
            .map_or(("", self.entry.as_str()), |(parent, name)| (parent, name));
        let candidates = |directory: &str| {
            let directory = directory.to_owned();
            ["collector.mjs".to_string()]
                .into_iter()
                .chain((0..MAX_FILES + 1).map(|index| format!("{index}.mjs")))
                .map(move |name| {
                    if directory.is_empty() {
                        name
                    } else {
                        format!("{directory}/{name}")
                    }
                })
        };
        let entry = candidates(parent)
            .chain(candidates(""))
            .find(|path| {
                let lower = path.to_ascii_lowercase();
                let prefix = format!("{lower}/");
                path_valid(path)
                    && self.files.keys().chain(&self.directories).all(|existing| {
                        let existing = existing.to_ascii_lowercase();
                        existing != lower && !existing.starts_with(&prefix)
                    })
            })
            .ok_or("无法为旧采集器创建新的入口脚本。")?;
        let original_path = if entry.contains('/') {
            name
        } else {
            &self.entry
        };
        let original = serde_json::to_string(&format!("./{original_path}"))
            .map_err(|error| error.to_string())?;
        let metadata = serde_json::from_str::<Value>(&self.definition)
            .ok()
            .and_then(|definition| serde_json::to_string_pretty(&definition).ok())
            .unwrap_or_else(|| self.definition.clone());
        let source = format!(
            "import {{ defineModule }} from '@mado/sdk'\nimport previous from {original}\n\nexport default defineModule({{{}\n  collect: previous.collect,\n  handlers: previous.handlers,\n}})\n",
            definition_members(&metadata)
        );
        self.files.insert(entry.clone(), source);
        self.entry = entry;
        self.definition_in_script = true;
        Ok(self)
    }

    fn same_source(&self, other: &Self) -> bool {
        self.id == other.id
            && self.entry == other.entry
            && self.files == other.files
            && self.fixture == other.fixture
            && self.sdk_api_version == other.sdk_api_version
    }

    fn inspected_project(&self, trusted: bool, offline: bool) -> Result<Project, String> {
        let value = runner::inspect(runner::Request {
            entry: self.entry.clone(),
            files: self.files.clone(),
            inputs: Value::Null,
            private_settings: Vec::new(),
            sampled_at: 0,
            action: None,
            state: Value::Null,
        })
        .map_err(|error| format!("入口脚本定义：{error}"))?;
        let mut inspected = self.clone();
        inspected.definition = value.to_string();
        if !offline {
            inspected.fixture = "{}".into();
        } else if let Some(file) = inspected.files.get("fixtures/sample.json") {
            inspected.fixture = file.clone();
        }
        inspected.project(trusted)
    }

    fn project(&self, trusted: bool) -> Result<Project, String> {
        let project = Project {
            manifest: Manifest {
                module_id: self.id.clone(),
                sdk_api_version: self.sdk_api_version,
                entry: self.entry.clone(),
                definition_in_script: self.definition_in_script,
                definition: serde_json::from_str(&self.definition)
                    .map_err(|error| format!("模块定义：{error}"))?,
            },
            files: self.files.clone(),
            fixture: serde_json::from_str(&self.fixture)
                .map_err(|error| format!("样例数据：{error}"))?,
        };
        project.validate(trusted)?;
        if !project.fixture.is_object() {
            return Err("样例数据须是 JSON 对象。".into());
        }
        Ok(project)
    }
}

fn definition_members(definition: &str) -> String {
    let text = definition.trim();
    let Some(members) = text
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    else {
        return format!("\n  ...({text}),");
    };
    if members.trim().is_empty() {
        return String::new();
    }
    let members = members
        .trim_end()
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("\"__proto__\":") {
                line.replacen("\"__proto__\":", "[\"__proto__\"]:", 1)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{members},")
}

#[derive(Clone, Deserialize, Serialize)]
struct Release {
    revision: u64,
    project: Project,
}

#[derive(Clone, Deserialize, Serialize)]
struct Module {
    draft: Draft,
    released: Option<Release>,
    previous: Option<Release>,
}

impl Module {
    fn has_variable_defaults(&self) -> bool {
        let release_has_defaults = self.released.iter().chain(&self.previous).any(|release| {
            release
                .project
                .manifest
                .definition
                .settings
                .values()
                .any(|setting| setting.is_variable() && !setting.default.is_empty())
        });
        if release_has_defaults {
            return true;
        }
        // Drafts can have valid JSON settings without a valid module definition.
        let Ok(definition) = serde_json::from_str::<Value>(&self.draft.definition) else {
            return false;
        };
        definition
            .get("settings")
            .and_then(Value::as_object)
            .is_some_and(|settings| {
                settings.values().any(|setting| {
                    ["description", "required", "secret"]
                        .iter()
                        .any(|key| setting.get(*key).is_some_and(|value| !value.is_null()))
                        && setting
                            .get("default")
                            .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
                })
            })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance<T = String> {
    id: String,
    module_id: String,
    name: String,
    settings: BTreeMap<String, T>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    approved_permissions: BTreeMap<String, Permission>,
    refresh_seconds: u64,
    enabled: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Catalog<T = String> {
    modules: BTreeMap<String, Module>,
    instances: BTreeMap<String, Instance<T>>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Stored<T> {
    Plain(T),
    Encrypted { ciphertext: String },
}

#[derive(Deserialize, Serialize)]
struct StoredCatalog {
    modules: BTreeMap<String, Stored<Module>>,
    instances: BTreeMap<String, Instance<Stored<String>>>,
}

impl<T> Catalog<T> {
    fn declared_settings(&self, module_id: &str) -> Option<&BTreeMap<String, Setting>> {
        // Bundled releases are deliberately excluded from the disk catalog.
        builtin::get(module_id)
            .or_else(|| {
                self.modules
                    .get(module_id)?
                    .released
                    .as_ref()
                    .map(|r| &r.project)
            })
            .map(|project| &project.manifest.definition.settings)
    }

    fn is_variable(&self, module_id: &str, key: &str) -> bool {
        self.declared_settings(module_id)
            .and_then(|settings| settings.get(key))
            .is_some_and(Setting::is_variable)
    }

    fn map_settings<U>(
        mut self,
        mut map: impl FnMut(bool, T) -> Result<Option<U>, String>,
    ) -> Result<Catalog<U>, String> {
        let mut instances = BTreeMap::new();
        for (id, instance) in std::mem::take(&mut self.instances) {
            let mut settings = BTreeMap::new();
            for (key, value) in instance.settings {
                if let Some(value) = map(self.is_variable(&instance.module_id, &key), value)? {
                    settings.insert(key, value);
                }
            }
            instances.insert(
                id,
                Instance {
                    id: instance.id,
                    module_id: instance.module_id,
                    name: instance.name,
                    settings,
                    approved_permissions: instance.approved_permissions,
                    refresh_seconds: instance.refresh_seconds,
                    enabled: instance.enabled,
                },
            );
        }
        Ok(Catalog {
            modules: self.modules,
            instances,
        })
    }
}

pub struct Workspace {
    path: PathBuf,
    catalog: Catalog,
    key: Option<KeyStore>,
    tested: BTreeMap<String, (Draft, Project)>,
    status: BTreeMap<String, Status>,
    interactions: BTreeMap<String, Interaction>,
    refresh: std::collections::BTreeSet<String>,
    sequence: u64,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    updated_at: Option<u64>,
    error: Option<String>,
    #[serde(skip)]
    data: Value,
}

#[derive(Clone)]
struct Interaction {
    name: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleSummary {
    id: String,
    name: String,
    built_in: bool,
    revision: Option<u64>,
    can_restore: bool,
    changed: bool,
    actions: Vec<String>,
    action_instance: Option<String>,
    active_action: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceView {
    modules: Vec<ModuleSummary>,
    status: BTreeMap<String, Status>,
}

impl Workspace {
    fn load(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let mut key = None;
        let directory = path.parent().ok_or("找不到采集器配置目录。")?;
        let mut catalog = match fs::read(&path) {
            Ok(bytes) => {
                let stored: StoredCatalog =
                    serde_json::from_slice(&bytes).map_err(|_| "采集器配置文件格式无效。")?;
                let mut open = |ciphertext: String| -> Result<String, String> {
                    if key.is_none() {
                        key = Some(KeyStore::load(directory)?);
                    }
                    key.as_ref().unwrap().decrypt(&ciphertext)
                };
                let modules = stored
                    .modules
                    .into_iter()
                    .map(|(id, module)| {
                        let module = match module {
                            Stored::Plain(module) => module,
                            Stored::Encrypted { ciphertext } => {
                                serde_json::from_str(&open(ciphertext)?)
                                    .map_err(|_| "加密的采集器模块格式无效。")?
                            }
                        };
                        Ok::<_, String>((id, module))
                    })
                    .collect::<Result<_, _>>()?;
                let catalog = Catalog {
                    modules,
                    instances: stored.instances,
                };
                catalog.map_settings(|variable, value| match value {
                    Stored::Plain(value) => Ok(Some(value)),
                    Stored::Encrypted { ciphertext } => {
                        let value = open(ciphertext)?;
                        // An upgrade can remove a bundled variable. Never
                        // turn its old ciphertext into an ordinary setting.
                        Ok(variable.then_some(value))
                    }
                })?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Catalog::default(),
            Err(error) => return Err(error.into()),
        };
        for project in builtin::projects() {
            let id = &project.manifest.module_id;
            let definition = &project.manifest.definition;
            let schedule = definition
                .schedule
                .as_ref()
                .expect("bundled collector schedule");
            catalog.modules.insert(
                id.clone(),
                Module {
                    draft: Draft::from_project(project),
                    released: Some(Release {
                        revision: 1,
                        project: project.clone(),
                    }),
                    previous: None,
                },
            );
            let instance = catalog
                .instances
                .entry(id.clone())
                .or_insert_with(|| Instance {
                    id: id.clone(),
                    module_id: id.clone(),
                    name: definition.name.clone(),
                    settings: BTreeMap::new(),
                    approved_permissions: definition.permissions.clone(),
                    refresh_seconds: schedule.refresh_seconds,
                    enabled: schedule.enabled,
                });
            instance.refresh_seconds = schedule.refresh_seconds;
            // Bundled paths stay as previously authorized. Updated grants require explicit reconfiguration.
            if instance.approved_permissions != definition.permissions {
                instance.enabled = false;
            }
        }
        Ok(Self {
            path,
            catalog,
            key,
            tested: BTreeMap::new(),
            status: BTreeMap::new(),
            interactions: BTreeMap::new(),
            refresh: Default::default(),
            sequence: 0,
        })
    }

    fn instance_status(&self, id: &str) -> Result<Value, String> {
        let instance = self.catalog.instances.get(id).ok_or("找不到采集实例。")?;
        let status = self.status.get(id);
        let settings: BTreeMap<_, _> = instance
            .settings
            .iter()
            .filter(|(key, _)| !self.catalog.is_variable(&instance.module_id, key))
            .collect();
        Ok(json!({
            "sequence": self.sequence,
            "enabled": instance.enabled,
            "settings": settings,
            "data": status.map(|status| &status.data),
            "error": status.and_then(|status| status.error.as_deref()),
            "action": self.interactions.get(id).map(|action| &action.name),
        }))
    }

    fn save(&mut self, next: Catalog) -> Result<(), String> {
        let directory = self.path.parent().ok_or("找不到采集器配置目录。")?;
        if let Some(key) = &self.key {
            key.verify_file(directory)?;
        }
        let mut stored = next.clone();
        stored.modules.retain(|id, _| builtin::get(id).is_none());
        let mut has_encrypted_values = false;
        let mut seal = |value: &str| -> Result<String, String> {
            if self.key.is_none() {
                self.key = Some(KeyStore::load_or_create(directory)?);
            }
            has_encrypted_values = true;
            self.key.as_ref().unwrap().encrypt(value)
        };
        let stored = stored.map_settings(|variable, value| {
            Ok(Some(if variable {
                Stored::Encrypted {
                    ciphertext: seal(&value)?,
                }
            } else {
                Stored::Plain(value)
            }))
        })?;
        let modules = stored
            .modules
            .into_iter()
            .map(|(id, module)| {
                let module = if module.has_variable_defaults() {
                    // The draft and source files can repeat the manifest value.
                    let serialized =
                        serde_json::to_string(&module).map_err(|_| "无法序列化采集器模块。")?;
                    Stored::Encrypted {
                        ciphertext: seal(&serialized)?,
                    }
                } else {
                    Stored::Plain(module)
                };
                Ok::<_, String>((id, module))
            })
            .collect::<Result<_, _>>()?;
        let stored = StoredCatalog {
            modules,
            instances: stored.instances,
        };
        let mut temp =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut temp, &stored).map_err(|error| error.to_string())?;
        temp.as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        temp.persist(&self.path)
            .map_err(|error| error.to_string())?;
        if !has_encrypted_values {
            self.key = None;
        }
        self.catalog = next;
        Ok(())
    }

    fn view(&self) -> WorkspaceView {
        let modules = self
            .catalog
            .modules
            .values()
            .map(|module| {
                let built_in = builtin::get(&module.draft.id).is_some();
                let action_instance = self
                    .catalog
                    .instances
                    .values()
                    .find(|instance| instance.module_id == module.draft.id && instance.enabled)
                    .map(|instance| instance.id.clone());
                ModuleSummary {
                    id: module.draft.id.clone(),
                    name: module
                        .released
                        .as_ref()
                        .map(|release| release.project.manifest.definition.name.clone())
                        .or_else(|| {
                            serde_json::from_str::<Value>(&module.draft.definition)
                                .ok()
                                .and_then(|definition| {
                                    definition["name"].as_str().map(str::to_owned)
                                })
                        })
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| "未命名采集器".into()),
                    built_in,
                    revision: module.released.as_ref().map(|release| release.revision),
                    can_restore: module.previous.is_some(),
                    changed: module.released.as_ref().is_none_or(|release| {
                        module.draft.entry != release.project.manifest.entry
                            || module.draft.files != release.project.files
                    }),
                    actions: module
                        .released
                        .as_ref()
                        .map(|release| release.project.manifest.definition.actions.clone())
                        .unwrap_or_default(),
                    active_action: action_instance
                        .as_ref()
                        .and_then(|id| self.interactions.get(id).map(|action| action.name.clone())),
                    action_instance,
                }
            })
            .collect();
        WorkspaceView {
            modules,
            status: self
                .catalog
                .instances
                .values()
                .filter_map(|instance| {
                    let status = self.status.get(&instance.id)?;
                    Some((instance.module_id.clone(), status.clone()))
                })
                .collect(),
        }
    }

    fn draft(&self, id: &str) -> Result<Draft, String> {
        self.catalog
            .modules
            .get(id)
            .map(|module| module.draft.clone())
            .ok_or("找不到采集模块。".into())
    }
}

#[tauri::command]
pub fn run_collector_action(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    action: String,
) -> Result<(), String> {
    command_window(&window)?;
    run_action(&app, &id, &action)
}

#[tauri::command]
pub fn cancel_collector_action(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
) -> Result<(), String> {
    command_window(&window)?;
    cancel_action(&app, &id)
}

pub(crate) const STATUS_EVENT: &str = "collector-status";

pub(crate) fn instance_status(app: &AppHandle, id: &str) -> Result<Value, String> {
    let state = app.state::<Mutex<Workspace>>();
    let workspace = state.lock().map_err(|error| error.to_string())?;
    workspace.instance_status(id)
}

fn suggest_binding(home: &Path, hint: &BindingHint) -> Option<String> {
    match hint {
        BindingHint::Executable { name } => {
            let name = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.clone()
            };
            let path = std::env::var_os("PATH").unwrap_or_default();
            std::env::split_paths(&path)
                .filter(|path| path.is_absolute())
                .chain([
                    home.join(".local/bin"),
                    PathBuf::from("/opt/homebrew/bin"),
                    PathBuf::from("/usr/local/bin"),
                ])
                .filter_map(|path| path.join(&name).into_os_string().into_string().ok())
                .find_map(|path| io::normalize_target("stdio", "程序", &path).ok())
        }
        BindingHint::UserDirectory { path, environment } => {
            let directory = std::env::var_os(environment)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(path));
            io::normalize_target("directory", "目录", directory.to_str()?).ok()
        }
    }
}

pub(crate) fn configure_bundled(
    app: &AppHandle,
    id: &str,
    enabled: bool,
    overrides: Option<BTreeMap<String, String>>,
) -> Result<(), String> {
    let project = builtin::get(id).ok_or("找不到随包采集器。")?;
    let definition = &project.manifest.definition;
    let bindings = if let Some(overrides) = overrides {
        let home = app.path().home_dir().map_err(|error| error.to_string())?;
        let mut bindings = BTreeMap::new();
        for (alias, hint) in &definition.suggested_bindings {
            if let Some(path) = overrides.get(alias).filter(|path| !path.trim().is_empty()) {
                let permission = &definition.permissions[alias];
                let scope = json!(permission);
                let kind = scope["kind"].as_str().unwrap_or_default();
                bindings.insert(format!("explicit.{alias}"), path.clone());
                if let Ok(path) = io::normalize_target(kind, permission.label(), path) {
                    bindings.insert(alias.clone(), path);
                }
            } else if let Some(path) = suggest_binding(&home, hint) {
                bindings.insert(alias.clone(), path);
            }
        }
        Some(bindings)
    } else {
        None
    };
    let state = app.state::<Mutex<Workspace>>();
    let mut workspace = state.lock().map_err(|error| error.to_string())?;
    let mut catalog = workspace.catalog.clone();
    let instance = catalog.instances.get_mut(id).ok_or("找不到采集实例。")?;
    instance.enabled = enabled;
    if let Some(bindings) = bindings {
        instance.settings.retain(|alias, _| {
            definition.settings.get(alias).is_some_and(Setting::is_variable)
        });
        instance.settings.extend(bindings);
        instance.approved_permissions = definition.permissions.clone();
    }
    workspace.save(catalog)?;
    if let Some(interaction) = workspace.interactions.remove(id) {
        interaction.cancelled.store(true, Ordering::Relaxed);
    }
    workspace.status.remove(id);
    workspace.refresh.insert(id.to_owned());
    workspace.sequence += 1;
    drop(workspace);
    super::publish_readings(app, id, BTreeMap::new());
    let _ = app.emit(STATUS_EVENT, id);
    Ok(())
}

pub(crate) fn refresh_instance(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<Mutex<Workspace>>();
    let mut workspace = state.lock().map_err(|error| error.to_string())?;
    if workspace
        .catalog
        .instances
        .get(id)
        .is_some_and(|instance| instance.enabled)
    {
        workspace.refresh.insert(id.to_owned());
    }
    Ok(())
}

pub(crate) fn run_action(app: &AppHandle, id: &str, name: &str) -> Result<(), String> {
    let state = app.state::<Mutex<Workspace>>();
    let mut workspace = state.lock().map_err(|error| error.to_string())?;
    let instance = workspace
        .catalog
        .instances
        .get(id)
        .filter(|instance| instance.enabled)
        .ok_or("请先启用采集器。")?;
    let project = workspace
        .catalog
        .modules
        .get(&instance.module_id)
        .and_then(|module| module.released.as_ref())
        .map(|release| &release.project)
        .ok_or("找不到已发布的采集器。")?;
    if !project
        .manifest
        .definition
        .actions
        .iter()
        .any(|action| action == name)
    {
        return Err("采集器未声明该操作。".into());
    }
    if workspace.interactions.contains_key(id) {
        return Err("采集器操作正在进行。".into());
    }
    workspace.interactions.insert(
        id.to_owned(),
        Interaction {
            name: name.to_owned(),
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    );
    workspace.sequence += 1;
    drop(workspace);
    let _ = app.emit(STATUS_EVENT, id);
    Ok(())
}

pub(crate) fn cancel_action(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<Mutex<Workspace>>();
    let mut workspace = state.lock().map_err(|error| error.to_string())?;
    if let Some(interaction) = workspace.interactions.remove(id) {
        interaction.cancelled.store(true, Ordering::Relaxed);
        workspace.sequence += 1;
        let _ = app.emit(STATUS_EVENT, id);
    }
    Ok(())
}

fn create_template(name: String, template: &str) -> Result<Draft, String> {
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err("请填写 1–80 字的采集器名称。".into());
    }
    let (permissions, resources, fixture, body) = match template {
        "sample" => (json!({}), json!({}), json!({}), "return { outputs: { reading: { value: 42 } } }"),
        "http" => (
            json!({ "source": { "kind": "http", "label": "接口地址" } }),
            json!({ "source": "https://example.com/metrics" }),
            json!({ "calls": [{ "operation": "http", "alias": "source", "params": { "path": "" },
                "result": { "status": 200, "text": "{\"value\":42}" } }] }),
            "const { value } = await context.http.getJson('source')\n    const reading = Number.isFinite(value) ? { value } : null\n    return { outputs: { reading } }",
        ),
        "file" => (
            json!({ "source": { "kind": "file", "label": "数据文件" } }),
            json!({}),
            json!({ "calls": [{ "operation": "readText", "alias": "source", "params": { "path": "" },
                "result": "{\"value\":42}" }] }),
            "const text = await context.files.readText('source')\n    const { value } = JSON.parse(text)\n    const reading = Number.isFinite(value) ? { value } : null\n    return { outputs: { reading } }",
        ),
        _ => return Err("找不到此模板。".into()),
    };
    let entry = "src/index.mjs".to_owned();
    let mut definition = json!({
        "name": name.trim(), "schedule": { "refreshSeconds": 60, "enabled": true },
        "outputs": { "reading": { "valueType": "number" } },
        "widgets": { "reading": { "outputKey": "reading", "kind": "metric",
            "label": "读数", "supportedSpans": [{ "w": 1, "h": 1 }] } }
    });
    if permissions.as_object().is_some_and(|map| !map.is_empty()) {
        definition["permissions"] = permissions;
    }
    if resources.as_object().is_some_and(|map| !map.is_empty()) {
        definition["resources"] = resources;
    }
    let definition =
        serde_json::to_string_pretty(&definition).map_err(|error| error.to_string())?;
    let source = format!(
        "import {{ defineModule }} from '@mado/sdk'\n\nexport default defineModule({{{}\n\n  async collect({}) {{\n    {body}\n  }},\n}})\n",
        definition_members(&definition),
        if template == "sample" { "" } else { "settings, context" },
    );
    Ok(Draft {
        id: uuid::Uuid::new_v4().to_string(),
        definition,
        definition_in_script: true,
        entry: entry.clone(),
        directories: vec!["src".into()],
        files: BTreeMap::from([(entry, source)]),
        fixture: serde_json::to_string_pretty(&fixture).map_err(|error| error.to_string())?,
        sdk_api_version: 2,
    })
}

pub fn start(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let directory = app.path().app_config_dir()?;
    fs::create_dir_all(&directory)?;
    let path = directory.join("collectors.json");
    let workspace = Workspace::load(path)?;
    {
        let device = app.state::<Mutex<Device>>();
        let mut device = device.lock().map_err(|error| error.to_string())?;
        for instance in workspace.catalog.instances.values() {
            if let Some(release) = workspace
                .catalog
                .modules
                .get(&instance.module_id)
                .and_then(|module| module.released.as_ref())
            {
                release
                    .project
                    .validate(builtin::get(&instance.module_id).is_some())?;
                device.install_widgets(
                    &instance.id,
                    release.project.widgets(&instance.id, &instance.name),
                )?;
            }
        }
    }
    app.manage(Mutex::new(workspace));
    let app = app.clone();
    thread::spawn(move || poll_sources(app));
    Ok(())
}

fn command_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "collectors" {
        Ok(())
    } else {
        Err("请从采集器工作区执行此操作。".into())
    }
}

#[tauri::command]
pub async fn open_collectors(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("collectors") {
        return window.set_focus().map_err(|error| error.to_string());
    }
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .cloned()
        .ok_or("找不到 Mado 主窗口配置。")?;
    config.label = "collectors".into();
    config.title = "Mado Studio · 数据采集器".into();
    config.width = 1080.0;
    config.height = 720.0;
    config.min_width = Some(790.0);
    config.min_height = Some(570.0);
    config.resizable = true;
    config.maximizable = true;
    WebviewWindowBuilder::from_config(&app, &config)
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn focus_layout(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    command_window(&window)?;
    crate::desktop_widget::show_editor(app).await
}

#[tauri::command]
pub fn collector_catalog(
    window: tauri::WebviewWindow,
    state: State<'_, Mutex<Workspace>>,
) -> Result<WorkspaceView, String> {
    command_window(&window)?;
    Ok(state.lock().map_err(|error| error.to_string())?.view())
}

#[tauri::command]
pub fn collector_draft(
    window: tauri::WebviewWindow,
    state: State<'_, Mutex<Workspace>>,
    id: String,
) -> Result<Draft, String> {
    command_window(&window)?;
    let mut workspace = state.lock().map_err(|error| error.to_string())?;
    let draft = workspace.draft(&id)?;
    if draft.definition_in_script {
        return Ok(draft);
    }
    let draft = draft.migrate()?;
    if let Some(module) = workspace.catalog.modules.get(&id) {
        let mut catalog = workspace.catalog.clone();
        catalog.modules.insert(
            id,
            Module {
                draft: draft.clone(),
                ..module.clone()
            },
        );
        workspace.save(catalog)?;
    }
    Ok(draft)
}

#[tauri::command]
pub fn create_collector(
    window: tauri::WebviewWindow,
    state: State<'_, Mutex<Workspace>>,
    name: String,
    template: String,
) -> Result<Draft, String> {
    command_window(&window)?;
    let mut state = state.lock().map_err(|error| error.to_string())?;
    let mut catalog = state.catalog.clone();
    let mut draft = create_template(name, &template)?;
    while catalog.modules.contains_key(&draft.id) {
        draft.id = uuid::Uuid::new_v4().to_string();
    }
    catalog.modules.insert(
        draft.id.clone(),
        Module {
            draft: draft.clone(),
            released: None,
            previous: None,
        },
    );
    state.save(catalog)?;
    Ok(draft)
}

#[tauri::command]
pub fn save_collector(
    window: tauri::WebviewWindow,
    state: State<'_, Mutex<Workspace>>,
    mut draft: Draft,
) -> Result<(), String> {
    command_window(&window)?;
    if builtin::get(&draft.id).is_some() {
        return Err("随包采集器由应用更新管理。".into());
    }
    let mut state = state.lock().map_err(|error| error.to_string())?;
    let mut catalog = state.catalog.clone();
    let module = catalog
        .modules
        .get_mut(&draft.id)
        .ok_or("找不到可编辑的采集模块。")?;
    draft.sdk_api_version = module.draft.sdk_api_version;
    draft.definition_in_script = module.draft.definition_in_script;
    draft.definition = module.draft.definition.clone();
    if draft.files.len() > MAX_FILES
        || draft.directories.len() > 32
        || draft.directories.iter().any(|path| !path_valid(path))
        || draft.files.keys().any(|path| !path_valid(path))
        || draft.files.values().map(String::len).sum::<usize>() + draft.fixture.len() > MAX_PROJECT
    {
        return Err("项目文件路径无效或超过容量限制。".into());
    }
    if module.draft != draft {
        module.draft = draft;
        state.save(catalog)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRequest {
    key: String,
    label: String,
    scope: Value,
    target: Option<String>,
}

#[derive(Deserialize)]
pub struct Approval {
    requests: Vec<GrantRequest>,
    targets: BTreeMap<String, String>,
}

struct PlannedSources {
    sources: Vec<Instance>,
    requests: Vec<GrantRequest>,
}

fn plan_sources(
    project: &Project,
    current: Option<&Project>,
    catalog: &Catalog,
    live: bool,
) -> Result<PlannedSources, String> {
    let id = &project.manifest.module_id;
    let definition = project.manifest.definition.clone();
    let current = current.map(|project| project.manifest.definition.clone());
    let mut sources: Vec<_> = catalog
        .instances
        .values()
        .filter(|source| source.module_id == *id)
        .cloned()
        .collect();
    let existing = !sources.is_empty();
    if !existing {
        sources.push(Instance {
            id: format!("collector:{id}"),
            module_id: id.clone(),
            name: definition.name.clone(),
            settings: BTreeMap::new(),
            approved_permissions: BTreeMap::new(),
            refresh_seconds: 60,
            enabled: true,
        });
    }
    let requires_filesystem = definition
        .inputs
        .values()
        .any(|input| io::is_path_resource(&input.kind))
        || definition.permissions.values().any(|permission| {
            !matches!(
                permission,
                Permission::Http { .. } | Permission::System { .. } | Permission::Browser { .. }
            )
        });
    let plan = move || {
        let multiple = sources.len() > 1;
        let mut requests = Vec::new();
        for source in &mut sources {
            let previous_settings = std::mem::take(&mut source.settings);
            if let Some(schedule) = &definition.schedule {
                if !multiple {
                    source.name = definition.name.clone();
                }
                source.refresh_seconds = schedule.refresh_seconds;
                source.enabled = schedule.enabled;
            }
            if let Some(previous) = &current {
                source.settings.extend(
                    previous_settings
                        .iter()
                        .filter(|(alias, _)| {
                            previous.settings.get(*alias).zip(definition.settings.get(*alias))
                                .is_some_and(|(old, new)| {
                                    if old.is_variable() || new.is_variable() {
                                        old.is_variable() && new.is_variable()
                                    } else {
                                        definition.schedule.is_none()
                                    }
                                })
                        })
                        .map(|(alias, value)| (alias.clone(), value.clone())),
                );
            }

            let grants = definition
                .permissions
                .iter()
                .map(|(alias, permission)| {
                    let previous = source.approved_permissions.get(alias);
                    (
                        alias,
                        json!(permission),
                        previous == Some(permission),
                        previous.is_some_and(|old| {
                            std::mem::discriminant(old) == std::mem::discriminant(permission)
                        }),
                    )
                })
                .chain(definition.inputs.iter().map(|(alias, input)| {
                    let previous = current.as_ref().and_then(|old| old.inputs.get(alias));
                    (
                        alias,
                        json!(input),
                        previous == Some(input),
                        previous.is_some_and(|old| old.kind == input.kind),
                    )
                }))
                .collect::<Vec<_>>();
            for (alias, scope, approved, same_kind) in grants {
                let kind = scope["kind"].as_str().expect("resource kind");
                let resource = scope["label"].as_str().unwrap_or(alias);
                let declared = definition.resources.get(alias);
                let old_declared = current.as_ref().and_then(|old| old.resources.get(alias));
                let saved = previous_settings
                    .get(alias)
                    .filter(|value| !value.is_empty() && same_kind);
                let mut target = if declared == old_declared && saved.is_some() {
                    saved.cloned()
                } else {
                    declared.or(saved).cloned()
                };
                let mut changed = !existing
                    || !approved
                    || (!matches!(kind, "system" | "browser")
                        && (target.is_none() || target.as_ref() != saved));
                if io::is_path_resource(kind) && (live || source.enabled || changed) {
                    if let Some(path) = declared.or(target.as_ref()) {
                        target = match io::normalize_target(kind, resource, path) {
                            Ok(path)
                                if declared.is_some() || saved.is_none_or(|old| old == &path) =>
                            {
                                Some(path)
                            }
                            Ok(_) => None,
                            Err(error) if declared.is_some() => return Err(error),
                            Err(_) => None,
                        };
                    }
                    changed |= target.is_none() || target.as_ref() != saved;
                }
                if let Some(value) = &target {
                    source.settings.insert(alias.clone(), value.clone());
                }
                if changed {
                    requests.push(GrantRequest {
                        key: format!("{}:{alias}", source.id),
                        label: if multiple {
                            format!("{} · {resource}", source.name)
                        } else {
                            resource.into()
                        },
                        scope,
                        target,
                    });
                }
            }
        }
        Ok(PlannedSources { sources, requests })
    };
    if requires_filesystem {
        io::check_paths(plan)
    } else {
        plan()
    }
}

impl PlannedSources {
    fn authorize(
        mut self,
        project: &Project,
        approval: Option<Approval>,
    ) -> Result<Vec<Instance>, String> {
        if !self.requests.is_empty() {
            let approval = approval
                .filter(|approval| approval.requests == self.requests)
                .ok_or("请确认当前版本的资源授权。")?;
            let requires_filesystem = self.requests.iter().any(|request| {
                io::is_path_resource(request.scope["kind"].as_str().unwrap_or_default())
            });
            let normalize = move || {
                let mut targets = BTreeMap::new();
                for request in self.requests {
                    let kind = request.scope["kind"].as_str().expect("resource kind");
                    if matches!(kind, "system" | "browser") {
                        continue;
                    }
                    let target = request
                        .target
                        .as_ref()
                        .or_else(|| approval.targets.get(&request.key))
                        .filter(|target| !target.trim().is_empty())
                        .ok_or_else(|| format!("请选择{}。", request.label))?;
                    let normalized = io::normalize_target(kind, &request.label, target)?;
                    if io::is_path_resource(kind) && normalized != *target {
                        return Err("资源路径已变化，请重新确认授权。".into());
                    }
                    targets.insert(request.key, normalized);
                }
                Ok(targets)
            };
            let targets = if requires_filesystem {
                io::check_paths(normalize)?
            } else {
                normalize()?
            };
            for (key, target) in targets {
                let (source, alias) = key.rsplit_once(':').expect("grant source and alias");
                self.sources
                    .iter_mut()
                    .find(|item| item.id == source)
                    .expect("grant source")
                    .settings
                    .insert(alias.into(), target);
            }
        }
        for source in &mut self.sources {
            validate_instance(project, source)?;
            io::settings(&project.manifest.definition, &source.settings)?;
            source.approved_permissions = project.manifest.definition.permissions.clone();
        }
        Ok(self.sources)
    }
}

fn publication_project(workspace: &Workspace, id: &str, restore: bool) -> Result<Project, String> {
    if builtin::get(id).is_some() {
        return Err("随包采集器由应用更新管理。".into());
    }
    let module = workspace.catalog.modules.get(id).ok_or("找不到采集器。")?;
    if restore {
        return module
            .previous
            .as_ref()
            .map(|release| release.project.clone())
            .ok_or("暂无可恢复版本。".into());
    }
    workspace
        .tested
        .get(id)
        .filter(|(draft, _)| draft == &module.draft)
        .map(|(_, project)| project.clone())
        .ok_or("请先试跑当前草稿并通过校验。".into())
}

#[tauri::command]
pub async fn collector_grants(
    window: tauri::WebviewWindow,
    app: AppHandle,
    target: Draft,
    restore: bool,
    live: bool,
) -> Result<Vec<GrantRequest>, String> {
    command_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let id = target.id.clone();
        let state = app.state::<Mutex<Workspace>>();
        let project = if live {
            let draft = state
                .lock()
                .map_err(|error| error.to_string())?
                .draft(&id)?;
            if !draft.same_source(&target) {
                return Err("脚本已变更，请重新试跑。".into());
            }
            draft.inspected_project(false, false)?
        } else {
            let state = state.lock().map_err(|error| error.to_string())?;
            publication_project(&state, &id, restore)?
        };
        let state = state.lock().map_err(|error| error.to_string())?;
        let current = state
            .catalog
            .modules
            .get(&id)
            .and_then(|module| module.released.as_ref());
        Ok(plan_sources(
            &project,
            current.map(|release| &release.project),
            &state.catalog,
            live,
        )?
        .requests)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn test_collector(
    window: tauri::WebviewWindow,
    app: AppHandle,
    target: Draft,
    live: bool,
    approval: Option<Approval>,
) -> Result<runner::ResultData, String> {
    command_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let id = target.id.clone();
        let state = app.state::<Mutex<Workspace>>();
        let draft = {
            let mut state = state.lock().map_err(|error| error.to_string())?;
            state.tested.remove(&id);
            let draft = state.draft(&id)?;
            if !draft.same_source(&target) {
                return Err("脚本已变更，请重新试跑。".into());
            }
            draft
        };
        let mut project = draft.inspected_project(builtin::get(&id).is_some(), !live)?;
        let mut response = if live {
            let planned = {
                let state = state.lock().map_err(|error| error.to_string())?;
                let current = state
                    .catalog
                    .modules
                    .get(&id)
                    .and_then(|module| module.released.as_ref());
                plan_sources(
                    &project,
                    current.map(|release| &release.project),
                    &state.catalog,
                    true,
                )?
            };
            let instances = planned.authorize(&project, approval)?;
            execute_live(&project, &instances[0], &http_client()?, now())?
        } else if project.manifest.sdk_api_version == 2 {
            let (settings, mut broker) =
                io::Broker::fixture(&project.manifest.definition, &project.fixture)?;
            runner::isolated_with(request(&project, settings, now()), move |call| {
                broker.call(call)
            })?
        } else {
            if project
                .manifest
                .definition
                .inputs
                .keys()
                .any(|key| project.fixture.get(key).is_none())
            {
                return Err("请在样例输入中提供模块声明的输入键。".into());
            }
            runner::isolated(request(&project, project.fixture.clone(), now()))?
        };
        if let Some(result) = &response.result {
            if let Err(error) = project.validate_outputs(result) {
                response.error = Some(error);
                response.result = None;
            }
        }
        response.definition = Some(json!(project.manifest.definition));
        let mut state = state.lock().map_err(|error| error.to_string())?;
        if response.result.is_some() && state.draft(&id)? == draft {
            project.fixture = json!({});
            state.tested.insert(id, (draft, project));
        }
        Ok(response)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn publish_collector(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    restore: bool,
    approval: Option<Approval>,
) -> Result<WorkspaceView, String> {
    command_window(&window)?;
    let workspace = app.state::<Mutex<Workspace>>();
    let mut workspace = workspace.lock().map_err(|error| error.to_string())?;
    let mut catalog = workspace.catalog.clone();
    let module = catalog.modules.get(&id).ok_or("找不到可发布的采集模块。")?;
    let project = publication_project(&workspace, &id, restore)?;
    if let Some(release) = &module.released {
        ensure_sdk_version(
            release.project.manifest.sdk_api_version,
            project.manifest.sdk_api_version,
        )?;
    }
    let current = module.released.as_ref().map(|release| &release.project);
    let sources =
        plan_sources(&project, current, &catalog, false)?.authorize(&project, approval)?;
    let new_count = sources
        .iter()
        .filter(|source| !catalog.instances.contains_key(&source.id))
        .count();
    if catalog.instances.len() + new_count > MAX_INSTANCES {
        return Err("已达到数据源上限。".into());
    }
    let device_state = app.state::<Mutex<Device>>();
    let mut device = device_state.lock().map_err(|error| error.to_string())?;
    let mut staged = device.clone();
    for instance in &sources {
        if let Some(old) = &module.released {
            for (key, previous) in &old.project.manifest.definition.widgets {
                let widget_id = old.project.widget_id(&instance.id, key);
                if device.uses_widget(&widget_id) {
                    let next = project
                        .manifest
                        .definition
                        .widgets
                        .get(key)
                        .ok_or("当前布局仍在使用此组件。")?;
                    let old_output = &old.project.manifest.definition.outputs[&previous.output_key];
                    let new_output = &project.manifest.definition.outputs[&next.output_key];
                    if next.kind != previous.kind
                        || next.format != previous.format
                        || next.chart != previous.chart
                        || next.trend != previous.trend
                        || next.range != previous.range
                        || next.output_key != previous.output_key
                        || old_output != new_output
                    {
                        return Err(
                            "此更新改变了正在使用的卡片读数契约，请先移除相关卡片并同步。".into(),
                        );
                    }
                }
            }
        }
        staged.install_widgets(&instance.id, project.widgets(&instance.id, &instance.name))?;
    }
    for instance in sources {
        staged.update_source_values(&instance.id, BTreeMap::new());
        catalog.instances.insert(instance.id.clone(), instance);
    }
    let module = catalog.modules.get_mut(&id).expect("published module");
    let revision = module
        .released
        .as_ref()
        .map_or(1, |release| release.revision + 1);
    if !restore {
        module.draft.definition = serde_json::to_string_pretty(&project.manifest.definition)
            .map_err(|error| error.to_string())?;
    }
    module.previous = module.released.replace(Release { revision, project });
    workspace.save(catalog)?;
    workspace.tested.remove(&id);
    let sources: Vec<_> = workspace
        .catalog
        .instances
        .values()
        .filter(|source| source.module_id == id)
        .map(|source| source.id.clone())
        .collect();
    for source in sources {
        workspace.status.remove(&source);
    }
    *device = staged;
    let _ = app.emit(SNAPSHOT_EVENT, crate::display_wire::local_snapshot(&device.snapshot()));
    Ok(workspace.view())
}

fn ensure_sdk_version(current: u32, next: u32) -> Result<(), String> {
    if current != next {
        return Err("更新须使用原模块的 SDK 版本。要导入不同版本，请选择作为副本导入。".into());
    }
    Ok(())
}

fn validate_instance(project: &Project, instance: &Instance) -> Result<(), String> {
    if instance.name.trim().is_empty()
        || instance.name.chars().count() > 80
        || !(5..=3600).contains(&instance.refresh_seconds)
    {
        return Err("请填写实例名称，刷新间隔须为 5–3600 秒。".into());
    }
    for (key, input) in &project.manifest.definition.inputs {
        let value = instance
            .settings
            .get(key)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("请配置{}。", input.label))?;
        match input.kind.as_str() {
            "httpJson" => {
                let url = reqwest::Url::parse(value)
                    .map_err(|_| format!("{}不是有效 URL。", input.label))?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("请输入 HTTP 或 HTTPS 地址。".into());
                }
            }
            "fileJson" if Path::new(value).is_absolute() => {}
            _ => return Err(format!("{}需要有效的绝对文件路径。", input.label)),
        }
    }
    Ok(())
}

fn request(project: &Project, inputs: Value, sampled_at: u64) -> runner::Request {
    runner::Request {
        entry: project.manifest.entry.clone(),
        files: project.files.clone(),
        inputs,
        private_settings: project
            .manifest
            .definition
            .settings
            .iter()
            .filter(|(_, setting)| setting.is_variable())
            .map(|(key, _)| key.clone())
            .collect(),
        sampled_at,
        action: None,
        state: Value::Null,
    }
}

fn open_json_file(path: impl AsRef<Path>) -> std::io::Result<fs::File> {
    let path = path.as_ref();
    let invalid =
        || std::io::Error::new(std::io::ErrorKind::InvalidInput, "expected a regular file");
    if !fs::metadata(path)?.is_file() {
        return Err(invalid());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    Ok(file)
}

#[tauri::command]
pub async fn choose_json_input(
    window: tauri::WebviewWindow,
    kind: Option<String>,
) -> Result<Option<String>, String> {
    command_window(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = window.dialog().file().set_parent(&window);
        let path = match kind.as_deref() {
            Some("directory") => dialog.blocking_pick_folder(),
            Some("file" | "process" | "stdio") => dialog.blocking_pick_file(),
            Some("fileJson") | None => dialog.add_filter("JSON", &["json"]).blocking_pick_file(),
            _ => return Err("未知的授权类型。".into()),
        };
        path.map(|path| {
            let path = path.into_path().map_err(|error| error.to_string())?;
            io::check_paths(move || {
                path.canonicalize()
                    .map_err(|_| "无法读取所选路径。")?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| "文件路径无法读取。".into())
            })
        })
        .transpose()
    })
    .await
    .map_err(|error| error.to_string())?
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())
}

fn read_inputs(
    project: &Project,
    instance: &Instance,
    client: &reqwest::blocking::Client,
) -> Result<Value, String> {
    let mut inputs = Map::new();
    for (key, input) in &project.manifest.definition.inputs {
        let value = instance
            .settings
            .get(key)
            .ok_or_else(|| format!("请配置{}。", input.label))?;
        let response: Box<dyn Read> = match input.kind.as_str() {
            "httpJson" => Box::new(
                client
                    .get(value)
                    .send()
                    .and_then(|response| response.error_for_status())
                    .map_err(|_| format!("{}请求失败。", input.label))?,
            ),
            "fileJson" => Box::new(
                open_json_file(value)
                    .map_err(|_| format!("无法读取{}，请选择普通 JSON 文件。", input.label))?,
            ),
            _ => return Err("自定义模块无法读取此输入。".into()),
        };
        let mut bytes = Vec::new();
        response
            .take(INPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "读取输入失败。")?;
        if bytes.len() > INPUT_LIMIT {
            return Err("输入超过 256 KiB。".into());
        }
        let parsed = serde_json::from_slice(&bytes)
            .map_err(|error| format!("{}返回了无效 JSON：{error}", input.label))?;
        inputs.insert(key.clone(), parsed);
    }
    Ok(Value::Object(inputs))
}

struct Schedule {
    instance: Instance,
    revision: u64,
    next_run: Instant,
    run: Option<u64>,
    history: History,
    state: Value,
}

struct Finished {
    run: u64,
    instance: Instance,
    revision: u64,
    result: Result<(u64, super::model::Collected), String>,
    interaction: Option<Interaction>,
}

fn execute_live(
    project: &Project,
    instance: &Instance,
    client: &reqwest::blocking::Client,
    at: u64,
) -> Result<runner::ResultData, String> {
    execute_with_state(project, instance, client, at, Value::Null, None)
}

fn execute_with_state(
    project: &Project,
    instance: &Instance,
    client: &reqwest::blocking::Client,
    at: u64,
    state: Value,
    interaction: Option<Interaction>,
) -> Result<runner::ResultData, String> {
    if project.manifest.sdk_api_version == 1 {
        let inputs = read_inputs(project, instance, client)?;
        return runner::isolated(request(project, inputs, at));
    }
    let definition = &project.manifest.definition;
    if definition.permissions != instance.approved_permissions {
        return Err("模块的资源授权已变化，请重新发布并确认。".into());
    }
    let settings = io::settings(definition, &instance.settings)?;
    let mut broker = io::Broker::live(definition, instance.settings.clone(), client.clone());
    let mut job = request(project, settings, at);
    job.state = state;
    if let Some(action) = interaction {
        job.action = Some(action.name);
        broker = broker.interactive(action.cancelled);
    }
    runner::isolated_with(job, move |call| broker.call(call))
}

fn collect_instance(
    project: &Project,
    instance: &Instance,
    client: &reqwest::blocking::Client,
    state: Value,
    interaction: Option<Interaction>,
) -> Result<(u64, super::model::Collected), String> {
    let at = now();
    let response = execute_with_state(project, instance, client, at, state, interaction)?;
    let result = response
        .result
        .ok_or_else(|| response.error.unwrap_or_else(|| "采集无结果。".into()))?;
    project.validate_outputs(&result)?;
    Ok((at, result))
}

pub fn run_cli(arguments: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let path = arguments.next().ok_or(
        "用法：mado-studio --collect <插件目录> [--grant 名称=路径] [--setting 名称=值] [--action 操作]",
    )?;
    let mut project = transfer::read_project(Path::new(&path), true)?;
    if project.manifest.definition_in_script {
        let draft = Draft::from_project(&project);
        project = draft.inspected_project(true, false)?;
    }
    let definition = &project.manifest.definition;
    let mut bindings = BTreeMap::new();
    let mut interaction = None;
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("缺少 {flag} 的值。"))?;
        if flag == "--action" {
            if interaction.is_some() || !definition.actions.contains(&value) {
                return Err("操作未声明或重复指定。".into());
            }
            interaction = Some(Interaction {
                name: value,
                cancelled: Arc::new(AtomicBool::new(false)),
            });
            continue;
        }
        let (alias, value) = value.split_once('=').ok_or("参数应为 名称=值。")?;
        let value = match flag.as_str() {
            "--grant" => {
                let (kind, label) = if let Some(permission) = definition.permissions.get(alias) {
                    let scope = json!(permission);
                    let kind = scope["kind"].as_str().unwrap_or_default();
                    if matches!(kind, "browser" | "system") {
                        return Err("此能力无需绑定路径。".into());
                    }
                    (kind.to_owned(), permission.label())
                } else if let Some(input) = definition.inputs.get(alias) {
                    (input.kind.clone(), input.label.as_str())
                } else {
                    return Err(format!("插件未声明资源 {alias}。"));
                };
                io::normalize_target(&kind, label, value)?
            }
            "--setting" if definition.settings.contains_key(alias) => value.to_owned(),
            _ => return Err(format!("未知参数 {flag} 或未声明设置 {alias}。")),
        };
        if bindings.insert(alias.into(), value).is_some() {
            return Err(format!("参数 {alias} 重复。"));
        }
    }
    let instance = Instance {
        id: project.manifest.module_id.clone(),
        module_id: project.manifest.module_id.clone(),
        name: definition.name.clone(),
        settings: bindings,
        approved_permissions: definition.permissions.clone(),
        refresh_seconds: 60,
        enabled: true,
    };
    validate_instance(&project, &instance)?;
    let (_, mut result) = collect_instance(
        &project,
        &instance,
        &http_client()?,
        Value::Null,
        interaction,
    )?;
    result.state = Value::Null;
    let output = serde_json::to_string(&result).map_err(|error| error.to_string())?;
    println!("{output}");
    Ok(())
}

fn poll_sources(app: AppHandle) {
    let Ok(client) = http_client() else { return };
    let state = app.state::<Mutex<Workspace>>();
    let mut tasks = BTreeMap::<String, Schedule>::new();
    let (sender, receiver) = std::sync::mpsc::channel::<Finished>();
    let mut active = 0;
    let mut sequence = 0;
    loop {
        let Ok(mut workspace) = state.lock() else {
            return;
        };
        for finished in receiver.try_iter() {
            active -= 1;
            let cancelled = finished
                .interaction
                .as_ref()
                .is_some_and(|interaction| interaction.cancelled.load(Ordering::Relaxed));
            if let Some(interaction) = &finished.interaction {
                if workspace
                    .interactions
                    .get(&finished.instance.id)
                    .is_some_and(|active| Arc::ptr_eq(&active.cancelled, &interaction.cancelled))
                {
                    workspace.interactions.remove(&finished.instance.id);
                    workspace.sequence += 1;
                }
            }
            let Some(task) = tasks.get_mut(&finished.instance.id) else {
                continue;
            };
            if task.run != Some(finished.run) {
                continue;
            }
            task.run = None;
            task.next_run = Instant::now() + Duration::from_secs(task.instance.refresh_seconds);
            if cancelled {
                task.next_run = Instant::now();
                continue;
            }
            let instance = &finished.instance;
            if workspace.catalog.instances.get(&instance.id) != Some(instance) {
                continue;
            }
            let Some(release) = workspace
                .catalog
                .modules
                .get(&instance.module_id)
                .and_then(|module| module.released.as_ref())
                .filter(|release| release.revision == finished.revision)
            else {
                continue;
            };
            match finished.result {
                Ok((at, result)) => {
                    task.state = result.state;
                    let values = task.history.project(
                        &release.project,
                        &instance.id,
                        at,
                        &result.outputs,
                        &result.notifications,
                    );
                    workspace.status.insert(
                        instance.id.clone(),
                        Status {
                            updated_at: Some(at),
                            error: None,
                            data: result.status,
                        },
                    );
                    super::publish_readings(&app, &instance.id, values);
                }
                Err(error) => {
                    workspace
                        .status
                        .entry(instance.id.clone())
                        .or_default()
                        .error = Some(error);
                }
            }
            workspace.sequence += 1;
            let _ = app.emit(STATUS_EVENT, &instance.id);
        }
        tasks.retain(|id, _| {
            workspace
                .catalog
                .instances
                .get(id)
                .is_some_and(|instance| instance.enabled)
        });
        for (id, status) in &workspace.status {
            let Some(instance) = workspace.catalog.instances.get(id) else {
                continue;
            };
            if instance.enabled
                && status
                    .updated_at
                    .is_some_and(|at| now().saturating_sub(at) >= instance.refresh_seconds * 3000)
            {
                super::publish_readings(&app, id, BTreeMap::new());
            }
        }
        let refresh = std::mem::take(&mut workspace.refresh);
        let mut refresh_pending = std::collections::BTreeSet::new();
        for id in &refresh {
            if let Some(task) = tasks.get_mut(id) {
                if task.run.is_none() {
                    task.next_run = Instant::now();
                    task.state = Value::Null;
                } else {
                    refresh_pending.insert(id.clone());
                }
            }
        }
        for instance in workspace
            .catalog
            .instances
            .values()
            .filter(|instance| instance.enabled)
        {
            if active >= 3 {
                break;
            }
            let Some(release) = workspace
                .catalog
                .modules
                .get(&instance.module_id)
                .and_then(|module| module.released.as_ref())
            else {
                continue;
            };
            let reset = tasks
                .get(&instance.id)
                .is_none_or(|task| task.instance != *instance || task.revision != release.revision);
            if reset {
                tasks.insert(
                    instance.id.clone(),
                    Schedule {
                        instance: instance.clone(),
                        revision: release.revision,
                        next_run: Instant::now(),
                        run: None,
                        history: History::default(),
                        state: Value::Null,
                    },
                );
            }
            let task = tasks.get_mut(&instance.id).expect("scheduled instance");
            let interaction = workspace.interactions.get(&instance.id).cloned();
            if task.run.is_none() && (task.next_run <= Instant::now() || interaction.is_some()) {
                let instance = instance.clone();
                let release = release.clone();
                let sender = sender.clone();
                let client = client.clone();
                let state = task.state.clone();
                sequence += 1;
                let run = sequence;
                task.run = Some(run);
                active += 1;
                thread::spawn(move || {
                    let result = collect_instance(
                        &release.project,
                        &instance,
                        &client,
                        state,
                        interaction.clone(),
                    );
                    let _ = sender.send(Finished {
                        run,
                        instance,
                        revision: release.revision,
                        result,
                        interaction,
                    });
                });
            }
        }
        workspace.refresh.extend(refresh_pending);
        drop(workspace);
        thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIABLE_VALUE: &str = "不应出现在本机配置的变量";
    const VARIABLE_KEY: &str = "collector-variables.key";

    fn variable_catalog() -> (Catalog, String) {
        let mut project = create_template("变量".into(), "sample")
            .unwrap()
            .project(false)
            .unwrap();
        project.manifest.definition.settings = serde_json::from_value(json!({
            "API_TOKEN": { "label": "令牌", "description": "测试变量", "secret": true },
            "PUBLIC_VALUE": { "label": "公开变量", "required": false },
            "legacy_option": { "label": "历史参数" }
        }))
        .unwrap();
        project.manifest.definition.permissions.insert(
            "source".into(),
            Permission::File {
                label: "文件".into(),
            },
        );
        project.files.insert(
            project.manifest.entry.clone(),
            r#"
            export default { collect(settings) {
                return { outputs: { reading: { value: settings.API_TOKEN.length } } };
            }};
        "#
            .into(),
        );
        project.validate(false).unwrap();
        let module_id = project.manifest.module_id.clone();
        let id = format!("collector:{module_id}");
        let instance = Instance {
            id: id.clone(),
            module_id: module_id.clone(),
            name: "变量".into(),
            settings: BTreeMap::from([
                ("API_TOKEN".into(), VARIABLE_VALUE.into()),
                ("PUBLIC_VALUE".into(), "test-public-variable".into()),
                ("legacy_option".into(), "legacy-text".into()),
                ("source".into(), "/path/to/authorized.json".into()),
            ]),
            approved_permissions: project.manifest.definition.permissions.clone(),
            refresh_seconds: 60,
            enabled: true,
        };
        let catalog = Catalog {
            instances: BTreeMap::from([(id.clone(), instance)]),
            modules: BTreeMap::from([(
                module_id,
                Module {
                    draft: Draft::from_project(&project),
                    released: Some(Release {
                        revision: 1,
                        project,
                    }),
                    previous: None,
                },
            )]),
        };
        (catalog, id)
    }

    #[test]
    fn encrypted_variables_hide_values_and_reach_the_script_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collectors.json");
        let mut workspace = Workspace::load(path.clone()).unwrap();
        let (catalog, id) = variable_catalog();
        workspace.save(catalog).unwrap();

        let status = workspace.instance_status(&id).unwrap();
        assert!(!status.to_string().contains(VARIABLE_VALUE));
        assert!(!status.to_string().contains("test-public-variable"));
        assert_eq!(status["settings"]["legacy_option"], "legacy-text");
        assert_eq!(status["settings"]["source"], "/path/to/authorized.json");
        let bytes = fs::read_to_string(&path).unwrap();
        assert!(!bytes.contains(VARIABLE_VALUE));
        assert!(!bytes.contains("test-public-variable"));
        let stored: Value = serde_json::from_str(&bytes).unwrap();
        assert_eq!(
            stored["instances"][&id]["settings"]["legacy_option"],
            "legacy-text"
        );
        assert_eq!(
            stored["instances"][&id]["settings"]["source"],
            "/path/to/authorized.json"
        );

        let restarted = Workspace::load(path).unwrap();
        let instance = &restarted.catalog.instances[&id];
        let project = &restarted.catalog.modules[&instance.module_id]
            .released
            .as_ref()
            .unwrap()
            .project;
        let settings = io::settings(&project.manifest.definition, &instance.settings).unwrap();
        assert_eq!(settings["API_TOKEN"], VARIABLE_VALUE);
        let result = runner::evaluate(request(project, settings, 1000))
            .result
            .unwrap();
        assert_eq!(result.outputs["reading"]["value"], 12);
    }

    #[test]
    fn variable_defaults_are_encrypted_and_restored_with_the_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collectors.json");
        let mut workspace = Workspace::load(path.clone()).unwrap();
        let (mut catalog, id) = variable_catalog();
        let instance = catalog.instances.get_mut(&id).unwrap();
        instance.settings.retain(|key, _| key == "source");
        let module_id = instance.module_id.clone();
        let module = catalog.modules.get_mut(&module_id).unwrap();
        let release = module.released.as_mut().unwrap();
        let definitions = &mut release.project.manifest.definition.settings;
        definitions.get_mut("legacy_option").unwrap().default = "legacy-default".into();
        definitions.get_mut("API_TOKEN").unwrap().default = "release-default-token".into();

        let mut previous = release.clone();
        previous
            .project
            .manifest
            .definition
            .settings
            .get_mut("API_TOKEN")
            .unwrap()
            .default = "previous-default-token".into();
        let mut draft_project = release.project.clone();
        draft_project
            .manifest
            .definition
            .settings
            .get_mut("API_TOKEN")
            .unwrap()
            .default = "draft-default-token".into();
        draft_project
            .files
            .values_mut()
            .next()
            .unwrap()
            .push_str("\n// author source includes draft-default-token");
        module.draft = Draft::from_project(&draft_project);
        module.previous = Some(previous);

        workspace.save(catalog.clone()).unwrap();
        let bytes = fs::read_to_string(&path).unwrap();
        for default in [
            "draft-default-token",
            "release-default-token",
            "previous-default-token",
        ] {
            assert!(!bytes.contains(default));
        }
        assert!(bytes.contains("/path/to/authorized.json"));

        let mut restarted = Workspace::load(path.clone()).unwrap();
        let module = &restarted.catalog.modules[&module_id];
        assert_eq!(module.draft, catalog.modules[&module_id].draft);
        let project = &module.released.as_ref().unwrap().project;
        let settings = io::settings(
            &project.manifest.definition,
            &restarted.catalog.instances[&id].settings,
        )
        .unwrap();
        assert_eq!(settings["API_TOKEN"], "release-default-token");
        assert_eq!(settings["legacy_option"], "legacy-default");
        let result = runner::evaluate(request(project, settings, 1000))
            .result
            .unwrap();
        assert_eq!(result.outputs["reading"]["value"], 21);
        assert_eq!(
            module
                .previous
                .as_ref()
                .unwrap()
                .project
                .manifest
                .definition
                .settings["API_TOKEN"]
                .default,
            "previous-default-token"
        );

        let mut next = restarted.catalog.clone();
        next.instances
            .get_mut(&id)
            .unwrap()
            .settings
            .insert("API_TOKEN".into(), VARIABLE_VALUE.into());
        restarted.save(next).unwrap();
        let bytes = fs::read_to_string(&path).unwrap();
        assert!(!bytes.contains(VARIABLE_VALUE));
        let reopened = Workspace::load(path.clone()).unwrap();
        assert_eq!(
            reopened.catalog.instances[&id].settings["API_TOKEN"],
            VARIABLE_VALUE
        );

        let key_path = directory.path().join(VARIABLE_KEY);
        fs::remove_file(&key_path).unwrap();
        assert!(Workspace::load(path.clone())
            .err()
            .unwrap()
            .to_string()
            .contains("密钥"));
        assert!(!key_path.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), bytes);
    }

    #[test]
    fn variable_defaults_in_incomplete_drafts_are_encrypted_without_blocking_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collectors.json");
        let mut workspace = Workspace::load(path.clone()).unwrap();
        let (mut catalog, id) = variable_catalog();
        let module_id = catalog.instances.remove(&id).unwrap().module_id;
        let module = catalog.modules.get_mut(&module_id).unwrap();
        module.released = None;
        let definition =
            r#"{"settings":{"API_TOKEN":{"secret":true,"default":"incomplete-draft-token"}}}"#;
        module.draft.definition = definition.into();
        assert!(module.draft.project(false).is_err());

        workspace.save(catalog).unwrap();
        assert!(!fs::read_to_string(&path)
            .unwrap()
            .contains("incomplete-draft-token"));
        let restarted = Workspace::load(path).unwrap();
        assert_eq!(
            restarted.catalog.modules[&module_id].draft.definition,
            definition
        );
    }

    #[test]
    fn encrypted_variables_migrate_legacy_plaintext_and_remove_deleted_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collectors.json");
        let (catalog, id) = variable_catalog();
        fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        let mut workspace = Workspace::load(path.clone()).unwrap();
        assert_eq!(
            workspace.catalog.instances[&id].settings["API_TOKEN"],
            VARIABLE_VALUE
        );
        workspace.save(workspace.catalog.clone()).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains(VARIABLE_VALUE));

        let mut restarted = Workspace::load(path.clone()).unwrap();
        let mut next = restarted.catalog.clone();
        let instance = next.instances.get_mut(&id).unwrap();
        assert_eq!(instance.settings["API_TOKEN"], VARIABLE_VALUE);
        instance.settings.remove("API_TOKEN");
        restarted.save(next).unwrap();

        let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(stored["instances"][&id]["settings"]
            .get("API_TOKEN")
            .is_none());
        let mut next = restarted.catalog.clone();
        next.instances.remove(&id);
        restarted.save(next).unwrap();
        let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(stored["instances"].get(&id).is_none());
    }

    #[test]
    fn encrypted_variables_fail_closed_when_the_key_is_lost_or_changed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collectors.json");
        let key_path = directory.path().join(VARIABLE_KEY);
        let mut workspace = Workspace::load(path.clone()).unwrap();
        let (catalog, _) = variable_catalog();
        workspace.save(catalog).unwrap();
        let original = fs::read(&path).unwrap();
        fs::remove_file(&key_path).unwrap();
        assert!(Workspace::load(path.clone())
            .err()
            .unwrap()
            .to_string()
            .contains("密钥"));
        assert!(workspace.save(workspace.catalog.clone()).is_err());
        assert!(!key_path.exists());
        assert_eq!(fs::read(&path).unwrap(), original);

        for damaged in [vec![42; 15], vec![42; 32]] {
            fs::write(&key_path, &damaged).unwrap();
            let error = Workspace::load(path.clone()).err().unwrap().to_string();
            assert!(error.contains("密钥"), "{error}");
            assert!(!error.contains(VARIABLE_VALUE));
            assert_eq!(fs::read(&key_path).unwrap(), damaged);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn encrypted_variables_do_not_echo_in_script_status_errors_or_logs() {
        let (catalog, id) = variable_catalog();
        let instance = &catalog.instances[&id];
        let mut project = catalog.modules[&instance.module_id]
            .released
            .as_ref()
            .unwrap()
            .project
            .clone();
        let settings = io::settings(&project.manifest.definition, &instance.settings).unwrap();
        project.files.insert(
            project.manifest.entry.clone(),
            r#"
            export default { collect(settings, context) {
                context.log('a'.repeat(395) + settings.API_TOKEN);
                context.log('ordinary diagnostic');
                return { outputs: { reading: { value: 42 } },
                    status: { [settings.API_TOKEN]: settings.PUBLIC_VALUE } };
            }};
        "#
            .into(),
        );
        let response = runner::evaluate(request(&project, settings.clone(), 1000));
        assert!(!response
            .result
            .unwrap()
            .status
            .to_string()
            .contains(VARIABLE_VALUE));
        assert!(!response.logs.join(" ").contains("不应出现在"));
        assert!(response.logs.iter().any(|log| log == "ordinary diagnostic"));

        project.files.insert(
            project.manifest.entry.clone(),
            r#"
            export default { collect(settings) {
                throw new Error('a'.repeat(2990) + settings.API_TOKEN);
            }};
        "#
            .into(),
        );
        let response = runner::evaluate(request(&project, settings, 1000));
        let error = response.error.unwrap();
        assert!(!error.contains("不应出"));
    }

    #[test]
    fn templates_assign_metadata_and_expose_only_author_fields() {
        let first = create_template("温度".into(), "http").unwrap();
        let second = create_template("温度".into(), "http").unwrap();
        assert_ne!(first.id, second.id);
        assert!(uuid::Uuid::parse_str(&first.id).is_ok());
        let definition: Value = serde_json::from_str(&first.definition).unwrap();
        assert!(definition.get("sdkApiVersion").is_none());
        assert!(definition.get("moduleId").is_none());
        assert!(definition.get("entry").is_none());
        let project = first.project(false).unwrap();
        assert_eq!(project.manifest.sdk_api_version, 2);
        let (settings, mut broker) =
            io::Broker::fixture(&project.manifest.definition, &project.fixture).unwrap();
        let response = runner::evaluate_with(request(&project, settings, 1000), move |call| {
            broker.call(call)
        });
        assert!(response.error.is_none(), "{:?}", response.error);
        let result = response.result.unwrap();
        project.validate_outputs(&result).unwrap();
        assert_eq!(result.outputs["reading"]["value"], 42);
    }

    #[test]
    fn migration_wraps_an_old_entry_without_moving_its_relative_imports() {
        let mut draft = create_template("旧模块".into(), "sample").unwrap();
        draft.definition_in_script = false;
        let entry = draft.entry.clone();
        let old = "import { value } from './helper.mjs'; export default { collect() { return { outputs: { reading: { value } } } } };";
        draft.files.insert(entry.clone(), old.into());
        draft
            .files
            .insert("src/helper.mjs".into(), "export const value = 83;".into());
        let migrated = draft.migrate().unwrap();
        assert_ne!(migrated.entry, entry);
        assert_eq!(migrated.files[&entry], old);

        let project = migrated.project(false).unwrap();
        let inspected = runner::inspect_code(request(&project, Value::Null, 0)).unwrap();
        assert_eq!(inspected["name"], "旧模块");
        let result = runner::evaluate(request(&project, json!({}), 0))
            .result
            .unwrap();
        assert_eq!(result.outputs["reading"]["value"], 83);
    }

    #[test]
    fn full_legacy_projects_keep_nested_files_and_literal_aliases() {
        let mut draft = create_template("旧模块".into(), "sample").unwrap();
        draft.definition_in_script = false;
        let mut definition: Value = serde_json::from_str(&draft.definition).unwrap();
        definition["settings"] =
            json!({ "__proto__": { "label": "保留的别名", "default": "value" } });
        draft.definition = definition.to_string();
        draft
            .files
            .insert("src/collector.mjs/helper.mjs".into(), "export {}".into());
        for index in 2..64 {
            draft
                .files
                .insert(format!("src/helper-{index}.mjs"), "export {}".into());
        }

        let migrated = draft.migrate().unwrap();
        assert_eq!(migrated.files.len(), 65);
        assert_ne!(migrated.entry, "src/collector.mjs");
        let project = migrated.project(false).unwrap();
        let inspected = runner::inspect_code(request(&project, Value::Null, 0)).unwrap();
        assert_eq!(inspected["settings"]["__proto__"]["default"], "value");
    }

    #[test]
    fn v1_project_keeps_its_input_contract() {
        let mut project = create_template("旧模块".into(), "sample")
            .unwrap()
            .project(false)
            .unwrap();
        project.manifest.sdk_api_version = 1;
        project.manifest.definition_in_script = false;
        project.manifest.definition.inputs = BTreeMap::from([(
            "source".into(),
            super::super::model::Input {
                kind: "fileJson".into(),
                label: "旧输入".into(),
                provider: None,
            },
        )]);
        project.files.insert(
            project.manifest.entry.clone(),
            r#"
            export default { collect(inputs) {
                return { outputs: { reading: { value: inputs.source.value } } }
            } }
        "#
            .into(),
        );
        project.fixture = json!({ "source": { "value": 42 } });
        let legacy = Draft::from_project(&project).project(false).unwrap();
        assert_eq!(legacy.manifest.sdk_api_version, 1);
        let result = runner::evaluate(request(&legacy, legacy.fixture.clone(), 0))
            .result
            .unwrap();
        legacy.validate_outputs(&result).unwrap();
        assert_eq!(result.outputs["reading"]["value"], 42);
        assert!(ensure_sdk_version(2, 1).is_err());
    }

    #[test]
    fn publishing_registers_one_stable_source_with_the_project_schedule() {
        let mut project = create_template("温度".into(), "sample")
            .unwrap()
            .project(false)
            .unwrap();
        let mut catalog = Catalog::default();
        let sources = plan_sources(&project, None, &catalog, false)
            .unwrap()
            .authorize(&project, None)
            .unwrap();
        assert_eq!(sources.len(), 1);
        let source = sources.into_iter().next().unwrap();
        let original_id = source.id.clone();
        assert!(source.enabled);
        assert_eq!(source.refresh_seconds, 60);
        catalog.instances.insert(source.id.clone(), source);

        let schedule = project.manifest.definition.schedule.as_mut().unwrap();
        schedule.refresh_seconds = 17;
        schedule.enabled = false;
        let sources = plan_sources(&project, None, &catalog, false)
            .unwrap()
            .authorize(&project, None)
            .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, original_id);
        assert_eq!(sources[0].refresh_seconds, 17);
        assert!(!sources[0].enabled);
    }

    #[test]
    fn a_declared_target_is_confirmed_once_and_cannot_be_replaced_by_the_dialog() {
        let mut project = create_template("温度".into(), "http")
            .unwrap()
            .project(false)
            .unwrap();
        let mut catalog = Catalog::default();
        let plan = plan_sources(&project, None, &catalog, false).unwrap();
        let request = plan.requests[0].clone();
        assert_eq!(
            request.target.as_deref(),
            Some("https://example.com/metrics")
        );
        assert!(plan.authorize(&project, None).is_err());

        let plan = plan_sources(&project, None, &catalog, false).unwrap();
        let approved = Approval {
            requests: vec![request.clone()],
            targets: BTreeMap::from([(request.key, "https://other.test/".into())]),
        };
        let source = plan
            .authorize(&project, Some(approved))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(source.settings["source"], "https://example.com/metrics");
        catalog.instances.insert(source.id.clone(), source);
        assert!(plan_sources(&project, Some(&project), &catalog, false)
            .unwrap()
            .requests
            .is_empty());

        let previous = project.clone();
        project
            .manifest
            .definition
            .resources
            .insert("source".into(), "https://other.test/".into());
        let plan = plan_sources(&project, Some(&previous), &catalog, false).unwrap();
        assert_eq!(plan.requests.len(), 1);
        assert!(plan.authorize(&project, None).is_err());
    }

    #[test]
    fn legacy_sources_keep_their_card_identity_and_settings() {
        let mut project = create_template("旧模块".into(), "http")
            .unwrap()
            .project(false)
            .unwrap();
        project.manifest.definition.schedule = None;
        project.manifest.definition.resources.clear();
        let mut catalog = Catalog::default();
        for (id, url) in [("old-a", "https://a.test/"), ("old-b", "https://b.test/")] {
            catalog.instances.insert(
                id.into(),
                Instance {
                    id: id.into(),
                    module_id: project.manifest.module_id.clone(),
                    name: id.into(),
                    settings: BTreeMap::from([("source".into(), url.into())]),
                    approved_permissions: project.manifest.definition.permissions.clone(),
                    refresh_seconds: 30,
                    enabled: false,
                },
            );
        }
        let plan = plan_sources(&project, Some(&project), &catalog, false).unwrap();
        assert!(plan.requests.is_empty());
        let sources = plan.authorize(&project, None).unwrap();
        assert_eq!(sources, catalog.instances.into_values().collect::<Vec<_>>());
    }

    #[test]
    fn removing_a_grant_does_not_copy_its_target_into_script_settings() {
        let mut before = create_template("旧模块".into(), "http")
            .unwrap()
            .project(false)
            .unwrap();
        before.manifest.definition.schedule = None;
        before.manifest.definition.resources.clear();
        let source = Instance {
            id: "old".into(),
            module_id: before.manifest.module_id.clone(),
            name: "旧模块".into(),
            settings: BTreeMap::from([(
                "source".into(),
                "https://example.test/?token=private".into(),
            )]),
            approved_permissions: before.manifest.definition.permissions.clone(),
            refresh_seconds: 60,
            enabled: true,
        };
        let catalog = Catalog {
            instances: BTreeMap::from([(source.id.clone(), source)]),
            ..Catalog::default()
        };
        let mut after = before.clone();
        after.manifest.definition.permissions.clear();
        after.manifest.definition.settings.insert(
            "source".into(),
            super::super::model::Setting {
                label: "普通参数".into(),
                default: "safe".into(),
                description: None,
                required: None,
                secret: None,
            },
        );
        let plan = plan_sources(&after, Some(&before), &catalog, false).unwrap();
        let sources = plan.authorize(&after, None).unwrap();
        assert!(!sources[0].settings.contains_key("source"));
    }

    #[cfg(unix)]
    #[test]
    fn file_grant_uses_the_resolved_target_and_a_moved_file_can_be_reselected() {
        let root = std::env::temp_dir().join(format!("tdisplay-plan-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("data.json");
        fs::write(&file, "{}").unwrap();
        let link = root.join("alias.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();

        let mut project = create_template("文件".into(), "file")
            .unwrap()
            .project(false)
            .unwrap();
        project
            .manifest
            .definition
            .resources
            .insert("source".into(), link.to_str().unwrap().into());
        let mut catalog = Catalog::default();
        let plan = plan_sources(&project, None, &catalog, false).unwrap();
        let resolved = file.canonicalize().unwrap();
        assert_eq!(plan.requests[0].target.as_deref(), resolved.to_str());
        let approval = Approval {
            requests: plan.requests.clone(),
            targets: BTreeMap::new(),
        };
        let source = plan
            .authorize(&project, Some(approval))
            .unwrap()
            .pop()
            .unwrap();
        catalog.instances.insert(source.id.clone(), source);
        let mut portable = project.clone();
        portable.manifest.definition.resources.clear();
        fs::remove_file(&file).unwrap();
        let plan = plan_sources(&portable, Some(&project), &catalog, true).unwrap();
        assert_eq!(plan.requests.len(), 1);
        assert!(plan.requests[0].target.is_none());

        portable
            .manifest
            .definition
            .schedule
            .as_mut()
            .unwrap()
            .enabled = false;
        let plan = plan_sources(&portable, Some(&project), &catalog, false).unwrap();
        assert!(plan.requests.is_empty());
        assert!(!plan.authorize(&portable, None).unwrap()[0].enabled);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scheduled_execution_requires_fresh_permission_approval_after_publish() {
        let mut project = create_template("测试".into(), "http")
            .unwrap()
            .project(false)
            .unwrap();
        let mut instance = Instance {
            id: "existing".into(),
            module_id: project.manifest.module_id.clone(),
            name: "测试".into(),
            settings: BTreeMap::from([("source".into(), "http://127.0.0.1:1".into())]),
            approved_permissions: BTreeMap::new(),
            refresh_seconds: 60,
            enabled: true,
        };
        instance.approved_permissions = project.manifest.definition.permissions.clone();
        let Permission::Http { methods, .. } = project
            .manifest
            .definition
            .permissions
            .get_mut("source")
            .unwrap()
        else {
            panic!("HTTP template should declare an HTTP permission");
        };
        methods.push("POST".into());
        let error = execute_live(&project, &instance, &http_client().unwrap(), 0).unwrap_err();
        assert!(error.contains("重新发布并确认"));
    }

    #[test]
    fn rejects_chart_metadata_on_a_metric_before_scheduling() {
        let mut project = create_template("温度".into(), "sample")
            .unwrap()
            .project(false)
            .unwrap();
        project
            .manifest
            .definition
            .widgets
            .get_mut("reading")
            .unwrap()
            .chart = Some(super::super::model::Chart {
            series: "bar".into(),
            range_seconds: 3600,
            sample_count: 0,
            min: 0.0,
            max: 100.0,
        });
        assert!(project.validate(false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_named_pipes_before_opening_json_inputs() {
        use std::os::unix::ffi::OsStrExt;
        let path =
            std::env::temp_dir().join(format!("tdisplay-test-{}.json", uuid::Uuid::new_v4()));
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let result = open_json_file(&path);
        fs::remove_file(path).unwrap();
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
    }
}
