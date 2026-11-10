use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager, State, Theme};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppTheme {
    Light,
    Dark,
    #[default]
    System,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayTheme {
    Light,
    #[default]
    Dark,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    app_theme: AppTheme,
    display_theme: DisplayTheme,
    revision: u64,
}
pub struct AppearanceStore {
    path: PathBuf,
    value: Appearance,
}
impl AppearanceStore {
    fn load(path: PathBuf) -> Result<Self, String> {
        let value = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Appearance::default(),
            Err(error) => return Err(error.to_string()),
        };
        Ok(Self { path, value })
    }
    fn update(
        &mut self,
        app_theme: Option<AppTheme>,
        display_theme: Option<DisplayTheme>,
    ) -> Result<Appearance, String> {
        let mut next = self.value.clone();
        if let Some(theme) = app_theme {
            next.app_theme = theme;
        }
        if let Some(theme) = display_theme {
            next.display_theme = theme;
        }
        if next == self.value {
            return Ok(next);
        }
        next.revision += 1;
        let parent = self.path.parent().ok_or("找不到外观配置目录。")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&next).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(&temporary, &self.path).map_err(|error| error.to_string())?;
        self.value = next.clone();
        Ok(next)
    }
}
fn apply_native_theme(app: &AppHandle, theme: AppTheme) {
    app.set_theme(match theme {
        AppTheme::Light => Some(Theme::Light),
        AppTheme::Dark => Some(Theme::Dark),
        AppTheme::System => None,
    });
}
pub fn start(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_config_dir()?.join("appearance.json");
    let store = AppearanceStore::load(path.clone()).unwrap_or_else(|error| {
        eprintln!("无法恢复外观设置：{error}");
        AppearanceStore {
            path,
            value: Appearance::default(),
        }
    });
    apply_native_theme(app, store.value.app_theme);
    app.manage(Mutex::new(store));
    Ok(())
}
#[tauri::command]
pub fn get_appearance(store: State<'_, Mutex<AppearanceStore>>) -> Result<Appearance, String> {
    Ok(store
        .lock()
        .map_err(|error| error.to_string())?
        .value
        .clone())
}
#[tauri::command]
pub fn update_appearance(
    app: AppHandle,
    store: State<'_, Mutex<AppearanceStore>>,
    app_theme: Option<AppTheme>,
    display_theme: Option<DisplayTheme>,
) -> Result<Appearance, String> {
    let mut store = store.lock().map_err(|error| error.to_string())?;
    let previous_theme = store.value.app_theme;
    let value = store
        .update(app_theme, display_theme)
        .map_err(|error| format!("保存外观失败：{error}"))?;
    if previous_theme != value.app_theme {
        apply_native_theme(&app, value.app_theme);
    }
    let _ = app.emit("appearance-changed", &value);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("tdisplay-appearance-{}", uuid::Uuid::new_v4()))
            .join("appearance.json")
    }
    #[test]
    fn defaults_and_partial_older_preferences_are_supported() {
        let defaults: Appearance = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.app_theme, AppTheme::System);
        assert_eq!(defaults.display_theme, DisplayTheme::Dark);
        let old: Appearance = serde_json::from_str(r#"{"appTheme":"dark"}"#).unwrap();
        assert_eq!(old.app_theme, AppTheme::Dark);
        assert_eq!(old.display_theme, DisplayTheme::Dark);
        assert!(serde_json::from_str::<Appearance>(r#"{"displayTheme":"invalid"}"#).is_err());
    }
    #[test]
    fn themes_persist_independently_and_updates_merge() {
        let path = path();
        let mut store = AppearanceStore::load(path.clone()).unwrap();
        store.update(Some(AppTheme::Dark), None).unwrap();
        let result = store.update(None, Some(DisplayTheme::Light)).unwrap();
        assert_eq!(result.app_theme, AppTheme::Dark);
        assert_eq!(result.display_theme, DisplayTheme::Light);
        assert_eq!(result.revision, 2);
        assert_eq!(AppearanceStore::load(path.clone()).unwrap().value, result);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn failed_save_keeps_last_preferences() {
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"blocker").unwrap();
        let mut store = AppearanceStore {
            path: path.join("blocked.json"),
            value: Appearance::default(),
        };
        assert!(store
            .update(Some(AppTheme::Light), Some(DisplayTheme::Light))
            .is_err());
        assert_eq!(store.value, Appearance::default());
        assert_eq!(std::fs::read(&path).unwrap(), b"blocker");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
