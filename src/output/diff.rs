use std::fmt::Write as _;

use serde::Serialize;

use crate::analysis::diff::DiffResult;
use crate::error::SnapshotError;

#[derive(Debug, Serialize)]
struct DiffJson<'a> {
    version: u32,
    total_nodes_a: usize,
    total_nodes_b: usize,
    rows: Vec<DiffRowJson<'a>>,
}

#[derive(Debug, Serialize)]
struct DiffRowJson<'a> {
    name: &'a str,
    count_a: u64,
    count_b: u64,
    count_delta: i64,
    #[serde(rename = "self_size_sum_a_bytes")]
    self_size_sum_a_bytes: i64,
    #[serde(rename = "self_size_sum_b_bytes")]
    self_size_sum_b_bytes: i64,
    #[serde(rename = "self_size_sum_delta_bytes")]
    self_size_sum_delta_bytes: i64,
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
        "| Constructor | Count A | Count B | Δ Count | Self Size A (bytes) | Self Size B (bytes) | Δ Self Size (bytes) |"
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
    let rows = result
        .rows
        .iter()
        .map(|row| DiffRowJson {
            name: row.name.as_str(),
            count_a: row.count_a,
            count_b: row.count_b,
            count_delta: row.count_delta,
            self_size_sum_a_bytes: row.self_size_sum_a,
            self_size_sum_b_bytes: row.self_size_sum_b,
            self_size_sum_delta_bytes: row.self_size_sum_delta,
        })
        .collect::<Vec<_>>();
    let payload = DiffJson {
        version: 1,
        total_nodes_a: result.total_nodes_a,
        total_nodes_b: result.total_nodes_b,
        rows,
    };
    serde_json::to_string_pretty(&payload).map_err(SnapshotError::Json)
}

pub fn format_csv(result: &DiffResult) -> String {
    let mut output = String::new();
    output.push_str(
        "constructor,count_a,count_b,count_delta,self_size_a_bytes,self_size_b_bytes,self_size_delta_bytes\n",
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

pub fn format_html(result: &DiffResult) -> String {
    let mut output = String::new();
    let title = "HeapSnapshot Diff";
    let _ = writeln!(
        output,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><style>{}</style></head><body>",
        base_styles()
    );
    let _ = writeln!(output, "<h1>{title}</h1>");
    let _ = writeln!(
        output,
        "<p><strong>Total nodes:</strong> A={} / B={}</p>",
        result.total_nodes_a, result.total_nodes_b
    );
    let _ = writeln!(
        output,
        "<table><thead><tr><th>Constructor</th><th>Count A</th><th>Count B</th><th>Δ Count</th><th>Self Size A (bytes)</th><th>Self Size B (bytes)</th><th>Δ Self Size (bytes)</th></tr></thead><tbody>"
    );
    for row in &result.rows {
        let name = escape_html_inline(row.name.as_str());
        let _ = writeln!(
            output,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            name,
            row.count_a,
            row.count_b,
            row.count_delta,
            row.self_size_sum_a,
            row.self_size_sum_b,
            row.self_size_sum_delta
        );
    }
    let _ = writeln!(output, "</tbody></table>");
    let _ = writeln!(output, "</body></html>");
    output
}

fn escape_html_inline(value: &str) -> String {
    let mut escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    escaped = escaped.replace('\r', "");
    escaped = escaped.replace('\n', "<br>");
    escaped
}

fn base_styles() -> &'static str {
    "body{font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;width:100%;margin-top:12px}th,td{border:1px solid #ddd;padding:8px;vertical-align:top}th{text-align:left;background:#f6f6f6}tr:nth-child(even){background:#fafafa}"
}
