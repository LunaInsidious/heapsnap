use std::fmt::Write as _;

use serde::Serialize;

use crate::analysis::retainers::{RetainerLink, RetainersResult};
use crate::error::SnapshotError;
use crate::snapshot::{EdgeView, SnapshotRaw};

#[derive(Debug, Serialize)]
struct RetainersJson {
    version: u32,
    target: NodeJson,
    paths: Vec<PathJson>,
}

#[derive(Debug, Serialize)]
struct PathJson {
    steps: Vec<StepJson>,
}

#[derive(Debug, Serialize)]
struct StepJson {
    from: NodeJson,
    edge: EdgeJson,
    to: NodeJson,
}

#[derive(Debug, Serialize)]
struct NodeJson {
    index: usize,
    id: Option<i64>,
    name: Option<String>,
    node_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct EdgeJson {
    index: usize,
    edge_type: Option<String>,
    name_or_index: Option<i64>,
    name: Option<String>,
}

pub fn format_markdown(snapshot: &SnapshotRaw, result: &RetainersResult) -> String {
    let mut output = String::new();
    let target = snapshot.node_view(result.target);
    let target_name = target
        .and_then(|node| node.name())
        .map(escape_inline_with_details)
        .unwrap_or_else(|| "<unknown>".to_string());
    let target_id = target.and_then(|node| node.id()).unwrap_or(-1);
    let _ = writeln!(
        output,
        "- Retaining paths for {} (id={})",
        target_name, target_id
    );

    for (index, path) in result.paths.iter().enumerate() {
        let _ = writeln!(output, "  - Path #{}", index + 1);
        for step in path {
            let line = format_step(snapshot, step);
            let _ = writeln!(output, "    - {line}");
        }
    }

    output
}

pub fn format_json(
    snapshot: &SnapshotRaw,
    result: &RetainersResult,
) -> Result<String, SnapshotError> {
    let target = node_json(snapshot, result.target);
    let mut paths = Vec::new();
    for path in &result.paths {
        let mut steps = Vec::new();
        for step in path {
            let from = node_json(snapshot, step.from_node);
            let to = node_json(snapshot, step.to_node);
            let edge = edge_json(snapshot, step.edge_index);
            steps.push(StepJson { from, edge, to });
        }
        paths.push(PathJson { steps });
    }

    let payload = RetainersJson {
        version: 1,
        target,
        paths,
    };
    serde_json::to_string_pretty(&payload).map_err(SnapshotError::Json)
}

pub fn format_html(snapshot: &SnapshotRaw, result: &RetainersResult) -> String {
    let mut output = String::new();
    let title = "HeapSnapshot Retainers";
    let target = snapshot.node_view(result.target);
    let target_name = target
        .and_then(|node| node.name())
        .map(escape_html_inline)
        .unwrap_or_else(|| "<unknown>".to_string());
    let target_id = target.and_then(|node| node.id()).unwrap_or(-1);

    let _ = writeln!(
        output,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><style>{}</style></head><body>",
        base_styles()
    );
    let _ = writeln!(
        output,
        "<h1>{title}</h1><p><strong>Target:</strong> {} (id={})</p>",
        target_name, target_id
    );

    for (index, path) in result.paths.iter().enumerate() {
        let _ = writeln!(output, "<h2>Path #{}</h2>", index + 1);
        let _ = writeln!(output, "<ol>");
        for step in path {
            let line = format_step(snapshot, step);
            let _ = writeln!(output, "<li>{line}</li>");
        }
        let _ = writeln!(output, "</ol>");
    }

    let _ = writeln!(output, "</body></html>");
    output
}

fn node_json(snapshot: &SnapshotRaw, node_index: usize) -> NodeJson {
    let node = snapshot.node_view(node_index);
    NodeJson {
        index: node_index,
        id: node.and_then(|value| value.id()),
        name: node.and_then(|value| value.name()).map(str::to_string),
        node_type: node.and_then(|value| value.node_type()).map(str::to_string),
    }
}

fn edge_json(snapshot: &SnapshotRaw, edge_index: usize) -> EdgeJson {
    let edge = snapshot.edge_view(edge_index);
    let name_or_index = edge.and_then(|value| value.name_or_index());
    EdgeJson {
        index: edge_index,
        edge_type: edge.and_then(|value| value.edge_type()).map(str::to_string),
        name_or_index,
        name: edge_name(snapshot, edge),
    }
}

fn format_step(snapshot: &SnapshotRaw, step: &RetainerLink) -> String {
    let from = snapshot.node_view(step.from_node);
    let to = snapshot.node_view(step.to_node);
    let edge = snapshot.edge_view(step.edge_index);

    let from_name = from
        .and_then(|node| node.name())
        .map(escape_inline_with_details)
        .unwrap_or_else(|| "<unknown>".to_string());
    let to_name = to
        .and_then(|node| node.name())
        .map(escape_inline_with_details)
        .unwrap_or_else(|| "<unknown>".to_string());
    let edge_type = edge
        .and_then(|value| value.edge_type())
        .map(escape_inline_with_details)
        .unwrap_or_else(|| "unknown".to_string());
    let edge_name = edge_name(snapshot, edge)
        .as_deref()
        .map(escape_inline_with_details)
        .unwrap_or_else(|| "<unknown>".to_string());

    format!("{from_name} --({edge_type}){edge_name}--> {to_name}")
}

fn edge_name(snapshot: &SnapshotRaw, edge: Option<EdgeView<'_>>) -> Option<String> {
    let edge = edge?;
    let edge_type = edge.edge_type().unwrap_or("unknown");
    let name_or_index = edge.name_or_index().unwrap_or(-1);

    if edge_type == "element" {
        return Some(format!("[{name_or_index}]"));
    }

    if name_or_index >= 0 {
        let idx = name_or_index as usize;
        if let Some(name) = snapshot.strings.get(idx) {
            return Some(name.to_string());
        }
        return Some(format!("<string:{name_or_index}>"));
    }

    Some(format!("<name:{name_or_index}>"))
}

fn escape_inline_with_details(value: &str) -> String {
    const MAX_LEN: usize = 120;
    let normalized = normalize_whitespace(value);
    if normalized.chars().count() <= MAX_LEN {
        return escape_inline(&normalized);
    }
    let summary = truncate_chars(&normalized, MAX_LEN);
    let summary = escape_html_inline(&summary);
    let full = escape_html_inline(&normalized);
    format!("<details><summary>{summary}…</summary><div>{full}</div></details>")
}

fn escape_inline(value: &str) -> String {
    let mut escaped = value.replace('\r', "");
    escaped = escaped.replace('\n', " ");
    escaped = escaped.replace('|', "\\|");
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

fn base_styles() -> &'static str {
    "body{font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;color:#111}h2{margin-top:20px}ol{padding-left:20px}li{margin:6px 0}"
}
