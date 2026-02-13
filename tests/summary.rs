use std::path::Path;

use heapsnap::analysis::summary::{SummaryOptions, summarize};
use heapsnap::cancel::CancelToken;
use heapsnap::output::summary as summary_output;
use heapsnap::parser::{ReadOptions, read_snapshot_file};

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
    assert_eq!(value["rows"][0]["self_size_sum_bytes"].is_number(), true);
    assert_eq!(value["rows"][0]["name"], "Node2");
    assert_eq!(value["rows"][1]["name"], "Node1");
    assert_eq!(value["rows"][2]["name"], "GC roots");
}

#[test]
fn summary_markdown_units_header() {
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

    let markdown = summary_output::format_markdown(&result);
    assert!(markdown.contains("Self Size Sum (bytes)"));
}

#[test]
fn summary_csv_units_header() {
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

    let csv = summary_output::format_csv(&result);
    let header = csv.lines().next().expect("csv header");
    assert_eq!(header, "constructor,count,self_size_sum_bytes");
}

#[test]
fn summary_html_includes_table_and_links() {
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

    let html = summary_output::format_html(&result, path);
    assert!(html.contains("<table>"));
    assert!(html.contains("static report"));
}
