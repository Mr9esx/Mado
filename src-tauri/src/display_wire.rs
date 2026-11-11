use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct DisplayFrame {
    pub config: Value,
    pub readings: Map<String, Value>,
    pub source_ids: BTreeMap<String, String>,
}

fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn copy_fields(target: &mut Map<String, Value>, source: &Value, keys: &[&str]) {
    for &key in keys {
        if let Some(value) = source.get(key).filter(|v| !v.is_null()) {
            target.insert(key.into(), value.clone());
        }
    }
}

fn cap_render_text(render: &mut Map<String, Value>) {
    for (key, max) in [("unit", 48), ("expiredText", 64)] {
        if let Some(text) = render.get(key).and_then(Value::as_str) {
            render.insert(key.into(), json!(clip(text, max)));
        }
    }
    if let Some(labels) = render.get_mut("stateLabels").and_then(Value::as_object_mut) {
        for label in labels.values_mut() {
            if let Some(text) = label.as_str() {
                *label = json!(clip(text, 128));
            }
        }
    }
    if let Some(fields) = render.get_mut("detailFields").and_then(Value::as_array_mut) {
        for field in fields {
            if let Some(text) = field["label"].as_str() {
                field["label"] = json!(clip(text, 64));
            }
        }
    }
}

fn fit_readings(readings: &mut Map<String, Value>) {
    // 留出消息信封空间，保留所有卡片和值，只缩短可选文字。
    for max in [512, 256, 128, 64, 32] {
        if serde_json::to_vec(readings).is_ok_and(|encoded| encoded.len() <= 64_000) {
            break;
        }
        for reading in readings.values_mut() {
            if let Some(text) = reading["detail"].as_str() {
                reading["detail"] = json!(clip(text, max));
            }
            if let Some(details) = reading.get_mut("details").and_then(Value::as_object_mut) {
                for value in details.values_mut() {
                    if let Some(text) = value.as_str() {
                        *value = json!(clip(text, max));
                    }
                }
            }
        }
    }
}

fn reading(raw: &Value, render: &Value) -> Value {
    if raw.is_null() {
        return Value::Null;
    }
    let kind = render["kind"].as_str().unwrap_or_default();
    let mut result = json!({"kind": kind});
    match kind {
        "state" => {
            let Some(state) = raw["state"]
                .as_str()
                .filter(|s| render["stateLabels"].get(*s).is_some())
            else {
                return Value::Null;
            };
            result["state"] = json!(state);
            result["detail"] = json!(raw["detail"]
                .as_str()
                .or_else(|| raw["title"].as_str())
                .map(|s| clip(s, 1024)));
        }
        "chart" => {
            let Some(samples) = raw["values"].as_array() else {
                return Value::Null;
            };
            result["values"] = json!(samples);
            result["windowEndUnixMs"] = raw["windowEndUnixMs"].clone();
        }
        _ if render["format"] == "countdown" => {
            let Some(deadline) = raw["deadlineUnixMs"]
                .as_u64()
                .or_else(|| raw.as_u64().and_then(|s| s.checked_mul(1000)))
            else {
                return Value::Null;
            };
            result["deadlineUnixMs"] = json!(deadline);
        }
        _ => {
            let Some(value) = raw.as_f64().or_else(|| raw["value"].as_f64()) else {
                return Value::Null;
            };
            result["value"] = json!(value);
            if kind == "progress" {
                result["ratio"] = if raw.is_number() {
                    json!((value / 100.0).clamp(0.0, 1.0))
                } else {
                    raw["ratio"].clone()
                };
            } else if kind == "trend" {
                result["delta"] = raw["delta"].clone();
            } else if kind == "range"
                && render["range"]["mode"] == "reading"
                && raw["bounds"].is_object()
            {
                result["bounds"] = raw["bounds"].clone();
            }
        }
    }
    if let Some(fields) = render["detailFields"].as_array() {
        let details: Map<String, Value> = fields
            .iter()
            .filter_map(|field| {
                let key = field["key"].as_str()?;
                raw["details"][key]
                    .as_str()
                    .map(|text| (key.to_owned(), json!(clip(text, 512))))
            })
            .collect();
        if !details.is_empty() {
            result["details"] = Value::Object(details);
        }
    }
    result
}

pub(crate) fn project(snapshot: &Value) -> Result<DisplayFrame, String> {
    let source_pages = snapshot["pages"]
        .as_array()
        .filter(|p| !p.is_empty())
        .ok_or("没有已应用页面。")?;
    let widgets = snapshot["appliedWidgets"]
        .as_array()
        .ok_or("没有已应用组件目录。")?;
    let mut pages = Vec::new();
    let mut readings = Map::new();
    let mut source_ids = BTreeMap::new();
    for page in source_pages {
        let page_id = page["id"].as_str().ok_or("页面缺少 ID。")?;
        let mut cards = Vec::new();
        for item in page["layout"].as_array().ok_or("页面缺少布局。")? {
            let id = item["id"].as_str().ok_or("布局缺少组件 ID。")?;
            let widget = widgets
                .iter()
                .find(|w| w["id"] == id)
                .ok_or("组件目录与已应用布局不匹配。")?;
            let card_id = format!("{}:{page_id}{id}", page_id.len());
            let kind = widget["kind"].as_str().ok_or("缺少组件类型。")?;
            let label = if item["w"] == 1 {
                &widget["shortName"]
            } else {
                &widget["name"]
            };
            let mut render = json!({
                "kind": match kind { "task" => "state", "countdown" => "metric", value => value },
                "label": clip(label.as_str().filter(|s| !s.is_empty()).unwrap_or(id), 128)
            });
            let r = render.as_object_mut().unwrap();
            copy_fields(
                r,
                widget,
                &[
                    "unit",
                    "format",
                    "accentColor",
                    "numberAnimation",
                    "stateLabels",
                    "supportsNotify",
                    "detailFields",
                    "expiredText",
                    "chart",
                    "trend",
                    "range",
                ],
            );
            cap_render_text(r);
            if kind == "countdown" {
                r.insert("format".into(), json!("countdown"));
            }
            if kind == "task" && !r.contains_key("stateLabels") {
                r.insert(
                    "stateLabels".into(),
                    json!({"running":"运行中","completed":"已完成",
                    "interrupted":"已中断","error":"运行异常","unknown":"状态未知"}),
                );
            }
            copy_fields(r, &item["appearance"], &["accentColor", "numberAnimation"]);
            let mut styles = widget["stateStyles"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            if let Some(overrides) = item["appearance"]["stateStyles"].as_object() {
                for (state, style) in overrides {
                    let mut combined = styles
                        .get(state)
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(fields) = style.as_object() {
                        combined.extend(fields.clone());
                    }
                    styles.insert(state.clone(), Value::Object(combined));
                }
            }
            if !styles.is_empty() {
                r.insert("stateStyles".into(), Value::Object(styles));
            }
            let read = reading(&snapshot["values"][id], &render);
            cards.push(json!({
                "cardId":card_id, "x":item["x"], "y":item["y"], "w":item["w"], "h":item["h"], "render":render
            }));
            source_ids.insert(card_id.clone(), id.into());
            readings.insert(card_id, read);
        }
        pages.push(json!({"pageId":page_id, "name":page["name"], "cards":cards}));
    }
    let theme = snapshot["theme"]
        .as_object()
        .map(|t| Value::Object(t.clone()))
        .unwrap_or_else(|| {
            json!({
                "active":"dark",
                "dark":{"background":"#1E1E1E","surface":"#4A4A4A","text":"#F1F1F1"},
                "light":{"background":"#E3E3E3","surface":"#FAFAFA","text":"#242424"}
            })
        });
    let config = json!({
        "schemaVersion":1, "profileId":"tdisplay-c5-v1", "theme":theme,
        "playback":{"defaultPageId":pages[0]["pageId"],"rotationSeconds":snapshot["rotationSeconds"].as_u64().unwrap_or(0)},
        "pages":pages
    });
    fit_readings(&mut readings);
    Ok(DisplayFrame {
        config,
        readings,
        source_ids,
    })
}

pub(crate) fn add_alerts(frame: &mut DisplayFrame, pending: &BTreeSet<(String, String)>) {
    for (card, source) in &frame.source_ids {
        if let Some(value) = frame.readings.get_mut(card) {
            if let Some(state) = value["state"].as_str() {
                if pending.contains(&(source.clone(), state.to_owned())) {
                    value["notify"] = json!(true);
                }
            }
        }
    }
}

// 本地适配保留编辑器快照，同时把已经解析的同一显示契约交给 React。
pub(crate) fn local_snapshot(snapshot: &crate::device::Snapshot) -> Value {
    let mut value = serde_json::to_value(snapshot).expect("可序列化的显示快照");
    if let Ok(mut frame) = project(&value) {
        let alerts = value["values"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(id, reading)| {
                if reading["notify"] == true {
                    reading["state"]
                        .as_str()
                        .map(|state| (id.clone(), state.to_owned()))
                } else {
                    None
                }
            })
            .collect();
        add_alerts(&mut frame, &alerts);
        value["displayFrame"] = json!({
            "config":frame.config, "readings":frame.readings, "sourceIds":frame.source_ids,
            "configRevision":value["revision"], "sequence":value["sequence"]
        });
    }
    value
}

#[cfg(test)]
#[path = "display_wire_tests.rs"]
mod tests;
