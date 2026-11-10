use super::{
    io,
    model::{Fields, History, Project},
    now, publish_readings, runner,
};
use crate::device::Widget;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};
use sysinfo::System;
use tauri::AppHandle;

fn project(manifest: &str, code: &str, fixture: Value) -> Project {
    let manifest =
        serde_json::from_str::<super::model::Manifest>(manifest).expect("valid bundled manifest");
    let project = Project {
        files: BTreeMap::from([(manifest.entry.clone(), code.into())]),
        manifest,
        fixture,
    };
    project.validate(true).expect("valid built-in collector");
    project
}

pub fn codex() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        project(
            include_str!("../../../shared/collectors/codex/manifest.json"),
            include_str!("../../../shared/collectors/codex/index.mjs"),
            json!({ "calls": [
                { "operation": "rpc", "alias": "appServer", "params": {
                    "method": "account/read", "params": { "refreshToken": false }
                }, "result": { "account": { "type": "chatgpt", "planType": "plus" } } },
                { "operation": "rpc", "alias": "appServer", "params": {
                    "method": "account/rateLimits/read"
                }, "result": {
                    "rateLimits": { "primary": { "usedPercent": 19, "resetsAt": 1794282600 },
                        "secondary": { "usedPercent": 5, "resetsAt": 1794325800 } },
                    "rateLimitResetCredits": { "availableCount": 1 }
                } }
            ] }),
        )
    })
}

pub fn system() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        project(
            include_str!("../../../shared/collectors/system/manifest.json"),
            include_str!("../../../shared/collectors/system/index.mjs"),
            json!({ "calls": [{ "operation": "system", "alias": "system", "params": {},
                "result": { "cpuPercent": 68, "memoryPercent": 42, "uptimeSeconds": 14400 }
            }] }),
        )
    })
}

pub fn widgets() -> Vec<Widget> {
    [codex(), system()]
        .into_iter()
        .flat_map(|project| {
            let manifest = &project.manifest;
            project.widgets(&manifest.module_id, &manifest.definition.name)
        })
        .collect()
}

fn collect_with(
    project: &Project,
    inputs: Value,
    sampled_at: u64,
    broker: impl FnMut(runner::Call) -> Result<Value, String> + 'static,
) -> Result<Fields, String> {
    let response = runner::evaluate_with(
        runner::Request {
            entry: project.manifest.entry.clone(),
            files: project.files.clone(),
            inputs,
            sampled_at,
        },
        broker,
    );
    let result = response
        .result
        .ok_or_else(|| response.error.unwrap_or_else(|| "采集无结果。".into()))?;
    project.validate_outputs(&result)?;
    Ok(result.outputs)
}

pub fn collect(project: &Project, inputs: Value, sampled_at: u64) -> Result<Fields, String> {
    collect_with(project, inputs, sampled_at, |_| {
        Err("未授予此宿主能力。".into())
    })
}

pub fn codex_export<T: serde::de::DeserializeOwned>(
    name: &str,
    settings: Value,
    broker: impl FnMut(runner::Call) -> Result<Value, String> + 'static,
) -> Result<T, String> {
    let project = codex();
    runner::evaluate_export(
        runner::Request {
            entry: project.manifest.entry.clone(),
            files: project.files.clone(),
            inputs: settings,
            sampled_at: now(),
        },
        name,
        broker,
    )
}

pub fn publish_codex(app: &AppHandle, input: Value) {
    let at = now();
    match collect(codex(), input, at) {
        Ok(outputs) => publish_readings(
            app,
            "codex",
            History::default().project(codex(), "codex", at, &outputs, &[]),
        ),
        Err(error) => eprintln!("Codex 采集模块失败：{error}"),
    }
}

pub fn system_readings() -> Value {
    struct Sampler {
        metrics: System,
        refreshed: Option<std::time::Instant>,
    }
    static SAMPLER: OnceLock<Mutex<Sampler>> = OnceLock::new();
    let sampler = SAMPLER.get_or_init(|| {
        let mut metrics = System::new();
        metrics.refresh_cpu_usage();
        Mutex::new(Sampler {
            metrics,
            refreshed: None,
        })
    });
    let Ok(mut sampler) = sampler.lock() else {
        return Value::Null;
    };
    if sampler
        .refreshed
        .is_none_or(|at| at.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
    {
        sampler.metrics.refresh_cpu_usage();
        sampler.metrics.refresh_memory();
        sampler.refreshed = Some(std::time::Instant::now());
    }
    let metrics = &sampler.metrics;
    let total = metrics.total_memory();
    let percent =
        (total != 0).then(|| (1000.0 * metrics.used_memory() as f64 / total as f64).round() / 10.0);
    json!({
        "cpuPercent": (metrics.global_cpu_usage() * 10.0).round() / 10.0,
        "memoryPercent": percent, "uptimeSeconds": System::uptime()
    })
}

pub fn start_system(app: AppHandle) {
    thread::spawn(move || {
        let Ok(client) = reqwest::blocking::Client::builder().build() else {
            return;
        };
        let project = system();
        let mut history = History::default();
        let _ = system_readings();
        thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        loop {
            let at = now();
            let mut broker = io::Broker::live(
                &project.manifest.definition,
                BTreeMap::new(),
                client.clone(),
            );
            match collect_with(project, json!({}), at, move |call| broker.call(call)) {
                Ok(outputs) => publish_readings(
                    &app,
                    "system",
                    history.project(project, "system", at, &outputs, &[]),
                ),
                Err(error) => eprintln!("系统状态采集失败：{error}"),
            }
            thread::sleep(Duration::from_secs(5));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{Device, PowerStatus};

    #[test]
    fn codex_script_uses_the_lowest_window_for_both_cards() {
        let project = codex();
        let inputs = json!({ "limits": {
            "windows": [
                { "remainingPercent": 75, "resetsAt": 200 },
                { "remainingPercent": 0, "resetsAt": 100 },
                { "remainingPercent": 0, "resetsAt": 300 }
            ] },
            "speed": 0
        });
        let outputs = collect(project, inputs, 500).unwrap();
        assert_eq!(outputs["quota"], json!({ "value": 0, "ratio": 0 }));
        assert_eq!(outputs["quotaReset"], json!({ "deadlineUnixMs": 100000 }));
        assert!(outputs["task"].is_null());
        let cards = History::default().project(project, "codex", 500, &outputs, &[]);
        assert!(cards["quota"]["ratio"] == 0);
        let disconnected = collect(project, json!({ "limits": { "windows": [] } }), 500).unwrap();
        assert!(disconnected["quota"].is_null() && disconnected["quotaReset"].is_null());
    }

    #[test]
    fn system_history_and_trend_reuse_one_cpu_output() {
        let project = system();
        let mut history = History::default();
        let output = |usage| {
            let fixture = json!({ "calls": [{ "operation": "system", "alias": "system", "params": {},
                "result": { "cpuPercent": usage, "memoryPercent": 42, "uptimeSeconds": 600 }
            }] });
            let (settings, mut broker) =
                io::Broker::fixture(&project.manifest.definition, &fixture).unwrap();
            collect_with(project, settings, 0, move |call| broker.call(call)).unwrap()
        };
        history.project(project, "system", 3_700_000, &output(64), &[]);
        let cards = history.project(project, "system", 4_000_000, &output(68), &[]);
        assert!(cards["system.cpu-trend"]["delta"] == 4.0);
        assert!(cards["system.cpu-usage-history"]["values"][11] == 68.0);
        let mut device = Device::new(PowerStatus::External);
        assert!(device.update_source_values("system", cards).is_some());
    }
}
