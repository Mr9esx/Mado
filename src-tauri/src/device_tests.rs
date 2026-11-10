use super::*;

#[test]
fn initial_layout_can_be_synced_without_moving_a_card() {
    let mut device = Device::new(PowerStatus::External);
    let initial = device.snapshot();
    let applied = device
        .apply_layout(initial.layout.clone(), initial.revision, 100)
        .unwrap();
    assert_eq!(applied.last_synced_at, Some(100));
    assert_eq!(applied.layout, initial.layout);
    assert_eq!(applied.revision, initial.revision + 1);
    assert!(applied.sequence > initial.sequence);
    assert_eq!(
        device
            .apply_layout(initial.layout, initial.revision, 200)
            .unwrap(),
        applied
    );
}

#[test]
fn invalid_layout_leaves_device_revision_and_layout_intact() {
    let mut device = Device::new(PowerStatus::External);
    let original = device.snapshot();
    let mut overlapping = original.layout.clone();
    overlapping[2].x = original.layout[0].x;
    overlapping[2].y = original.layout[0].y;

    assert_eq!(
        device
            .apply_layout(overlapping, original.revision, 100)
            .unwrap_err(),
        "组件位置发生重叠。"
    );

    let mut oversized = original.layout.clone();
    oversized[0].w = 2;
    assert_eq!(
        device
            .apply_layout(oversized, original.revision, 100)
            .unwrap_err(),
        "组件尺寸不受支持。"
    );
    assert_eq!(device.snapshot(), original);
}

#[test]
fn acknowledged_layout_survives_live_value_refresh() {
    let mut device = Device::new(PowerStatus::External);
    let original = device.snapshot();
    assert_eq!(original.last_synced_at, None);
    let mut moved = original.layout.clone();
    moved[2].x = 3;

    let applied = device
        .apply_layout(moved.clone(), original.revision, 100)
        .unwrap();
    assert_eq!(applied.revision, 1);
    assert_eq!(applied.layout, moved);
    assert_eq!(applied.last_synced_at, Some(100));

    let refreshed = device
        .update_source_values(
            "codex",
            BTreeMap::from([(
                "speed".into(),
                serde_json::json!({"kind": "metric", "value": 57}),
            )]),
        )
        .unwrap();
    assert_eq!(refreshed.revision, 1);
    assert_eq!(refreshed.layout, moved);
    assert_eq!(refreshed.last_synced_at, Some(100));
    assert!(refreshed.sequence > applied.sequence);
    assert_eq!(
        refreshed.values["speed"],
        serde_json::json!({"kind": "metric", "value": 57})
    );
    assert!(applied.values.is_empty());
}

#[test]
fn clearing_one_source_keeps_other_source_readings() {
    let mut device = Device::new(PowerStatus::External);
    device.update_source_values(
        "codex",
        BTreeMap::from([(
            "speed".into(),
            serde_json::json!({"kind": "metric", "value": 57}),
        )]),
    );
    device.update_source_values(
        "system",
        BTreeMap::from([(
            "system.cpu".into(),
            serde_json::json!({"kind": "metric", "value": 43}),
        )]),
    );

    let snapshot = device
        .update_source_values("codex", BTreeMap::new())
        .unwrap();
    assert_eq!(
        snapshot.values,
        BTreeMap::from([(
            "system.cpu".into(),
            serde_json::json!({"kind": "metric", "value": 43})
        )])
    );
}

#[test]
fn presentation_changes_wait_for_a_new_layout_acknowledgment() {
    let mut device = Device::new(PowerStatus::External);
    let initial = device.snapshot();
    let mut codex = initial
        .widgets
        .iter()
        .filter(|widget| widget.group == "codex")
        .cloned()
        .collect::<Vec<_>>();
    codex
        .iter_mut()
        .find(|widget| widget.id == "speed")
        .unwrap()
        .name = "新版速度".into();
    device.install_widgets("codex", codex).unwrap();
    assert_eq!(device.snapshot().applied_widgets, initial.applied_widgets);
    let next = device
        .apply_layout(initial.layout, initial.revision, 101)
        .unwrap();
    assert_eq!(next.revision, initial.revision + 1);
    assert_eq!(
        next.applied_widgets
            .iter()
            .find(|widget| widget.id == "speed")
            .unwrap()
            .name,
        "新版速度"
    );
}

#[test]
fn reminders_are_emitted_once_and_do_not_enter_the_snapshot() {
    let mut device = Device::new(PowerStatus::External);
    let values = BTreeMap::from([(
        "task".into(),
        serde_json::json!({ "kind": "state", "state": "completed", "notify": true }),
    )]);
    let event = device
        .update_source_values("codex", values.clone())
        .unwrap();
    assert_eq!(event.values["task"]["notify"], true);
    assert!(device.snapshot().values["task"].get("notify").is_none());
    assert!(
        device
            .update_source_values("codex", values)
            .unwrap()
            .sequence
            > event.sequence
    );
}

#[test]
fn rejects_stale_drafts_and_accepts_retried_layouts() {
    let mut device = Device::new(PowerStatus::External);
    let original = device.snapshot();
    let mut moved = original.layout.clone();
    moved[2].x = 3;
    let applied = device
        .apply_layout(moved.clone(), original.revision, 100)
        .unwrap();

    assert_eq!(
        device.apply_layout(moved, original.revision, 200).unwrap(),
        applied
    );
    assert!(device
        .apply_layout(original.layout, original.revision, 300)
        .is_err());
    assert_eq!(device.snapshot().last_synced_at, Some(100));
}

#[test]
fn power_status_is_serialized_by_source() {
    let external = Device::new(PowerStatus::External).snapshot();
    let battery = Device::new(PowerStatus::Battery { percent: 67 }).snapshot();

    assert_eq!(
        serde_json::to_value(external).unwrap()["power"],
        serde_json::json!({ "source": "external" })
    );
    assert_eq!(
        serde_json::to_value(battery).unwrap()["power"],
        serde_json::json!({ "source": "battery", "percent": 67 })
    );
}

#[test]
fn pages_are_independent_and_inactive_pages_protect_widget_references() {
    let mut device = Device::new(PowerStatus::External);
    let initial = device.snapshot();
    let pages = vec![
        DisplayPage {
            id: "one".into(),
            name: "首页".into(),
            layout: vec![],
        },
        DisplayPage {
            id: "two".into(),
            name: "指标".into(),
            layout: initial.layout.clone(),
        },
    ];
    let applied = device
        .apply_pages(pages.clone(), 0, initial.revision, 100)
        .unwrap();
    assert_eq!(applied.pages, pages);
    assert!(applied.layout.is_empty());
    assert!(device.uses_widget("quota"));
    assert!(device.install_widgets("codex", vec![]).is_err());
    let switched = device.select_page("two").unwrap();
    assert_eq!(switched.layout, initial.layout);
    assert_eq!(switched.revision, applied.revision);
    assert_eq!(switched.last_synced_at, Some(100));
    assert!(switched.sequence > applied.sequence);
    assert!(device.select_page("missing").is_err());
    assert_eq!(device.snapshot(), switched);
}

#[test]
fn invalid_page_sets_never_replace_the_applied_configuration() {
    let mut device = Device::new(PowerStatus::External);
    let initial = device.snapshot();
    let page = initial.pages[0].clone();
    let invalid = vec![
        vec![],
        vec![page.clone(); 2],
        (0..6)
            .map(|i| DisplayPage {
                id: format!("p{i}"),
                ..page.clone()
            })
            .collect(),
        vec![DisplayPage {
            name: "  ".into(),
            ..page.clone()
        }],
        vec![DisplayPage {
            layout: vec![page.layout[0].clone(); 2],
            ..page.clone()
        }],
    ];
    for pages in invalid {
        assert!(device.apply_pages(pages, 0, initial.revision, 100).is_err());
        assert_eq!(device.snapshot(), initial);
    }
    assert!(device
        .apply_pages(initial.pages.clone(), 1, initial.revision, 100)
        .is_err());
    assert_eq!(device.snapshot(), initial);
}

#[test]
fn page_changes_persist_and_deleting_active_page_selects_a_survivor() {
    let directory = std::env::temp_dir().join(format!("tdisplay-pages-{}", uuid::Uuid::new_v4()));
    let path = directory.join("pages.json");
    let mut device = Device::new(PowerStatus::External);
    device.attach_storage(path.clone()).unwrap();
    let initial = device.snapshot();
    let mut second = initial.pages[0].clone();
    second.id = "second".into();
    second.name = "第二页".into();
    let pages = vec![initial.pages[0].clone(), second];
    device
        .apply_pages(pages.clone(), 5, initial.revision, 100)
        .unwrap();
    device.select_page("second").unwrap();
    let mut restored = Device::new(PowerStatus::External);
    restored.attach_storage(path.clone()).unwrap();
    let snapshot = restored.snapshot();
    assert_eq!(snapshot.pages, pages);
    assert_eq!(snapshot.active_page_id, "second");
    assert_eq!(snapshot.rotation_seconds, 5);
    assert_eq!(snapshot.last_synced_at, Some(100));
    let next = restored
        .apply_pages(vec![pages[0].clone()], 0, snapshot.revision, 200)
        .unwrap();
    assert_eq!(next.active_page_id, pages[0].id);
    assert_eq!(next.pages.len(), 1);
    assert!(restored
        .apply_pages(pages, 0, snapshot.revision, 300)
        .is_err());
    assert_eq!(restored.snapshot(), next);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn saving_pages_is_atomic_when_the_storage_path_is_not_writable() {
    let directory = std::env::temp_dir().join(format!("tdisplay-pages-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let blocker = directory.join("file");
    std::fs::write(&blocker, b"keep").unwrap();
    let mut device = Device::new(PowerStatus::External);
    let _ = device.attach_storage(blocker.join("pages.json"));
    let initial = device.snapshot();
    assert!(device
        .apply_pages(initial.pages.clone(), 0, initial.revision, 100)
        .is_err());
    assert_eq!(device.snapshot(), initial);
    assert_eq!(std::fs::read(blocker).unwrap(), b"keep");
    std::fs::remove_dir_all(directory).unwrap();
}
