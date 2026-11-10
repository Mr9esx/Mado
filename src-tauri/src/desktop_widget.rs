use crate::device::Device;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

// Keep the existing label so the display window retains its event/drag permissions.
const WINDOW_LABEL: &str = "display";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Preferences {
    always_on_top: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            always_on_top: true,
        }
    }
}

pub struct WidgetSettings {
    path: PathBuf,
    preferences: Preferences,
}

impl WidgetSettings {
    fn load(path: PathBuf) -> Self {
        let preferences = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Preferences::default(),
        };
        Self { path, preferences }
    }

    fn save_pinned(&mut self, pinned: bool) -> Result<(), String> {
        let next = Preferences {
            always_on_top: pinned,
        };
        let parent = self.path.parent().ok_or("找不到桌面控件配置目录。")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(&next).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(&temporary, &self.path).map_err(|error| error.to_string())?;
        self.preferences = next;
        Ok(())
    }
}

pub fn start(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(Mutex::new(WidgetSettings::load(
        app.path().app_config_dir()?.join("desktop-widget.json"),
    )));
    Ok(())
}

#[tauri::command]
pub async fn open_desktop_widget(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.show().map_err(|error| error.to_string())?;
        window.unminimize().map_err(|error| error.to_string())?;
        return window.set_focus().map_err(|error| error.to_string());
    }
    let (width, height) = app
        .state::<Mutex<Device>>()
        .lock()
        .map_err(|error| error.to_string())?
        .display_size();
    let pinned = app
        .state::<Mutex<WidgetSettings>>()
        .lock()
        .map_err(|error| error.to_string())?
        .preferences
        .always_on_top;
    WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::default())
        .title("TDisplay · 桌面控件")
        .inner_size(f64::from(width), f64::from(height))
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .always_on_top(pinned)
        .build()
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn widget_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == WINDOW_LABEL {
        Ok(())
    } else {
        Err("请从桌面控件调整置顶。".into())
    }
}

#[tauri::command]
pub fn get_desktop_widget_pinned(window: WebviewWindow) -> Result<bool, String> {
    widget_window(&window)?;
    window.is_always_on_top().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_desktop_widget_pinned(
    window: WebviewWindow,
    settings: State<'_, Mutex<WidgetSettings>>,
    pinned: bool,
) -> Result<bool, String> {
    widget_window(&window)?;
    let mut settings = settings.lock().map_err(|error| error.to_string())?;
    let previous = window
        .is_always_on_top()
        .map_err(|error| error.to_string())?;
    window
        .set_always_on_top(pinned)
        .map_err(|error| error.to_string())?;
    if let Err(error) = settings.save_pinned(pinned) {
        let _ = window.set_always_on_top(previous);
        return Err(format!("无法保存置顶偏好：{error}"));
    }
    window.is_always_on_top().map_err(|error| error.to_string())
}

/// A widget outlives the editor. Recreate the editor when its native window was closed.
#[tauri::command]
pub async fn show_editor(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|error| error.to_string())?;
        window.unminimize().map_err(|error| error.to_string())?;
        return window.set_focus().map_err(|error| error.to_string());
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or("找不到布局编辑器配置。")?;
    WebviewWindowBuilder::from_config(&app, config)
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_widget_is_pinned_by_default() {
        assert!(Preferences::default().always_on_top);
    }

    #[test]
    fn unpinned_preference_survives_reopening_and_invalid_config_recovers() {
        let directory =
            std::env::temp_dir().join(format!("tdisplay-widget-{}", uuid::Uuid::new_v4()));
        let path = directory.join("preferences.json");
        let mut settings = WidgetSettings::load(path.clone());
        assert!(settings.preferences.always_on_top);
        settings.save_pinned(false).unwrap();
        assert!(!WidgetSettings::load(path.clone()).preferences.always_on_top);
        settings.save_pinned(true).unwrap();
        assert!(WidgetSettings::load(path.clone()).preferences.always_on_top);
        fs::write(&path, b"invalid json").unwrap();
        assert!(WidgetSettings::load(path).preferences.always_on_top);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_save_does_not_change_the_last_successful_preference() {
        let directory =
            std::env::temp_dir().join(format!("tdisplay-widget-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let blocker = directory.join("not-a-directory");
        fs::write(&blocker, b"keep").unwrap();
        let mut settings = WidgetSettings::load(blocker.join("preferences.json"));
        assert!(settings.save_pinned(false).is_err());
        assert!(settings.preferences.always_on_top);
        assert_eq!(fs::read(blocker).unwrap(), b"keep");
        fs::remove_dir_all(directory).unwrap();
    }
}
