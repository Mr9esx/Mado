#![cfg(unix)]

use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("tdisplay-collector-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn codex_mjs_runs_from_its_directory_and_collects_all_sources() {
    let fixture = Fixture::new();
    let sessions = fixture.0.join("sessions/2026/11/10");
    fs::create_dir_all(&sessions).unwrap();
    let title = "保持完整的中文标题，设备渲染时按规则滚动";
    fs::write(
        fixture.0.join("session_index.jsonl"),
        json!({ "id": "root", "thread_name": title }).to_string(),
    )
    .unwrap();
    let now = time::OffsetDateTime::now_utc() - time::Duration::seconds(5);
    let event = |kind, payload, offset| {
        let timestamp = (now + time::Duration::seconds(offset))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        json!({ "type": kind, "payload": payload, "timestamp": timestamp }).to_string()
    };
    fs::write(
        sessions.join("rollout-current.jsonl"),
        [
            json!({ "type": "session_meta", "payload": { "id": "root" } }).to_string(),
            event(
                "event_msg",
                json!({ "type": "task_started", "turn_id": "current" }),
                0,
            ),
            event(
                "response_item",
                json!({ "type": "message", "role": "assistant" }),
                2,
            ),
            event(
                "event_msg",
                json!({
                    "type": "token_count",
                    "info": {
                        "total_token_usage": { "output_tokens": 80 },
                        "last_token_usage": { "output_tokens": 80 },
                    }
                }),
                3,
            ),
        ]
        .join("\n"),
    )
    .unwrap();

    let program = fixture.0.join("app-server");
    fs::write(&program, r#"#!/bin/sh
[ "$*" = 'app-server --listen stdio://' ] || exit 9
IFS= read -r line || exit 8
case "$line" in *'"method":"initialize"'*) ;; *) exit 7;; esac
case "$line" in *'"jsonrpc"'*) exit 6;; esac
printf '%s\n' '{"id":1,"result":{}}'
IFS= read -r line || exit 5
case "$line" in *'"method":"initialized"'*) ;; *) exit 4;; esac
IFS= read -r line || exit 3
case "$line" in *'"method":"account/read"'*) ;; *) exit 2;; esac
printf '%s\n' '{"id":2,"result":{"account":{"type":"chatgpt","planType":"plus"}}}'
IFS= read -r line || exit 1
case "$line" in *'"method":"account/rateLimits/read"'*) ;; *) exit 10;; esac
printf '%s\n' '{"id":3,"result":{"rateLimits":{"primary":{"usedPercent":10}},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":100,"resetsAt":1900000000}}},"rateLimitResetCredits":{"availableCount":0}}}'
"#).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();

    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../shared/collectors/codex");
    let execution = Command::new(env!("CARGO_BIN_EXE_mado-studio"))
        .arg("--collect")
        .arg(directory)
        .args(["--grant", &format!("appServer={}", program.display())])
        .args(["--grant", &format!("logs={}", fixture.0.display())])
        .output()
        .unwrap();
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    let collected: Value = serde_json::from_slice(&execution.stdout).unwrap();
    assert_eq!(collected["status"]["phase"], "connected");
    assert_eq!(
        collected["outputs"],
        json!({
            "quota": { "value": 0, "ratio": 0 },
            "quotaReset": { "deadlineUnixMs": 1900000000000_i64 },
            "resets": { "value": 0 },
            "speed": { "value": 40 },
            "task": { "state": "running", "detail": title },
        })
    );
}

#[test]
fn generic_action_receives_notification_emitted_before_its_rpc_reply() {
    let fixture = Fixture::new();
    let program = fixture.0.join("server");
    fs::write(
        &program,
        r#"#!/bin/sh
IFS= read -r request || exit 1
case "$request" in *'"method":"begin"'*) ;; *) exit 2;; esac
printf '%s\n' '{"jsonrpc":"2.0","method":"ready","params":{"value":7}}'
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":null}'
"#,
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(fixture.0.join("manifest.json"), json!({
        "moduleId": "fixture",
        "sdkApiVersion": 2,
        "entry": "index.mjs",
        "name": "Fixture",
        "actions": ["wait"],
        "permissions": {
            "server": { "kind": "stdio", "label": "Test process", "args": [], "methods": ["begin", "ready"] }
        },
        "outputs": { "reading": { "valueType": "number" } },
        "widgets": { "reading": {
            "kind": "metric", "label": "Reading", "outputKey": "reading",
            "supportedSpans": [{ "w": 1, "h": 1 }]
        } }
    }).to_string()).unwrap();
    fs::write(
        fixture.0.join("index.mjs"),
        r#"
import { defineModule } from '@tdisplay/sdk'
export default defineModule({
  collect() { return { outputs: { reading: null } } },
  handlers: {
    async wait(_, context) {
      await context.process.request('server', 'begin')
      const notice = await context.process.receive('server', 'ready')
      return { outputs: { reading: { value: notice.value } } }
    },
  },
})
"#,
    )
    .unwrap();
    let execution = Command::new(env!("CARGO_BIN_EXE_mado-studio"))
        .arg("--collect")
        .arg(&fixture.0)
        .args([
            "--grant",
            &format!("server={}", program.display()),
            "--action",
            "wait",
        ])
        .output()
        .unwrap();
    assert!(
        execution.status.success(),
        "{}",
        String::from_utf8_lossy(&execution.stderr)
    );
    let collected: Value = serde_json::from_slice(&execution.stdout).unwrap();
    assert_eq!(collected["outputs"]["reading"], json!({ "value": 7 }));
}
