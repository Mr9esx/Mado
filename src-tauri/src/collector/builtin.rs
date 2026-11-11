use super::model::{Manifest, Project};
use crate::device::Widget;
use serde_json::{json, Value};
use std::sync::OnceLock;

fn project(manifest: &str, files: &[(&str, &str)], fixture: Value) -> Project {
    let manifest = serde_json::from_str::<Manifest>(manifest).expect("valid bundled manifest");
    let project = Project {
        files: files
            .iter()
            .map(|(path, code)| ((*path).into(), (*code).into()))
            .collect(),
        manifest,
        fixture,
    };
    project.validate(true).expect("valid bundled collector");
    project
}

pub fn projects() -> &'static [Project] {
    static PROJECTS: OnceLock<[Project; 3]> = OnceLock::new();
    PROJECTS.get_or_init(|| {
        [
            project(
                include_str!("../../../shared/collectors/codex/manifest.json"),
                &[
                    (
                        "index.mjs",
                        include_str!("../../../shared/collectors/codex/index.mjs"),
                    ),
                    (
                        "rollouts.mjs",
                        include_str!("../../../shared/collectors/codex/rollouts.mjs"),
                    ),
                ],
                json!({ "calls": [
                { "operation": "rpc", "alias": "appServer", "params": {
                    "method": "initialize", "params": { "clientInfo": {
                        "name": "tdisplay-collector", "version": "1.0.0"
                    } }
                }, "result": {} },
                { "operation": "notify", "alias": "appServer", "params": {
                    "method": "initialized"
                }, "result": null },
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
            ),
            project(
                include_str!("../../../shared/collectors/system/manifest.json"),
                &[(
                    "index.mjs",
                    include_str!("../../../shared/collectors/system/index.mjs"),
                ),
                (
                    "projection.mjs",
                    include_str!("../../../shared/collectors/system/projection.mjs"),
                )],
                json!({ "calls": [{ "operation": "system", "alias": "system", "params": {},
                "result": { "cpuUsagePercent": 68, "memoryUsedBytes": 8_589_934_592_u64, "memoryTotalBytes": 17_179_869_184_u64, "memoryAvailableBytes": 8_589_934_592_u64, "systemDiskUsedPercent": 43.2, "networkReceivedBytes": 1000, "networkTransmittedBytes": 500, "uptimeSeconds": 14400 } }] }),
            ),
            project(
                include_str!("../../../shared/collectors/github/manifest.json"),
                &[
                    (
                        "index.mjs",
                        include_str!("../../../shared/collectors/github/index.mjs"),
                    ),
                    (
                        "client.mjs",
                        include_str!("../../../shared/collectors/github/client.mjs"),
                    ),
                ],
                json!({
                    "settings": { "GITHUB_REPOSITORY": "openai/codex" },
                    "calls": [
                        {
                            "operation": "http", "alias": "githubApi",
                            "params": {
                                "method": "GET",
                                "path": "/search/issues?q=repo%3Aopenai%2Fcodex+is%3Apr+is%3Aopen&per_page=1",
                                "headers": { "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" }
                            },
                            "result": { "status": 200, "text": "{\"total_count\":4}" }
                        },
                        {
                            "operation": "http", "alias": "githubApi",
                            "params": {
                                "method": "GET",
                                "path": "/search/issues?q=repo%3Aopenai%2Fcodex+is%3Aissue+is%3Aopen&per_page=1",
                                "headers": { "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" }
                            },
                            "result": { "status": 200, "text": "{\"total_count\":6}" }
                        },
                        {
                            "operation": "http", "alias": "githubApi",
                            "params": {
                                "method": "GET",
                                "path": "/repos/openai/codex/actions/runs?per_page=1",
                                "headers": { "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" }
                            },
                            "result": { "status": 200, "text": "{\"workflow_runs\":[{\"status\":\"completed\",\"conclusion\":\"success\",\"name\":\"CI\"}]}" }
                        }
                    ]
                }),
            ),
        ]
    })
}

pub fn get(id: &str) -> Option<&'static Project> {
    projects()
        .iter()
        .find(|project| project.manifest.module_id == id)
}

pub fn widgets() -> Vec<Widget> {
    projects()
        .iter()
        .flat_map(|project| {
            let manifest = &project.manifest;
            project.widgets(&manifest.module_id, &manifest.definition.name)
        })
        .collect()
}

#[cfg(test)]
mod binding_tests {
    #[test]
    fn bundled_binding_hints_match_the_manifest() {
        let project = super::get("codex").unwrap();
        assert!(project.manifest.definition.suggested_bindings.contains_key("appServer"));
        assert!(project.manifest.definition.suggested_bindings.contains_key("logs"));
    }

    #[test]
    fn system_module_exposes_additional_short_title_widgets_without_changing_legacy_ids() {
        let project = super::get("system").unwrap();
        assert_eq!(project.manifest.definition.name, "系统");
        let widgets = &project.manifest.definition.widgets;
        for id in [
            "cpu-usage-history",
            "cpu-trend",
            "cpu-range",
            "memory",
            "uptime",
        ] {
            assert!(widgets.contains_key(id), "missing compatible widget id {id}");
        }
        for id in ["system-disk", "network-download", "network-upload"] {
            assert!(widgets.contains_key(id), "missing new system widget {id}");
        }
        assert!(widgets.values().all(|widget| {
            widget
                .label
                .chars()
                .filter(|character| ('\u{4e00}'..='\u{9fff}').contains(character))
                .count()
                <= 4
        }));
        assert!(project.manifest.definition.outputs.contains_key("systemDisk"));
        assert!(project.manifest.definition.outputs.contains_key("downloadRate"));
        assert!(project.manifest.definition.outputs.contains_key("uploadRate"));
    }

    #[test]
    fn bundled_system_collector_resolves_projection_and_collects_outputs() {
        use crate::collector::runner::{self, Request};
        use serde_json::{json, Value};

        let project = super::get("system").unwrap();
        let result = runner::evaluate_with(
            Request {
                entry: project.manifest.entry.clone(),
                files: project.files.clone(),
                inputs: json!({}),
                private_settings: vec![],
                sampled_at: 10_000,
                action: None,
                state: Value::Null,
            },
            |call| {
                assert_eq!(call.operation, "system");
                Ok(json!({
                    "cpuUsagePercent": 25.0,
                    "memoryUsedBytes": 4 * 1024_u64.pow(3),
                    "memoryTotalBytes": 8 * 1024_u64.pow(3),
                    "memoryAvailableBytes": 4 * 1024_u64.pow(3),
                    "systemDiskUsedPercent": 50.0,
                    "networkReceivedBytes": 1000,
                    "networkTransmittedBytes": 2000,
                    "uptimeSeconds": 3600,
                }))
            },
        );

        let collected = result
            .result
            .unwrap_or_else(|| panic!("bundled collector should execute: {:?}", result.error));
        assert_eq!(collected.outputs["cpuUsage"]["value"], 25.0);
        assert_eq!(collected.outputs["systemDisk"]["ratio"], 0.5);
        assert_eq!(collected.outputs["downloadRate"], Value::Null);
        assert_eq!(collected.state["network"]["receivedBytes"], 1000);
        assert!(result.error.is_none());
    }

    #[test]
    fn github_module_declares_config_and_short_widget_titles() {
        let project = super::get("github").unwrap();
        let definition = &project.manifest.definition;
        assert_eq!(project.manifest.module_id, "github");
        assert_eq!(definition.schedule.as_ref().unwrap().refresh_seconds, 60);
        assert!(!definition.schedule.as_ref().unwrap().enabled);
        assert!(definition.settings["GITHUB_REPOSITORY"].required.unwrap());
        assert_eq!(definition.settings["GITHUB_TOKEN"].secret, Some(true));
        assert_eq!(definition.settings["GITHUB_TOKEN"].required, Some(false));
        let labels: Vec<_> = definition.widgets.values().map(|widget| widget.label.as_str()).collect();
        assert_eq!(labels, ["CI状态", "Issue", "PR数"]);
    }

    #[test]
    fn bundled_github_collector_imports_and_projects_rest_results() {
        use crate::collector::runner::{self, Request};
        use serde_json::{json, Value};

        let project = super::get("github").unwrap();
        let result = runner::evaluate_with(
            Request {
                entry: project.manifest.entry.clone(),
                files: project.files.clone(),
                inputs: json!({ "GITHUB_REPOSITORY": "https://github.com/openai/codex", "GITHUB_TOKEN": "" }),
                private_settings: vec!["GITHUB_TOKEN".into()],
                sampled_at: 10_000,
                action: None,
                state: Value::Null,
            },
            |call| {
                assert_eq!(call.operation, "http");
                assert_eq!(call.alias, "githubApi");
                assert_eq!(call.params["method"], "GET");
                assert!(call.params["headers"].get("Authorization").is_none());
                let body = if call.params["path"].as_str().unwrap().contains("is%3Apr") {
                    json!({ "total_count": 4 })
                } else if call.params["path"].as_str().unwrap().contains("is%3Aissue") {
                    json!({ "total_count": 6 })
                } else {
                    json!({ "workflow_runs": [{ "status": "completed", "conclusion": "success", "name": "CI" }] })
                };
                Ok(json!({ "status": 200, "text": body.to_string() }))
            },
        );
        let collected = result.result.unwrap_or_else(|| panic!("bundled collector failed: {:?}", result.error));
        assert_eq!(collected.outputs["pullRequests"], json!({ "value": 4 }));
        assert_eq!(collected.outputs["issues"], json!({ "value": 6 }));
        assert_eq!(collected.outputs["ci"], json!({ "state": "success", "detail": "CI" }));
    }

    #[test]
    fn github_fixture_collects_expected_widget_values() {
        use crate::collector::{io::Broker, runner::{self, Request}};
        use serde_json::Value;

        let project = super::get("github").unwrap();
        let (inputs, mut broker) = Broker::fixture(&project.manifest.definition, &project.fixture).unwrap();
        let result = runner::evaluate_with(
            Request {
                entry: project.manifest.entry.clone(),
                files: project.files.clone(),
                inputs,
                private_settings: vec!["GITHUB_TOKEN".into()],
                sampled_at: 10_000,
                action: None,
                state: Value::Null,
            },
            move |call| broker.call(call),
        );
        let collected = result.result.unwrap_or_else(|| panic!("fixture collector failed: {:?}", result.error));
        assert_eq!(collected.outputs["pullRequests"]["value"], 4);
        assert_eq!(collected.outputs["issues"]["value"], 6);
        assert_eq!(collected.outputs["ci"]["state"], "success");
    }

    #[test]
    fn cpu_chart_advances_one_column_per_collection() {
        use crate::collector::model::{Fields, History};
        use serde_json::{json, Value};

        let project = super::get("system").unwrap();
        let mut history = History::default();
        let mut outputs = Fields::new();
        let chart_id = project.widget_id("system", "cpu-usage-history");
        let start = 4_000_000;
        let mut collect = |at, sample: Value| {
            outputs.insert("cpuUsage".into(), sample);
            history.project(project, "system", at, &outputs, &[])[&chart_id]["values"]
                .as_array().unwrap().clone()
        };
        collect(start, json!({"value": 25}));
        collect(start + 1000, json!({"value": 25}));
        let columns = collect(start + 2000, json!({"value": 70}));
        assert_eq!(columns.len(), 12);
        assert_eq!(columns[9..], [json!(25.0), json!(25.0), json!(70.0)]);

        let columns = collect(start + 3000, Value::Null);
        assert_eq!(columns[8..], [json!(25.0), json!(25.0), json!(70.0), Value::Null]);

        let columns = collect(start + 3_604_000, json!({"value": 30}));
        assert!(columns[..11].iter().all(Value::is_null));
        assert_eq!(columns[11], json!(30.0));
    }
}
