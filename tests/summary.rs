use std::path::Path;

use heapsnap::analysis::summary::{summarize, SummaryOptions};
use heapsnap::cancel::CancelToken;
use heapsnap::output::summary as summary_output;
use heapsnap::parser::{read_snapshot_file, ReadOptions};

#[test]
fn summary_json_fixture_small() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot = read_snapshot_file(path, options).expect("snapshot");

    let result = summarize(
        &snapshot,
        SummaryOptions {
            top: 10,
            contains: None,
        },
    )
    .expect("summary");

    let json = summary_output::format_json(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert_eq!(value["version"], 1);
    assert_eq!(value["total_nodes"], 3);
    assert_eq!(value["rows"][0]["name"], "Node2");
    assert_eq!(value["rows"][1]["name"], "Node1");
    assert_eq!(value["rows"][2]["name"], "GC roots");
}
