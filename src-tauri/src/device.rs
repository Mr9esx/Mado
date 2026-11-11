use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NumberAnimation {
    None,
    DigitRoll,
    Count,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_duration_ms: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAppearance {
    #[serde(skip_serializing_if = "Option::is_none")]
    accent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    number_animation: Option<NumberAnimation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    state_styles: BTreeMap<String, StateStyle>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Palette {
    background: String,
    surface: String,
    text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DisplayTheme {
    active: String,
    dark: Palette,
    light: Palette,
}

pub fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.as_bytes()[0] == b'#' && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

pub fn valid_state_style(style: &StateStyle) -> bool {
    style.color.as_deref().is_none_or(is_hex_color)
        && style.alert_text.as_deref().is_none_or(|value| value == "auto" || is_hex_color(value))
        && style.alert_duration_ms.is_none_or(|ms| ms <= 10000 && ms % 1000 == 0)
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutItem {
    id: String,
    x: u8,
    y: u8,
    w: u8,
    h: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<CardAppearance>,
}

impl LayoutItem {
    fn appearance_fits(&self, widget: &Widget) -> bool {
        self.appearance.as_ref().is_none_or(|appearance| {
            let numeric = matches!(widget.kind.as_str(), "metric" | "progress" | "trend" | "range")
                && widget.format.as_deref() != Some("countdown");
            appearance.accent_color.as_deref().is_none_or(is_hex_color)
                && (appearance.number_animation.is_none() || numeric)
                && (appearance.state_styles.is_empty() || widget.supports_notify)
                && appearance.state_styles.iter().all(|(state, style)| {
                    widget.state_labels.contains_key(state) && valid_state_style(style)
                })
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DisplayPage {
    id: String,
    name: String,
    layout: Vec<LayoutItem>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPages {
    pages: Vec<DisplayPage>,
    active_page_id: String,
    rotation_seconds: u32,
    last_synced_at: Option<u64>,
    #[serde(default)]
    theme: Option<DisplayTheme>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Widget {
    pub id: String,
    pub group: String,
    #[serde(default)]
    pub group_name: String,
    pub name: String,
    pub short_name: String,
    pub unit: String,
    pub sizes: Vec<[u8; 2]>,
    pub kind: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub state_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(default)]
    pub supports_notify: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_animation: Option<NumberAnimation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub state_styles: BTreeMap<String, StateStyle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_fields: Vec<DetailField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DetailField {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Display {
    width: u32,
    height: u32,
    columns: usize,
    rows: usize,
    max_pages: usize,
    initial_layout: Vec<LayoutItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum PowerStatus {
    External,
    Battery { percent: u8 },
}

pub type WidgetValue = serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pages: Vec<DisplayPage>,
    active_page_id: String,
    rotation_seconds: u32,
    revision: u64,
    sequence: u64,
    layout: Vec<LayoutItem>,
    widgets: Vec<Widget>,
    applied_widgets: Vec<Widget>,
    values: BTreeMap<String, WidgetValue>,
    last_synced_at: Option<u64>,
    last_readings_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<DisplayTheme>,
    power: PowerStatus,
}

#[derive(Clone)]
pub struct Device {
    storage_path: Option<PathBuf>,
    display: Display,
    snapshot: Snapshot,
    source_values: BTreeMap<String, BTreeMap<String, WidgetValue>>,
}

impl Device {
    pub fn new(power: PowerStatus) -> Self {
        let display: Display = serde_json::from_str(include_str!("../../shared/display.json"))
            .expect("the bundled display definition must be valid JSON");
        let widgets = crate::collector::builtin::widgets();
        let applied_widgets = display
            .initial_layout
            .iter()
            .filter_map(|item| widgets.iter().find(|widget| widget.id == item.id).cloned())
            .collect();
        let snapshot = Snapshot {
            pages: vec![DisplayPage {
                id: "main".into(),
                name: "Codex 软件".into(),
                layout: display.initial_layout.clone(),
            }],
            active_page_id: "main".into(),
            rotation_seconds: 0,
            revision: 0,
            sequence: 0,
            layout: display.initial_layout.clone(),
            widgets,
            applied_widgets,
            values: BTreeMap::new(),
            last_synced_at: None,
            last_readings_at: None,
            theme: None,
            power,
        };

        Self {
            storage_path: None,
            display,
            snapshot,
            source_values: BTreeMap::new(),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    pub fn display_size(&self) -> (u32, u32) {
        (self.display.width, self.display.height)
    }

    pub fn uses_widget(&self, id: &str) -> bool {
        self.snapshot
            .pages
            .iter()
            .flat_map(|page| &page.layout)
            .any(|item| item.id == id)
    }

    fn resolve_widgets(&self, layout: &[LayoutItem]) -> Vec<Widget> {
        layout
            .iter()
            .filter_map(|item| {
                self.snapshot
                    .widgets
                    .iter()
                    .find(|widget| widget.id == item.id)
                    .cloned()
            })
            .collect()
    }

    pub fn apply_layout(
        &mut self,
        layout: Vec<LayoutItem>,
        base_revision: u64,
        synced_at: u64,
    ) -> Result<Snapshot, String> {
        let mut pages = self.snapshot.pages.clone();
        pages
            .iter_mut()
            .find(|page| page.id == self.snapshot.active_page_id)
            .unwrap()
            .layout = layout;
        self.apply_pages(
            pages,
            self.snapshot.rotation_seconds,
            base_revision,
            synced_at,
        )
    }

    fn validate_pages(&self, pages: &[DisplayPage], rotation_seconds: u32) -> Result<(), String> {
        if pages.is_empty() || pages.len() > self.display.max_pages {
            return Err(format!("请保留 1–{} 页。", self.display.max_pages));
        }
        if ![0, 5, 10, 15, 30, 60].contains(&rotation_seconds) {
            return Err("轮播间隔不受支持。".into());
        }
        let mut ids = HashSet::new();
        for page in pages {
            if page.id.trim().is_empty() || !ids.insert(&page.id) {
                return Err("页面 ID 为空或重复。".into());
            }
            if page.name.trim().is_empty() || page.name.chars().count() > 20 {
                return Err("页面名称须为 1–20 个字符。".into());
            }
            self.validate_layout(&page.layout)?;
        }
        Ok(())
    }

    fn page_widgets(&self, pages: &[DisplayPage]) -> Vec<Widget> {
        let mut seen = HashSet::new();
        pages
            .iter()
            .flat_map(|page| self.resolve_widgets(&page.layout))
            .filter(|widget| seen.insert(widget.id.clone()))
            .collect()
    }

    fn persist(&self, snapshot: &Snapshot) -> Result<(), String> {
        let Some(path) = &self.storage_path else {
            return Ok(());
        };
        let saved = SavedPages {
            pages: snapshot.pages.clone(),
            active_page_id: snapshot.active_page_id.clone(),
            rotation_seconds: snapshot.rotation_seconds,
            last_synced_at: snapshot.last_synced_at,
            theme: snapshot.theme.clone(),
        };
        let write = || -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temp = path.with_extension("tmp");
            std::fs::write(&temp, serde_json::to_vec_pretty(&saved)?)?;
            std::fs::rename(temp, path)?;
            Ok(())
        };
        write().map_err(|error| format!("保存页面失败：{error}"))
    }

    pub fn attach_storage(&mut self, path: PathBuf) -> Result<(), String> {
        self.storage_path = Some(path.clone());
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        let saved: SavedPages = serde_json::from_slice(&data).map_err(|error| error.to_string())?;
        self.validate_pages(&saved.pages, saved.rotation_seconds)?;
        Self::validate_theme(saved.theme.as_ref())?;
        let active = saved
            .pages
            .iter()
            .find(|page| page.id == saved.active_page_id)
            .unwrap_or(&saved.pages[0]);
        self.snapshot.active_page_id = active.id.clone();
        self.snapshot.layout = active.layout.clone();
        self.snapshot.applied_widgets = self.page_widgets(&saved.pages);
        self.snapshot.pages = saved.pages;
        self.snapshot.rotation_seconds = saved.rotation_seconds;
        self.snapshot.last_synced_at = saved.last_synced_at;
        self.snapshot.theme = saved.theme;
        self.snapshot.sequence += 1;
        Ok(())
    }

    pub fn apply_pages(
        &mut self,
        pages: Vec<DisplayPage>,
        rotation_seconds: u32,
        base_revision: u64,
        synced_at: u64,
    ) -> Result<Snapshot, String> {
        self.apply_display(pages, rotation_seconds, self.snapshot.theme.clone(), base_revision, synced_at)
    }

    pub fn apply_display(
        &mut self,
        mut pages: Vec<DisplayPage>,
        rotation_seconds: u32,
        theme: Option<DisplayTheme>,
        base_revision: u64,
        synced_at: u64,
    ) -> Result<Snapshot, String> {
        for page in &mut pages {
            page.name = page.name.trim().into();
        }
        self.validate_pages(&pages, rotation_seconds)?;
        Self::validate_theme(theme.as_ref())?;
        let applied_widgets = self.page_widgets(&pages);
        if self.snapshot.last_synced_at.is_some()
            && pages == self.snapshot.pages
            && rotation_seconds == self.snapshot.rotation_seconds
            && applied_widgets == self.snapshot.applied_widgets
            && theme == self.snapshot.theme
        {
            return Ok(self.snapshot());
        }
        if base_revision != self.snapshot.revision {
            return Err("页面版本已更新，请放弃草稿后重新编辑。".into());
        }
        let mut next = self.snapshot();
        let active = pages
            .iter()
            .find(|page| page.id == next.active_page_id)
            .unwrap_or(&pages[0]);
        next.active_page_id = active.id.clone();
        next.layout = active.layout.clone();
        next.pages = pages;
        next.rotation_seconds = rotation_seconds;
        next.theme = theme;
        next.applied_widgets = applied_widgets;
        next.revision += 1;
        next.sequence += 1;
        next.last_synced_at = Some(synced_at);
        self.persist(&next)?;
        self.snapshot = next;
        Ok(self.snapshot())
    }

    fn validate_theme(theme: Option<&DisplayTheme>) -> Result<(), String> {
        if let Some(theme) = theme {
            if !matches!(theme.active.as_str(), "dark" | "light")
                || [&theme.dark, &theme.light].iter().any(|palette| {
                    ![&palette.background, &palette.surface, &palette.text].iter().all(|color| is_hex_color(color))
                })
            {
                return Err("屏幕主题或颜色无效。".into());
            }
        }
        Ok(())
    }

    pub fn select_page(&mut self, id: &str) -> Result<Snapshot, String> {
        let page = self
            .snapshot
            .pages
            .iter()
            .find(|page| page.id == id)
            .ok_or("页面不存在。")?;
        if id == self.snapshot.active_page_id {
            return Ok(self.snapshot());
        }
        let mut next = self.snapshot();
        next.active_page_id = page.id.clone();
        next.layout = page.layout.clone();
        next.sequence += 1;
        self.persist(&next)?;
        self.snapshot = next;
        Ok(self.snapshot())
    }

    fn validate_layout(&self, layout: &[LayoutItem]) -> Result<(), String> {
        let mut occupied = vec![false; self.display.columns * self.display.rows];
        let mut ids = HashSet::new();

        for item in layout {
            let widget = self
                .snapshot
                .widgets
                .iter()
                .find(|widget| widget.id == item.id)
                .ok_or("组件类型不受支持。")?;
            if !ids.insert(&item.id) {
                return Err("同一组件不能重复放置。".into());
            }
            if !widget.sizes.contains(&[item.w, item.h]) {
                return Err("组件尺寸不受支持。".into());
            }
            if !item.appearance_fits(widget) {
                return Err("组件外观无效。".into());
            }
            let x = usize::from(item.x);
            let y = usize::from(item.y);
            let right = x + usize::from(item.w);
            let bottom = y + usize::from(item.h);
            if right > self.display.columns || bottom > self.display.rows {
                return Err("组件超出网格边界。".into());
            }
            for row in y..bottom {
                for column in x..right {
                    let cell = &mut occupied[row * self.display.columns + column];
                    if *cell {
                        return Err("组件位置发生重叠。".into());
                    }
                    *cell = true;
                }
            }
        }

        Ok(())
    }

    pub fn install_widgets(&mut self, source: &str, widgets: Vec<Widget>) -> Result<(), String> {
        let mut next = self.snapshot.widgets.clone();
        next.retain(|widget| widget.group != source);
        if widgets
            .iter()
            .any(|widget| next.iter().any(|other| other.id == widget.id))
        {
            return Err("组件 ID 与其他数据源冲突。".into());
        }
        next.extend(widgets);
        for item in self.snapshot.pages.iter().flat_map(|page| &page.layout) {
            if !next
                .iter()
                .any(|widget| widget.id == item.id
                    && widget.sizes.contains(&[item.w, item.h])
                    && item.appearance_fits(widget))
            {
                return Err("当前布局仍在使用此组件、规格或外观，请先调整布局并同步。".into());
            }
        }
        if self.snapshot.widgets != next {
            self.snapshot.widgets = next;
            self.snapshot.sequence += 1;
        }
        Ok(())
    }

    pub fn update_source_values(
        &mut self,
        source: &str,
        mut values: BTreeMap<String, WidgetValue>,
    ) -> Option<Snapshot> {
        let notifications = values
            .iter_mut()
            .filter_map(|(key, reading)| {
                let notify = reading.as_object_mut()?.remove("notify")?;
                (notify == true).then(|| key.clone())
            })
            .collect::<Vec<_>>();
        if self
            .source_values
            .get(source)
            .map_or(values.is_empty(), |current| *current == values)
            && notifications.is_empty()
        {
            return None;
        }
        if values.is_empty() {
            self.source_values.remove(source);
        } else {
            self.source_values.insert(source.into(), values);
        }
        self.snapshot.values = self
            .source_values
            .values()
            .flatten()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        self.snapshot.sequence += 1;
        self.snapshot.last_readings_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs());
        let mut frame = self.snapshot();
        for key in notifications {
            if let Some(reading) = frame.values.get_mut(&key) {
                reading["notify"] = true.into();
            }
        }
        Some(frame)
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
