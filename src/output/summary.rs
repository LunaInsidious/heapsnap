use std::fmt::Write as _;

use serde::Serialize;

use crate::analysis::summary::{SummaryResult, SummaryRow};
use crate::error::SnapshotError;

#[derive(Debug, Serialize)]
struct SummaryJson<'a> {
    version: u32,
    total_nodes: usize,
    rows: &'a [SummaryRow],
}

pub fn format_markdown(result: &SummaryResult) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# HeapSnapshot Summary");
    let _ = writeln!(output, "");
    let _ = writeln!(output, "- Total nodes: {}", result.total_nodes);
    let _ = writeln!(output, "");
    let _ = writeln!(output, "| Constructor | Count | Self Size Sum |");
    let _ = writeln!(output, "| --- | ---: | ---: |");
    for row in &result.rows {
        let name = if row.name.is_empty() {
            format_empty_name(&result.empty_name_types)
        } else {
            row.name.clone()
        };
        let _ = writeln!(
            output,
            "| {} | {} | {} |",
            escape_table_cell(name.as_str()),
            row.count,
            row.self_size_sum
        );
    }
    output
}

pub fn format_json(result: &SummaryResult) -> Result<String, SnapshotError> {
    let payload = SummaryJson {
        version: 1,
        total_nodes: result.total_nodes,
        rows: &result.rows,
    };
    serde_json::to_string_pretty(&payload).map_err(SnapshotError::Json)
}

pub fn format_csv(result: &SummaryResult) -> String {
    let mut output = String::new();
    output.push_str("constructor,count,self_size_sum\n");
    for row in &result.rows {
        output.push('"');
        output.push_str(&row.name.replace('"', "\"\""));
        output.push('"');
        output.push(',');
        output.push_str(&row.count.to_string());
        output.push(',');
        output.push_str(&row.self_size_sum.to_string());
        output.push('\n');
    }
    output
}

fn escape_table(value: &str) -> String {
    value.replace('|', "\\|")
}

fn escape_table_cell(value: &str) -> String {
    const MAX_LEN: usize = 120;
    let normalized = normalize_whitespace(value);
    if normalized.chars().count() <= MAX_LEN {
        return escape_table_inline(&normalized);
    }

    let summary = truncate_chars(&normalized, MAX_LEN);
    let summary = escape_html_inline(&summary);
    let full = escape_html_inline(&normalized);
    format!("<details><summary>{summary}…</summary><div>{full}</div></details>")
}

fn escape_table_inline(value: &str) -> String {
    let mut escaped = escape_table(value);
    escaped = escaped.replace('\r', "");
    escaped = escaped.replace('\n', "<br>");
    escaped = escaped.replace('`', "\\`");
    escaped = escaped.replace('$', "\\$");
    escaped
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn normalize_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(ch);
    }
    let collapsed = out.trim().to_string();
    de_spaced(&collapsed)
}

fn de_spaced(value: &str) -> String {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    if tokens.is_empty() {
        return String::new();
    }

    let single = tokens.iter().filter(|t| t.chars().count() == 1).count();
    if single * 10 < tokens.len() * 7 {
        return value.to_string();
    }

    let mut out: Vec<String> = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    for token in tokens {
        if token.chars().count() == 1 {
            buf.push(token);
        } else {
            if !buf.is_empty() {
                if buf.len() >= 3 {
                    out.push(buf.concat());
                } else {
                    out.extend(buf.iter().map(|t| t.to_string()));
                }
                buf.clear();
            }
            out.push(token.to_string());
        }
    }
    if !buf.is_empty() {
        if buf.len() >= 3 {
            out.push(buf.concat());
        } else {
            out.extend(buf.iter().map(|t| t.to_string()));
        }
    }

    out.join(" ")
}

fn escape_html_inline(value: &str) -> String {
    let mut escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    escaped = escaped.replace('$', "&#36;");
    escaped = escaped.replace('|', "&#124;");
    escaped = escaped.replace('`', "&#96;");
    escaped = escaped.replace('\r', "");
    escaped = escaped.replace('\n', "<br>");
    escaped
}

fn format_empty_name(types: &[crate::analysis::summary::EmptyTypeSummary]) -> String {
    if types.is_empty() {
        return "(empty)".to_string();
    }
    let mut parts = Vec::new();
    for item in types.iter().take(3) {
        parts.push(format!("{}={}", item.node_type, item.count));
    }
    let suffix = if types.len() > 3 { ", …" } else { "" };
    format!("(empty; types: {}{suffix})", parts.join(", "))
}
