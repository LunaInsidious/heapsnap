use std::fmt::Write as _;

use serde::Serialize;

use crate::analysis::diff::{DiffResult, DiffRow};
use crate::error::SnapshotError;

#[derive(Debug, Serialize)]
struct DiffJson<'a> {
    version: u32,
    total_nodes_a: usize,
    total_nodes_b: usize,
    rows: &'a [DiffRow],
}

pub fn format_markdown(result: &DiffResult) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# HeapSnapshot Diff");
    let _ = writeln!(
        output,
        "- Total nodes: A={} / B={}",
        result.total_nodes_a, result.total_nodes_b
    );
    let _ = writeln!(output, "");
    let _ = writeln!(
        output,
        "| Constructor | Count A | Count B | Δ Count | Self Size A | Self Size B | Δ Self Size |"
    );
    let _ = writeln!(output, "| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    for row in &result.rows {
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} |",
            escape_table(row.name.as_str()),
            row.count_a,
            row.count_b,
            row.count_delta,
            row.self_size_sum_a,
            row.self_size_sum_b,
            row.self_size_sum_delta
        );
    }
    output
}

pub fn format_json(result: &DiffResult) -> Result<String, SnapshotError> {
    let payload = DiffJson {
        version: 1,
        total_nodes_a: result.total_nodes_a,
        total_nodes_b: result.total_nodes_b,
        rows: &result.rows,
    };
    serde_json::to_string_pretty(&payload).map_err(SnapshotError::Json)
}

pub fn format_csv(result: &DiffResult) -> String {
    let mut output = String::new();
    output.push_str(
        "constructor,count_a,count_b,count_delta,self_size_a,self_size_b,self_size_delta\n",
    );
    for row in &result.rows {
        output.push('"');
        output.push_str(&row.name.replace('"', "\"\""));
        output.push('"');
        output.push(',');
        output.push_str(&row.count_a.to_string());
        output.push(',');
        output.push_str(&row.count_b.to_string());
        output.push(',');
        output.push_str(&row.count_delta.to_string());
        output.push(',');
        output.push_str(&row.self_size_sum_a.to_string());
        output.push(',');
        output.push_str(&row.self_size_sum_b.to_string());
        output.push(',');
        output.push_str(&row.self_size_sum_delta.to_string());
        output.push('\n');
    }
    output
}

fn escape_table(value: &str) -> String {
    value.replace('|', "\\|")
}
