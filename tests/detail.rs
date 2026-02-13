use std::path::Path;

use heapsnap::analysis::detail::{DetailOptions, DetailResult, detail};
use heapsnap::cancel::CancelToken;
use heapsnap::output::detail as detail_output;
use heapsnap::parser::{ReadOptions, read_snapshot_file};

#[test]
fn detail_name_json_fixture_small() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot = read_snapshot_file(path, options).expect("snapshot");

    let result = detail(
        &snapshot,
        DetailOptions {
            id: None,
            name: Some("Node1".to_string()),
            skip: 0,
            limit: 10,
            top_retainers: 5,
            top_edges: 5,
        },
    )
    .expect("detail");

    let json = detail_output::format_json(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert_eq!(value["version"], 1);
    assert_eq!(value["mode"], "name");
    assert_eq!(value["name"], "Node1");
    assert!(value["constructor_summary"]["total_count"].is_number());

    let html = detail_output::format_html(&result, path);
    assert!(html.contains("static report"));
}

#[test]
fn detail_id_json_fixture_small() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot = read_snapshot_file(path, options).expect("snapshot");

    let result = detail(
        &snapshot,
        DetailOptions {
            id: Some(2),
            name: None,
            skip: 0,
            limit: 10,
            top_retainers: 5,
            top_edges: 5,
        },
    )
    .expect("detail");

    let json = detail_output::format_json(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert_eq!(value["version"], 1);
    assert_eq!(value["mode"], "id");
    assert_eq!(value["id"], 2);
    assert_eq!(value["name"], "Node1");
    assert!(value["retainers"].is_array());
    assert!(value["outgoing_edges"].is_array());
    assert!(value["shallow_size_distribution"].is_array());
    assert!(matches!(result, DetailResult::ById(_)));
}
