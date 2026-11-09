use super::*;

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
        .update_values(BTreeMap::from([("speed".into(), WidgetValue::Number(57))]))
        .unwrap();
    assert_eq!(refreshed.revision, 1);
    assert_eq!(refreshed.layout, moved);
    assert_eq!(refreshed.last_synced_at, Some(100));
    assert!(refreshed.sequence > applied.sequence);
    assert_eq!(refreshed.values["speed"], WidgetValue::Number(57));
    assert!(applied.values.is_empty());
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
