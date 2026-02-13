use std::path::Path;

use heapsnap::analysis::diff::{DiffOptions, diff_summaries};
use heapsnap::cancel::CancelToken;
use heapsnap::output::diff as diff_output;
use heapsnap::parser::{ReadOptions, read_snapshot_file};

#[test]
fn diff_json_units_fixture_small() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot_a = read_snapshot_file(path, options).expect("snapshot a");

    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot_b = read_snapshot_file(path, options).expect("snapshot b");

    let result = diff_summaries(
        &snapshot_a,
        &snapshot_b,
        DiffOptions {
            top: 10,
            contains: None,
        },
    )
    .expect("diff");

    let json = diff_output::format_json(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");

    assert_eq!(value["version"], 1);
    assert_eq!(value["rows"][0]["self_size_sum_a_bytes"].is_number(), true);
    assert_eq!(value["rows"][0]["self_size_sum_b_bytes"].is_number(), true);
    assert_eq!(
        value["rows"][0]["self_size_sum_delta_bytes"].is_number(),
        true
    );
}

#[test]
fn diff_csv_units_header() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot_a = read_snapshot_file(path, options).expect("snapshot a");

    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot_b = read_snapshot_file(path, options).expect("snapshot b");

    let result = diff_summaries(
        &snapshot_a,
        &snapshot_b,
        DiffOptions {
            top: 10,
            contains: None,
        },
    )
    .expect("diff");

    let csv = diff_output::format_csv(&result);
    let header = csv.lines().next().expect("csv header");
    assert_eq!(
        header,
        "constructor,count_a,count_b,count_delta,self_size_a_bytes,self_size_b_bytes,self_size_delta_bytes"
    );
}

#[test]
fn diff_html_includes_table() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot_a = read_snapshot_file(path, options).expect("snapshot a");

    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot_b = read_snapshot_file(path, options).expect("snapshot b");

    let result = diff_summaries(
        &snapshot_a,
        &snapshot_b,
        DiffOptions {
            top: 10,
            contains: None,
        },
    )
    .expect("diff");

    let html = diff_output::format_html(&result);
    assert!(html.contains("<table>"));
}
