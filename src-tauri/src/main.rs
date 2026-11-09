#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod codex;
mod device;

use device::{Device, LayoutItem, PowerStatus, Snapshot};
use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

const SNAPSHOT_EVENT: &str = "device-snapshot";

#[tauri::command]
fn get_snapshot(device: State<'_, Mutex<Device>>) -> Result<Snapshot, String> {
    device
        .lock()
        .map(|device| device.snapshot())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn sync_layout(
    app: AppHandle,
    device: State<'_, Mutex<Device>>,
    layout: Vec<LayoutItem>,
    base_revision: u64,
) -> Result<Snapshot, String> {
    let synced_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let snapshot = device
        .lock()
        .map_err(|error| error.to_string())?
        .apply_layout(layout, base_revision, synced_at)?;
    let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
async fn open_display(app: AppHandle, device: State<'_, Mutex<Device>>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("display") {
        window.set_focus().map_err(|error| error.to_string())?;
    } else {
        let (width, height) = device
            .lock()
            .map_err(|error| error.to_string())?
            .display_size();
        WebviewWindowBuilder::new(&app, "display", WebviewUrl::default())
            .title("TDisplay · 虚拟屏")
            .inner_size(f64::from(width), f64::from(height))
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .maximizable(false)
            .build()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn main() {
    let power = std::env::var("TDISPLAY_SIM_BATTERY_PERCENT")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|&percent| percent <= 100)
        .map_or(PowerStatus::External, |percent| PowerStatus::Battery {
            percent,
        });
    tauri::Builder::default()
        .manage(Mutex::new(Device::new(power)))
        .setup(|app| {
            app.manage(codex::start(app.handle().clone())?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            sync_layout,
            open_display,
            codex::get_codex_status,
            codex::connect_codex,
            codex::refresh_codex,
            codex::pause_codex,
            codex::login_codex,
            codex::cancel_codex_login
        ])
        .run(tauri::generate_context!())
        .expect("TDisplay Studio failed to start");
}
