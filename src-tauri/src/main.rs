#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod codex;
mod collector;
mod desktop_widget;
mod device;

use device::{Device, DisplayPage, LayoutItem, PowerStatus, Snapshot};
use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager, State};

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
fn apply_pages(
    app: AppHandle,
    device: State<'_, Mutex<Device>>,
    pages: Vec<DisplayPage>,
    rotation_seconds: u32,
    base_revision: u64,
) -> Result<Snapshot, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let snapshot = device
        .lock()
        .map_err(|error| error.to_string())?
        .apply_pages(pages, rotation_seconds, base_revision, now)?;
    let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn select_page(
    app: AppHandle,
    device: State<'_, Mutex<Device>>,
    id: String,
) -> Result<Snapshot, String> {
    let snapshot = device
        .lock()
        .map_err(|error| error.to_string())?
        .select_page(&id)?;
    let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
    Ok(snapshot)
}

fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == collector::runner::RUNNER_FLAG)
    {
        if let Err(error) = collector::runner::run_child() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let power = std::env::var("TDISPLAY_SIM_BATTERY_PERCENT")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|&percent| percent <= 100)
        .map_or(PowerStatus::External, |percent| PowerStatus::Battery {
            percent,
        });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(Device::new(power)))
        .setup(|app| {
            appearance::start(app.handle())?;
            desktop_widget::start(app.handle())?;
            collector::workspace::start(app.handle())?;
            let path = app.path().app_config_dir()?.join("display-pages.json");
            if let Err(error) = app
                .state::<Mutex<Device>>()
                .lock()
                .unwrap()
                .attach_storage(path)
            {
                eprintln!("无法恢复页面：{error}");
            }
            app.manage(codex::start(app.handle().clone())?);
            collector::builtin::start_system(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            appearance::get_appearance,
            appearance::update_appearance,
            get_snapshot,
            sync_layout,
            apply_pages,
            select_page,
            desktop_widget::open_desktop_widget,
            desktop_widget::get_desktop_widget_pinned,
            desktop_widget::set_desktop_widget_pinned,
            desktop_widget::show_editor,
            collector::workspace::open_collectors,
            collector::workspace::focus_layout,
            collector::workspace::collector_catalog,
            collector::workspace::collector_draft,
            collector::workspace::create_collector,
            collector::workspace::save_collector,
            collector::workspace::test_collector,
            collector::workspace::choose_json_input,
            collector::workspace::publish_collector,
            collector::workspace::collector_grants,
            collector::workspace::transfer::inspect_collector_import,
            collector::workspace::transfer::import_collector,
            collector::workspace::transfer::export_collector,
            codex::get_codex_status,
            codex::connect_codex,
            codex::refresh_codex,
            codex::pause_codex,
            codex::login_codex,
            codex::cancel_codex_login
        ])
        .build(tauri::generate_context!())
        .expect("TDisplay Studio failed to start")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => collector::process::terminate_active(),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = desktop_widget::show_editor(app).await {
                        eprintln!("无法打开布局编辑器：{error}");
                    }
                });
            }
            _ => {}
        });
}
