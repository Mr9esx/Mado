use super::*;
use serde_json::json;

#[test]
fn uses_the_main_bucket_and_keeps_zero_reset_cards() {
    let response = json!({
        "rateLimits": { "primary": { "usedPercent": 10, "windowDurationMins": 300 } },
        "rateLimitsByLimitId": {
            "codex": {
                "primary": { "usedPercent": 24, "windowDurationMins": 10080, "resetsAt": 1900000000 },
                "secondary": { "usedPercent": 50, "windowDurationMins": 300 }
            },
            "codex_other": { "primary": { "usedPercent": 99, "windowDurationMins": 60 } }
        },
        "rateLimitResetCredits": { "availableCount": 0 }
    });

    let account = parse_limits(&response);
    assert_eq!(
        account
            .windows
            .iter()
            .map(|w| w.remaining_percent)
            .collect::<Vec<_>>(),
        [50, 76]
    );
    assert_eq!(account.windows[1].resets_at, Some(1900000000));
    assert_eq!(account.reset_count, Some(0));
    assert_eq!(account.limiting_window().unwrap().remaining_percent, 50);
    assert_eq!(account.limiting_window().unwrap().resets_at, None);
}

#[test]
fn countdown_uses_the_window_with_the_lowest_remaining_quota() {
    let account = parse_limits(&json!({ "rateLimits": {
        "primary": { "usedPercent": 30, "windowDurationMins": 300, "resetsAt": 100 },
        "secondary": { "usedPercent": 80, "windowDurationMins": 10080, "resetsAt": 200 }
    } }));
    assert_eq!(account.limiting_window().unwrap().resets_at, Some(200));
}

#[test]
fn tracks_the_latest_turn_and_ignores_a_different_turn_ending() {
    let started = r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"new"}}
{"type":"event_msg","payload":{"type":"task_complete","turn_id":"old"}}
"#;
    assert_eq!(
        parse_rollout(started.as_bytes()).turn.unwrap().state,
        TaskState::Running
    );
    let interrupted = format!(
        "{started}{}",
        r#"{"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"new"}}"#
    );
    assert_eq!(
        parse_rollout(interrupted.as_bytes()).turn.unwrap().state,
        TaskState::Interrupted
    );
}

#[test]
fn task_reading_keeps_the_full_title_without_switching_on_log_writes() {
    let fixture = std::env::temp_dir().join(format!("tdisplay-task-test-{}", std::process::id()));
    let logs = fixture.join("sessions/2026/11/09");
    std::fs::create_dir_all(&logs).unwrap();
    let rollout = logs.join("rollout-task-test.jsonl");
    std::fs::write(
        &rollout,
        concat!(
            r#"{"type":"session_meta","payload":{"id":"root"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1","started_at":200}}"#,
            "\n",
        ),
    )
    .unwrap();
    let title = "用完整的中文对话标题测试单行滚动，不省略后半段";
    std::fs::write(
        fixture.join("session_index.jsonl"),
        json!({ "id": "root", "thread_name": title }).to_string(),
    )
    .unwrap();
    let modified = rollout.metadata().unwrap().modified().unwrap();
    let older_turn = logs.join("rollout-older-turn.jsonl");
    std::fs::write(
        &older_turn,
        concat!(
            r#"{"type":"session_meta","payload":{"id":"older-root"}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"older","started_at":100}}"#,
            "\n",
        ),
    )
    .unwrap();
    let newer_write = modified + Duration::from_secs(5);
    File::options()
        .write(true)
        .open(older_turn)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(newer_write))
        .unwrap();
    let mut cache = RolloutCache::default();
    let active = cache.read(&fixture, newer_write).task.unwrap();
    assert_eq!(active.state, TaskState::Running);
    assert_eq!(active.title.as_deref(), Some(title));
    let stale = cache
        .read(
            &fixture,
            newer_write + ACTIVE_WINDOW + Duration::from_secs(1),
        )
        .task
        .unwrap();
    assert_eq!(stale.state, TaskState::Unknown);
    std::fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn absent_quota_and_reset_count_are_unknown() {
    let account = parse_limits(&json!({ "rateLimitResetCredits": null }));
    assert!(account.limiting_window().is_none());
    assert_eq!(account.reset_count, None);
}

#[test]
fn response_speed_excludes_tool_runtime_and_deduplicates_usage() {
    let log = r#"
{"timestamp":"2026-11-09T01:00:00Z","type":"event_msg","payload":{"type":"task_started"}}
{"timestamp":"2026-11-09T01:00:02Z","type":"response_item","payload":{"type":"custom_tool_call"}}
{"timestamp":"2026-11-09T01:00:12Z","type":"response_item","payload":{"type":"custom_tool_call_output"}}
{"timestamp":"2026-11-09T01:00:12.001Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"output_tokens":100},"last_token_usage":{"output_tokens":100}}}}
{"timestamp":"2026-11-09T01:00:15Z","type":"response_item","payload":{"type":"message","role":"assistant"}}
{"timestamp":"2026-11-09T01:00:15.001Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"output_tokens":160},"last_token_usage":{"output_tokens":60}}}}
{"timestamp":"2026-11-09T01:00:16Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"output_tokens":160},"last_token_usage":{"output_tokens":60}}}}
"#;
    let mut samples = parse_rollout(log.as_bytes()).samples;
    let speed = average_speed(&mut samples).unwrap();
    assert_eq!(speed.samples, 2);
    assert_eq!(speed.tokens_per_second, 32);
}

#[test]
fn copied_samples_do_not_count_twice_in_a_forked_rollout() {
    let sample = Sample {
        tokens: 50,
        duration_ms: 1000,
        ended_ms: 2000,
    };
    let mut samples = vec![sample.clone(), sample];
    let speed = average_speed(&mut samples).unwrap();
    assert_eq!(speed.samples, 1);
}
