// Presentation adapter for the existing Codex settings pane. The collector
// workspace owns execution, scheduling and the lifetime of interactive actions.
use crate::collector::workspace;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs};
use tauri::{AppHandle, Manager};

const ID: &str = "codex";
const PROGRAM: &str = "appServer";

pub fn restore_settings(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let directory = app.path().app_config_dir()?;
    let catalog = fs::read(directory.join("collectors.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if catalog
        .as_ref()
        .is_some_and(|value| value["instances"].get(ID).is_some())
    {
        return Ok(());
    }
    let Some(previous) = fs::read(directory.join("codex-source.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return Ok(());
    };
    let overrides = previous["cliPath"]
        .as_str()
        .map_or_else(BTreeMap::new, |path| {
            BTreeMap::from([(PROGRAM.into(), path.into())])
        });
    workspace::configure_bundled(
        app,
        ID,
        previous["enabled"].as_bool().unwrap_or(false),
        Some(overrides),
    )?;
    Ok(())
}

#[tauri::command]
pub fn get_codex_status(app: AppHandle) -> Result<Value, String> {
    let status = workspace::instance_status(&app, ID)?;
    let enabled = status["enabled"] == true;
    let data = &status["data"];
    let settings = &status["settings"];
    let phase = if !enabled {
        "disabled"
    } else if status["action"] == "login" {
        "signingIn"
    } else if settings[PROGRAM].is_null() {
        "needsCli"
    } else if status["error"].is_string() {
        data["phase"].as_str().unwrap_or("error")
    } else {
        data["phase"].as_str().unwrap_or("checking")
    };
    Ok(json!({
        "sequence": status["sequence"],
        "enabled": enabled,
        "cliPath": settings[format!("explicit.{PROGRAM}")],
        "resolvedCli": settings[PROGRAM],
        "phase": phase,
        "message": status["error"].as_str().or_else(|| data["message"].as_str()),
        "plan": data["plan"],
        "limits": data["limits"].as_object().map_or_else(
            || json!({ "windows": [], "resetCount": null }),
            |limits| json!(limits),
        ),
        "accountUpdatedAt": data["accountUpdatedAt"],
        "speed": data["speed"],
    }))
}

#[tauri::command]
pub fn connect_codex(app: AppHandle, cli_path: Option<String>) -> Result<(), String> {
    let overrides = cli_path.map_or_else(BTreeMap::new, |path| {
        BTreeMap::from([(PROGRAM.into(), path)])
    });
    workspace::configure_bundled(&app, ID, true, Some(overrides))
}

#[tauri::command]
pub fn refresh_codex(app: AppHandle) -> Result<(), String> {
    workspace::refresh_instance(&app, ID)
}

#[tauri::command]
pub fn pause_codex(app: AppHandle) -> Result<(), String> {
    workspace::configure_bundled(&app, ID, false, None)
}

#[tauri::command]
pub fn login_codex(app: AppHandle) -> Result<(), String> {
    workspace::run_action(&app, ID, "login")
}

#[tauri::command]
pub fn cancel_codex_login(app: AppHandle) -> Result<(), String> {
    workspace::cancel_action(&app, ID)
}
