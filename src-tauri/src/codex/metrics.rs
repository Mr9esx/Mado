use crate::device::{TaskReading, TaskState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use walkdir::WalkDir;

const RECENT_LOGS: usize = 16;
const LOG_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const RECENT_SAMPLES: usize = 10;
const LOOKBACK: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const LOG_HEADER_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountLimits {
    pub windows: Vec<QuotaWindow>,
    pub reset_count: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QuotaWindow {
    pub duration_minutes: Option<u64>,
    pub remaining_percent: u64,
    pub resets_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Speed {
    pub tokens_per_second: u64,
    pub samples: usize,
    pub latest_at: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    tokens: u64,
    duration_ms: i64,
    ended_ms: i64,
}

#[derive(Default)]
pub(super) struct RolloutCache {
    files: HashMap<PathBuf, CachedRollout>,
}

struct CachedRollout {
    modified: SystemTime,
    size: u64,
    thread_id: Option<String>,
    readings: RolloutReadings,
}

#[derive(Default)]
pub(super) struct LocalReadings {
    pub speed: Option<Speed>,
    pub task: Option<TaskReading>,
}

#[derive(Default)]
struct RolloutReadings {
    samples: Vec<Sample>,
    turn: Option<ObservedTurn>,
}

struct ObservedTurn {
    id: Option<String>,
    state: TaskState,
    started_at: Option<SystemTime>,
}

impl RolloutCache {
    pub fn read(&mut self, codex_home: &Path, now: SystemTime) -> LocalReadings {
        let Some(cutoff) = now.checked_sub(LOOKBACK) else {
            return LocalReadings::default();
        };
        let mut recent: Vec<_> = WalkDir::new(codex_home.join("sessions"))
            .min_depth(4)
            .max_depth(4)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?;
                if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                    return None;
                }
                let metadata = entry.metadata().ok()?;
                let modified = metadata.modified().ok()?;
                (modified >= cutoff).then(|| (modified, metadata.len(), entry.into_path()))
            })
            .collect();
        recent.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        recent.truncate(RECENT_LOGS);
        let paths: HashSet<_> = recent.iter().map(|(_, _, path)| path).collect();
        self.files.retain(|path, _| paths.contains(path));
        for (modified, size, path) in recent {
            if self
                .files
                .get(&path)
                .is_some_and(|file| file.modified == modified && file.size == size)
            {
                continue;
            }
            if let Ok(rollout) = read_rollout(&path, modified, size) {
                self.files.insert(path, rollout);
            } else {
                self.files.remove(&path);
            }
        }

        let cutoff_ms = cutoff
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let samples: Vec<_> = self
            .files
            .values()
            .flat_map(|file| &file.readings.samples)
            .filter(|sample| sample.ended_ms >= cutoff_ms)
            .collect();
        let micros = |time: SystemTime| {
            time.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64
        };
        let turns: Vec<_> = self
            .files
            .values()
            .filter_map(|file| {
                let id = file.thread_id.as_deref()?;
                let turn = file.readings.turn.as_ref()?;
                Some(
                    json!({ "id": id, "state": turn.state, "modified": micros(file.modified),
                "started": turn.started_at.map(micros) }),
                )
            })
            .collect();
        let Ok(summary) =
            summarize(json!({ "samples": samples, "turns": turns, "nowMicros": micros(now) }))
        else {
            return LocalReadings::default();
        };
        LocalReadings {
            speed: summary.speed,
            task: summary.task.map(|task| TaskReading {
                state: task.state,
                title: read_thread_title(codex_home, &task.id),
            }),
        }
    }
}

fn read_rollout(path: &Path, modified: SystemTime, size: u64) -> io::Result<CachedRollout> {
    let mut file = BufReader::new(File::open(path)?);
    let mut header = Vec::new();
    BufReader::new(file.by_ref().take(LOG_HEADER_BYTES)).read_until(b'\n', &mut header)?;
    let thread_id = root_thread_id(&header);
    let offset = size.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file.take(LOG_TAIL_BYTES));
    if offset > 0 {
        reader.read_until(b'\n', &mut Vec::new())?;
    }
    let mut readings = parse_rollout(reader);
    // Each file contributes at most the ten newest distinct candidates to the merge.
    readings.samples.sort_unstable_by_key(|sample| {
        std::cmp::Reverse((sample.ended_ms, sample.duration_ms, sample.tokens))
    });
    readings
        .samples
        .dedup_by_key(|sample| (sample.ended_ms, sample.duration_ms, sample.tokens));
    readings.samples.truncate(RECENT_SAMPLES);
    Ok(CachedRollout {
        modified,
        size,
        thread_id,
        readings,
    })
}

fn root_thread_id(header: &[u8]) -> Option<String> {
    let event: Value = serde_json::from_slice(header).ok()?;
    let meta = event.get("payload")?;
    if event.get("type")?.as_str()? != "session_meta"
        || meta
            .get("parent_thread_id")
            .is_some_and(|parent| !parent.is_null())
        || meta.pointer("/source/subagent").is_some()
        || meta.pointer("/source/subAgent").is_some()
    {
        return None;
    }
    meta.get("id")?.as_str().map(str::to_owned)
}

fn read_thread_title(codex_home: &Path, id: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct IndexEntry {
        id: String,
        thread_name: String,
    }

    let file = File::open(codex_home.join("session_index.jsonl")).ok()?;
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<IndexEntry>(&line).ok())
        .filter(|entry| entry.id == id)
        .last()
        .map(|entry| entry.thread_name)
        .filter(|name| !name.trim().is_empty())
}

#[derive(Default)]
struct ResponseWindow {
    start: Option<i64>,
    generated: Option<i64>,
    next_input: Option<i64>,
    ambiguous: bool,
}

impl ResponseWindow {
    fn input(&mut self, time: i64) {
        if self.generated.is_some() {
            self.next_input = Some(time);
        } else {
            self.start = Some(time);
        }
    }

    fn output(&mut self, time: i64) {
        self.ambiguous |= self.next_input.is_some();
        self.generated = Some(time);
    }

    fn finish(&mut self, tokens: u64) -> Option<Sample> {
        let previous = std::mem::replace(
            self,
            Self {
                start: self.next_input,
                ..Self::default()
            },
        );
        let ended_ms = previous.generated?;
        let duration_ms = ended_ms - previous.start?;
        (!previous.ambiguous && tokens >= 20 && (100..=300_000).contains(&duration_ms)).then_some(
            Sample {
                tokens,
                duration_ms,
                ended_ms,
            },
        )
    }
}

fn observe_turn(turn: &mut Option<ObservedTurn>, kind: &str, payload: &Value, event: &Value) {
    let state = match kind {
        "task_started" => TaskState::Running,
        "task_complete" => TaskState::Completed,
        "turn_aborted" => TaskState::Interrupted,
        "error"
            if turn
                .as_ref()
                .is_some_and(|current| current.state == TaskState::Running)
                && payload.get("will_retry").and_then(Value::as_bool) != Some(true) =>
        {
            TaskState::Error
        }
        _ => return,
    };
    let id = payload.get("turn_id").and_then(Value::as_str);
    let previous_id = turn.as_ref().and_then(|current| current.id.as_deref());
    if state != TaskState::Running
        && previous_id
            .zip(id)
            .is_some_and(|(previous, incoming)| previous != incoming)
    {
        return;
    }
    let id = if state == TaskState::Running {
        id
    } else {
        id.or(previous_id)
    };
    let started_at = if state == TaskState::Running {
        payload
            .get("started_at")
            .and_then(Value::as_u64)
            .or_else(|| {
                let time =
                    OffsetDateTime::parse(event.get("timestamp")?.as_str()?, &Rfc3339).ok()?;
                u64::try_from(time.unix_timestamp()).ok()
            })
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
    } else {
        None
    };
    *turn = Some(ObservedTurn {
        id: id.map(str::to_owned),
        state,
        started_at,
    });
}

fn parse_rollout(reader: impl BufRead) -> RolloutReadings {
    let mut window = ResponseWindow::default();
    let mut previous_total = None;
    let mut readings = RolloutReadings::default();
    for line in reader.split(b'\n').map_while(Result::ok) {
        let Ok(event) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let Some(payload) = event.get("payload") else {
            continue;
        };
        let Some(kind) = payload.get("type").and_then(Value::as_str) else {
            continue;
        };
        let event_type = event.get("type").and_then(Value::as_str);
        if event_type == Some("event_msg") {
            observe_turn(&mut readings.turn, kind, payload, &event);
        }
        match (event_type, kind) {
            (Some("event_msg"), "task_complete" | "turn_aborted" | "error") => {
                window = ResponseWindow::default();
            }
            (Some("event_msg"), "token_count") => {
                let Some(total) = payload
                    .pointer("/info/total_token_usage/output_tokens")
                    .and_then(Value::as_u64)
                else {
                    continue;
                };
                if previous_total == Some(total) {
                    continue;
                }
                previous_total = Some(total);
                let tokens = payload
                    .pointer("/info/last_token_usage/output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                if let Some(sample) = window.finish(tokens) {
                    readings.samples.push(sample)
                }
            }
            (Some("event_msg"), "task_started" | "user_message")
            | (
                Some("response_item"),
                "message"
                | "reasoning"
                | "function_call"
                | "custom_tool_call"
                | "function_call_output"
                | "custom_tool_call_output"
                | "mcp_call_output",
            ) => {
                let Some(time) = event
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
                    .map(|date| (date.unix_timestamp_nanos() / 1_000_000) as i64)
                else {
                    continue;
                };
                match kind {
                    "task_started" => {
                        window = ResponseWindow {
                            start: Some(time),
                            ..ResponseWindow::default()
                        }
                    }
                    "user_message"
                    | "function_call_output"
                    | "custom_tool_call_output"
                    | "mcp_call_output" => window.input(time),
                    "message" => match payload.get("role").and_then(Value::as_str) {
                        Some("user") => window.input(time),
                        Some("assistant") => window.output(time),
                        _ => {}
                    },
                    _ => window.output(time),
                }
            }
            _ => {}
        }
    }
    readings
}

#[derive(Deserialize)]
struct SelectedTurn {
    id: String,
    state: TaskState,
}

#[derive(Deserialize)]
struct RolloutSummary {
    speed: Option<Speed>,
    task: Option<SelectedTurn>,
}

fn summarize(data: Value) -> Result<RolloutSummary, String> {
    crate::collector::builtin::codex_export("summarizeRollouts", data, |_| {
        Err("会话汇总只使用提供的采样。".into())
    })
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
