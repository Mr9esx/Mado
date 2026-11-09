use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct LayoutItem {
    id: String,
    x: u8,
    y: u8,
    w: u8,
    h: u8,
}

#[derive(Deserialize)]
struct Widget {
    id: String,
    sizes: Vec<[u8; 2]>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Display {
    width: u32,
    height: u32,
    columns: usize,
    rows: usize,
    widgets: Vec<Widget>,
    initial_layout: Vec<LayoutItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum PowerStatus {
    External,
    Battery { percent: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Running,
    Completed,
    Interrupted,
    Error,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaskReading {
    pub state: TaskState,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum WidgetValue {
    Number(u64),
    Task(TaskReading),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    revision: u64,
    sequence: u64,
    layout: Vec<LayoutItem>,
    values: BTreeMap<String, WidgetValue>,
    last_synced_at: Option<u64>,
    power: PowerStatus,
}

pub struct Device {
    display: Display,
    snapshot: Snapshot,
}

impl Device {
    pub fn new(power: PowerStatus) -> Self {
        let display: Display = serde_json::from_str(include_str!("../../shared/display.json"))
            .expect("the bundled display definition must be valid JSON");
        let snapshot = Snapshot {
            revision: 0,
            sequence: 0,
            layout: display.initial_layout.clone(),
            values: BTreeMap::new(),
            last_synced_at: None,
            power,
        };

        Self { display, snapshot }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    pub fn display_size(&self) -> (u32, u32) {
        (self.display.width, self.display.height)
    }

    pub fn apply_layout(
        &mut self,
        layout: Vec<LayoutItem>,
        base_revision: u64,
        synced_at: u64,
    ) -> Result<Snapshot, String> {
        if layout == self.snapshot.layout {
            return Ok(self.snapshot());
        }
        if base_revision != self.snapshot.revision {
            return Err("设备布局版本已更新。".into());
        }

        self.validate_layout(&layout)?;
        self.snapshot.layout = layout;
        self.snapshot.revision += 1;
        self.snapshot.sequence += 1;
        self.snapshot.last_synced_at = Some(synced_at);
        Ok(self.snapshot())
    }

    fn validate_layout(&self, layout: &[LayoutItem]) -> Result<(), String> {
        let mut occupied = vec![false; self.display.columns * self.display.rows];
        let mut ids = HashSet::new();

        for item in layout {
            let widget = self
                .display
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

    pub fn update_values(&mut self, values: BTreeMap<String, WidgetValue>) -> Option<Snapshot> {
        if self.snapshot.values == values {
            return None;
        }
        self.snapshot.values = values;
        self.snapshot.sequence += 1;
        Some(self.snapshot())
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
