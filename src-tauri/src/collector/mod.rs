pub mod builtin;
mod io;
mod model;
pub(crate) mod process;
pub mod runner;
pub mod workspace;

use crate::{
    device::{Device, WidgetValue},
    SNAPSHOT_EVENT,
};
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn publish_readings(app: &AppHandle, source: &str, values: BTreeMap<String, WidgetValue>) {
    let state = app.state::<Mutex<Device>>();
    let snapshot = state
        .lock()
        .ok()
        .and_then(|mut device| device.update_source_values(source, values));
    if let Some(snapshot) = snapshot {
        let _ = app.emit(SNAPSHOT_EVENT, snapshot);
    }
}
