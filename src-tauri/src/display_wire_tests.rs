use super::project;
use serde_json::json;

#[test]
fn projects_all_pages_and_clears_snapshot_alerts() {
    let snapshot = json!({
        "pages":[
            {"id":"a","name":"甲","layout":[{"id":"task","x":0,"y":0,"w":1,"h":1}]},
            {"id":"b","name":"乙","layout":[{"id":"task","x":0,"y":0,"w":1,"h":1}]}
        ],
        "appliedWidgets":[{"id":"task","kind":"state","name":"任务","shortName":"任务","supportsNotify":true,
            "stateLabels":{"completed":"已完成"}}],
        "values":{"task":{"kind":"state","state":"completed","detail":"完成","notify":true}},
        "rotationSeconds":5
    });
    let frame = project(&snapshot).unwrap();
    let first = frame.config["pages"][0]["cards"][0]["cardId"]
        .as_str()
        .unwrap();
    let second = frame.config["pages"][1]["cards"][0]["cardId"]
        .as_str()
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(frame.readings[first]["detail"], "完成");
    assert_eq!(frame.readings[second]["detail"], "完成");
    assert!(frame.readings[first].get("notify").is_none());
    assert_eq!(frame.config["playback"]["rotationSeconds"], 5);
}

#[test]
fn a_missing_reading_is_null_but_zero_is_preserved() {
    let snapshot = json!({
        "pages":[{"id":"a","name":"甲","layout":[{"id":"cpu","x":0,"y":0,"w":1,"h":1},{"id":"memory","x":1,"y":0,"w":1,"h":1}]}],
        "appliedWidgets":[{"id":"cpu","kind":"metric","name":"CPU","shortName":"CPU"},{"id":"memory","kind":"progress","name":"内存","shortName":"内存"}],
        "values":{"cpu":0}
    });
    let frame = project(&snapshot).unwrap();
    let cards = &frame.config["pages"][0]["cards"];
    assert_eq!(
        frame.readings[cards[0]["cardId"].as_str().unwrap()]["value"].as_f64(),
        Some(0.0)
    );
    assert!(frame.readings[cards[1]["cardId"].as_str().unwrap()].is_null());
}
