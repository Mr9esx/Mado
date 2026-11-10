use crate::device::{Widget, WidgetValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type Fields = BTreeMap<String, Value>;
// Leave room for the small entry wrapper when upgrading a full legacy project.
pub const MAX_PROJECT: usize = 513 * 1024;
pub const MAX_FILES: usize = 65;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub manifest: Manifest,
    pub files: BTreeMap<String, String>,
    pub fixture: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub module_id: String,
    pub sdk_api_version: u32,
    pub entry: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub definition_in_script: bool,
    #[serde(flatten)]
    pub definition: Definition,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Definition {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Setting>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub permissions: BTreeMap<String, Permission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<CollectionSchedule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<String, String>,
    pub outputs: BTreeMap<String, Output>,
    pub widgets: BTreeMap<String, WidgetSpec>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectionSchedule {
    pub refresh_seconds: u64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Input {
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Setting {
    pub label: String,
    #[serde(default)]
    pub default: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Permission {
    Http {
        label: String,
        #[serde(default = "get_only")]
        methods: Vec<String>,
    },
    File {
        label: String,
    },
    Directory {
        label: String,
    },
    Process {
        label: String,
        args: Vec<Vec<String>>,
    },
    Stdio {
        label: String,
        args: Vec<String>,
        methods: Vec<String>,
    },
    System {
        label: String,
    },
}

fn get_only() -> Vec<String> {
    vec!["GET".into()]
}

impl Permission {
    pub fn label(&self) -> &str {
        match self {
            Self::Http { label, .. }
            | Self::File { label }
            | Self::Directory { label }
            | Self::Process { label, .. }
            | Self::Stdio { label, .. }
            | Self::System { label } => label,
        }
    }

    fn valid(&self) -> bool {
        let args_valid = |args: &[String]| {
            args.len() <= 24
                && args
                    .iter()
                    .all(|arg| arg.len() <= 1024 && !arg.contains('\0'))
        };
        label_valid(self.label())
            && match self {
                Self::Http { methods, .. } => {
                    !methods.is_empty()
                        && methods.len() <= 6
                        && methods.iter().all(|method| {
                            matches!(
                                method.as_str(),
                                "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
                            )
                        })
                }
                Self::Process { args, .. } => {
                    !args.is_empty() && args.len() <= 12 && args.iter().all(|set| args_valid(set))
                }
                Self::Stdio { args, methods, .. } => {
                    args_valid(args)
                        && !methods.is_empty()
                        && methods.len() <= 24
                        && methods.iter().all(|method| {
                            !method.is_empty()
                                && method.len() <= 100
                                && method.split('/').all(key_valid)
                        })
                }
                _ => true,
            }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub value_type: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSpec {
    pub output_key: String,
    pub kind: String,
    pub label: String,
    pub supported_spans: Vec<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub state_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub supports_notify: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<Chart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<Trend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Span {
    pub w: u8,
    pub h: u8,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub series: String,
    pub range_seconds: u64,
    pub sample_count: usize,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trend {
    pub comparison_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_unit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Range {
    pub mode: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collected {
    pub outputs: Fields,
    #[serde(default)]
    pub notifications: Vec<String>,
}

pub fn key_valid(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn path_valid(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && path.split('/').all(|part| {
            part != "."
                && part != ".."
                && !part.is_empty()
                && !part.chars().any(|ch| {
                    ch.is_control() || matches!(ch, '\\' | ':' | '<' | '>' | '|' | '?' | '*' | '"')
                })
        })
}

fn label_valid(label: &str) -> bool {
    !label.trim().is_empty() && label.chars().count() <= 80
}

impl Project {
    pub fn validate(&self, trusted: bool) -> Result<(), String> {
        let manifest = &self.manifest;
        let definition = &manifest.definition;
        match manifest.sdk_api_version {
            1 if definition.settings.is_empty() && definition.permissions.is_empty() => {}
            2 if definition.inputs.is_empty() => {}
            1 | 2 => return Err("SDK v1 使用 inputs，SDK v2 使用 settings 和 permissions。".into()),
            _ => return Err("此模块的 SDK 版本不受支持。".into()),
        }
        if definition
            .schedule
            .as_ref()
            .is_some_and(|schedule| !(5..=3600).contains(&schedule.refresh_seconds))
        {
            return Err("采集间隔须为 5–3600 秒。".into());
        }
        if definition.resources.iter().any(|(key, target)| {
            target.trim().is_empty()
                || target.len() > 4096
                || target.chars().any(char::is_control)
                || !(definition.inputs.contains_key(key)
                    || definition
                        .permissions
                        .get(key)
                        .is_some_and(|permission| !matches!(permission, Permission::System { .. })))
        }) {
            return Err("resources 只能绑定已声明的文件、程序或网络资源。".into());
        }
        if definition.schedule.is_some()
            && definition.permissions.iter().any(|(alias, permission)| {
                matches!(permission, Permission::Http { .. })
                    && !definition.resources.contains_key(alias)
            })
        {
            return Err("请在 resources 中填写 HTTP 资源的地址。".into());
        }
        if definition.settings.len() > 12
            || definition.permissions.len() > 12
            || definition.settings.iter().any(|(key, setting)| {
                !key_valid(key)
                    || !label_valid(&setting.label)
                    || setting.default.len() > 4096
                    || definition.permissions.contains_key(key)
            })
            || definition
                .permissions
                .iter()
                .any(|(key, permission)| !key_valid(key) || !permission.valid())
        {
            return Err("普通参数或资源授权的定义无效。".into());
        }
        if manifest.module_id.len() > 128 || !manifest.module_id.split('.').all(key_valid) {
            return Err("模块 ID 无效。".into());
        }
        if !trusted && matches!(manifest.module_id.as_str(), "codex" | "system") {
            return Err("此 ID 由内置模块保留。".into());
        }
        if self.files.len() > MAX_FILES
            || self.files.values().map(String::len).sum::<usize>() > MAX_PROJECT
            || self.files.keys().any(|path| !path_valid(path))
            || self
                .files
                .keys()
                .map(|path| path.to_lowercase())
                .collect::<BTreeSet<_>>()
                .len()
                != self.files.len()
        {
            return Err("项目文件路径无效、重名或超过容量限制。".into());
        }
        if !self.files.contains_key(&manifest.entry)
            || !(manifest.entry.ends_with(".mjs") || manifest.entry.ends_with(".js"))
        {
            return Err("请选择一个项目内的 JavaScript 入口。".into());
        }
        if !label_valid(&definition.name)
            || definition.outputs.is_empty()
            || definition.outputs.len() > 24
            || definition.widgets.len() > 24
            || definition.inputs.len() > 12
        {
            return Err("请填写模块名称，并声明至少一路输出。".into());
        }
        for (key, input) in &definition.inputs {
            let allowed = match input.kind.as_str() {
                "httpJson" | "fileJson" => input.provider.is_none(),
                "native" => trusted && input.provider.is_some(),
                _ => false,
            };
            if !allowed || !key_valid(key) || !label_valid(&input.label) {
                return Err(format!("输入 {key} 无效或未获授权。"));
            }
        }
        for (key, output) in &definition.outputs {
            let state = output.value_type == "state";
            if !key_valid(key)
                || !matches!(
                    output.value_type.as_str(),
                    "number" | "deadline" | "progress" | "state"
                )
                || state == output.state_keys.is_empty()
                || output
                    .state_keys
                    .iter()
                    .chain(&output.detail_keys)
                    .any(|key| !key_valid(key))
                || output.unit.chars().count() > 24
            {
                return Err(format!("输出 {key} 的类型或字段无效。"));
            }
        }
        for (key, widget) in &definition.widgets {
            let output = definition
                .outputs
                .get(&widget.output_key)
                .ok_or_else(|| format!("组件 {key} 引用了不存在的输出。"))?;
            let valid = match widget.kind.as_str() {
                "metric" => match widget.format.as_deref() {
                    None | Some("number") => output.value_type == "number",
                    Some("countdown") => output.value_type == "deadline",
                    _ => false,
                },
                "progress" => output.value_type == "progress",
                "state" => {
                    output.value_type == "state"
                        && output.state_keys.iter().all(|key| {
                            widget
                                .state_labels
                                .get(key)
                                .is_some_and(|label| label_valid(label))
                        })
                }
                "chart" => {
                    output.value_type == "number"
                        && widget.chart.as_ref().is_some_and(|chart| {
                            chart.series == "bar"
                                && (1..=3600).contains(&chart.range_seconds)
                                && (1..=12).contains(&chart.sample_count)
                                && chart.range_seconds * 1000 % chart.sample_count as u64 == 0
                                && chart.min < chart.max
                        })
                }
                "trend" => {
                    output.value_type == "number"
                        && widget
                            .trend
                            .as_ref()
                            .is_some_and(|trend| (1..=3600).contains(&trend.comparison_seconds))
                }
                "range" => {
                    output.value_type == "number"
                        && widget
                            .range
                            .as_ref()
                            .is_some_and(|range| match range.mode.as_str() {
                                "fixed" => {
                                    range.min.zip(range.max).is_some_and(|(min, max)| min < max)
                                }
                                "reading" => range.min.is_none() && range.max.is_none(),
                                _ => false,
                            })
                }
                _ => false,
            };
            if !valid
                || widget.chart.is_some() != (widget.kind == "chart")
                || widget.trend.is_some() != (widget.kind == "trend")
                || widget.range.is_some() != (widget.kind == "range")
                || (widget.supports_notify && widget.kind != "state")
                || !key_valid(key)
                || !label_valid(&widget.label)
                || !widget
                    .supported_spans
                    .iter()
                    .any(|span| span.w == 1 && span.h == 1)
                || widget
                    .supported_spans
                    .iter()
                    .any(|span| !matches!((span.w, span.h), (1, 1) | (2, 1) | (2, 2) | (4, 1)))
            {
                return Err(format!("组件 {key} 的类型、输出或规格不匹配。"));
            }
        }
        Ok(())
    }

    pub fn validate_outputs(&self, result: &Collected) -> Result<(), String> {
        let outputs = &self.manifest.definition.outputs;
        if !outputs.keys().eq(result.outputs.keys()) {
            return Err("返回值 outputs 必须覆盖模块声明的所有输出键，缺值请用 null。".into());
        }
        for (key, value) in &result.outputs {
            if value.is_null() {
                continue;
            }
            let output = &outputs[key];
            let valid = match output.value_type.as_str() {
                "number" => {
                    value["value"].is_number()
                        && value.get("bounds").is_none_or(|bounds| {
                            bounds["min"]
                                .as_f64()
                                .zip(bounds["max"].as_f64())
                                .is_some_and(|(min, max)| min < max)
                        })
                }
                "deadline" => value["deadlineUnixMs"]
                    .as_u64()
                    .is_some_and(|timestamp| timestamp <= 8_640_000_000_000_000),
                "progress" => {
                    value["value"].is_number()
                        && value.get("ratio").is_some_and(|ratio| {
                            ratio.is_null()
                                || ratio
                                    .as_f64()
                                    .is_some_and(|ratio| (0.0..=1.0).contains(&ratio))
                        })
                }
                "state" => {
                    value["state"]
                        .as_str()
                        .is_some_and(|state| output.state_keys.iter().any(|key| key == state))
                        && value
                            .get("detail")
                            .is_none_or(|detail| detail.is_null() || detail.is_string())
                }
                _ => false,
            };
            let details_valid = value.get("details").is_none_or(|details| {
                details.as_object().is_some_and(|details| {
                    details
                        .iter()
                        .all(|(key, value)| output.detail_keys.contains(key) && value.is_string())
                })
            });
            if !valid || !details_valid {
                return Err(format!(
                    "输出 {key} 的值与 {} 契约不符。",
                    output.value_type
                ));
            }
        }
        if result.notifications.iter().any(|key| {
            outputs
                .get(key)
                .is_none_or(|output| output.value_type != "state" || result.outputs[key].is_null())
        }) {
            return Err("通知只能引用本次非空的状态输出。".into());
        }
        Ok(())
    }

    pub fn widget_id(&self, source: &str, key: &str) -> String {
        if source == "codex" {
            key.into()
        } else {
            format!("{source}.{key}")
        }
    }

    pub fn widgets(&self, source: &str, source_name: &str) -> Vec<Widget> {
        let definition = &self.manifest.definition;
        definition
            .widgets
            .iter()
            .map(|(key, widget)| Widget {
                id: self.widget_id(source, key),
                group: source.into(),
                group_name: source_name.into(),
                name: widget.label.clone(),
                short_name: widget
                    .short_label
                    .clone()
                    .unwrap_or_else(|| widget.label.clone()),
                unit: definition.outputs[&widget.output_key].unit.clone(),
                sizes: widget
                    .supported_spans
                    .iter()
                    .map(|span| [span.w, span.h])
                    .collect(),
                kind: widget.kind.clone(),
                state_labels: widget.state_labels.clone(),
                accent_color: widget.accent_color.clone(),
            })
            .collect()
    }
}

#[derive(Default)]
pub struct History(BTreeMap<String, VecDeque<(u64, f64)>>);

impl History {
    pub fn project(
        &mut self,
        project: &Project,
        source: &str,
        at: u64,
        outputs: &Fields,
        notifications: &[String],
    ) -> BTreeMap<String, WidgetValue> {
        let windows = &project.manifest.definition.widgets;
        for (key, value) in outputs {
            if !windows.values().any(|widget| {
                widget.output_key == *key && matches!(widget.kind.as_str(), "chart" | "trend")
            }) {
                continue;
            }
            let samples = self.0.entry(key.clone()).or_default();
            while samples
                .front()
                .is_some_and(|(time, _)| *time < at.saturating_sub(3_660_000))
            {
                samples.pop_front();
            }
            if let Some(value) = value["value"].as_f64() {
                samples.push_back((at, value));
            }
        }
        windows.iter().filter_map(|(key, widget)| {
            let reading = outputs.get(&widget.output_key)?.as_object()?;
            let mut payload = Value::Object(reading.clone());
            if let Some(chart) = &widget.chart {
                let start = at.saturating_sub(chart.range_seconds * 1000);
                let step = chart.range_seconds * 1000 / chart.sample_count as u64;
                let mut slots = vec![None; chart.sample_count];
                for &(time, sample) in self.0.get(&widget.output_key).into_iter().flatten() {
                    if time > start && time <= at {
                        let index = ((time - start - 1) / step) as usize;
                        if let Some(slot) = slots.get_mut(index) {
                            *slot = Some(sample);
                        }
                    }
                }
                payload = json!({ "windowEndUnixMs": at, "values": slots, "min": chart.min, "max": chart.max });
            } else if let Some(trend) = &widget.trend {
                let target = at.saturating_sub(trend.comparison_seconds * 1000);
                let previous = self.0.get(&widget.output_key).into_iter().flatten().rev()
                    .find(|(time, _)| *time <= target && target - *time <= 60_000);
                payload["delta"] = json!(previous.and_then(|(_, previous)| payload["value"].as_f64().map(|value| value - previous)));
                payload["deltaUnit"] = json!(trend.delta_unit);
            } else if let Some(range) = &widget.range {
                if range.mode == "fixed" {
                    payload["bounds"] = json!({ "min": range.min, "max": range.max });
                }
            }
            payload["kind"] = json!(widget.kind);
            if widget.supports_notify && notifications.contains(&widget.output_key) {
                payload["notify"] = true.into();
            }
            Some((project.widget_id(source, key), payload))
        }).collect()
    }
}
