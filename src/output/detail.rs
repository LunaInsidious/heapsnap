use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::analysis::detail::{
    DetailById, DetailByName, DetailResult, OutgoingEdgeSummary, RetainerSummary, ShallowSizeBucket,
};
use crate::error::SnapshotError;

const HEADER_PREVIEW_MAX: usize = 50;
const V8_HEAP_SNAPSHOT_STRING_LIMIT_DOC_URL: &str =
    "https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/flags/flag-definitions.h#3098";

#[derive(Debug, Serialize)]
struct DetailJson<'a> {
    version: u32,
    mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    node_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    self_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    constructor_summary: Option<ConstructorSummaryJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ids: Option<Vec<NodeRefJson>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retainers: Option<Vec<RetainerJson>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outgoing_edges: Option<Vec<OutgoingEdgeJson>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shallow_size_distribution: Option<Vec<ShallowSizeBucketJson>>,
}

#[derive(Debug, Serialize)]
struct ConstructorSummaryJson {
    total_count: u64,
    self_size_sum_bytes: i64,
    max_self_size_bytes: i64,
    min_self_size_bytes: i64,
    avg_self_size_bytes: f64,
    skip: usize,
    limit: usize,
    total_ids: u64,
}

#[derive(Debug, Serialize)]
struct NodeRefJson {
    index: usize,
    id: Option<i64>,
    node_type: Option<String>,
    self_size_bytes: i64,
}

#[derive(Debug, Serialize)]
struct RetainerJson {
    from_index: usize,
    from_id: Option<i64>,
    from_name: Option<String>,
    from_node_type: Option<String>,
    from_self_size_bytes: i64,
    edge_index: usize,
    edge_type: Option<String>,
    edge_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OutgoingEdgeJson {
    edge_index: usize,
    edge_type: Option<String>,
    edge_name: Option<String>,
    to_index: usize,
    to_id: Option<i64>,
    to_name: Option<String>,
    to_node_type: Option<String>,
    to_self_size_bytes: i64,
}

#[derive(Debug, Serialize)]
struct ShallowSizeBucketJson {
    label: String,
    min: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<i64>,
    count: u64,
}

pub fn format_markdown(result: &DetailResult) -> String {
    match result {
        DetailResult::ByName(payload) => format_markdown_name(payload),
        DetailResult::ById(payload) => format_markdown_id(payload),
    }
}

pub fn format_json(result: &DetailResult) -> Result<String, SnapshotError> {
    let payload = match result {
        DetailResult::ByName(detail) => DetailJson {
            version: 1,
            mode: "name",
            name: Some(detail.name.as_str()),
            id: None,
            node_type: None,
            self_size_bytes: None,
            constructor_summary: Some(summary_json(
                detail.total_count,
                detail.self_size_sum,
                detail.max_self_size,
                detail.min_self_size,
                detail.avg_self_size,
                detail.skip,
                detail.limit,
                detail.total_ids,
            )),
            ids: Some(node_refs_json(&detail.ids)),
            retainers: None,
            outgoing_edges: None,
            shallow_size_distribution: None,
        },
        DetailResult::ById(detail) => DetailJson {
            version: 1,
            mode: "id",
            name: Some(detail.name.as_str()),
            id: Some(detail.id),
            node_type: detail.node_type.as_deref(),
            self_size_bytes: Some(detail.self_size),
            constructor_summary: Some(summary_json(
                detail.total_count,
                detail.self_size_sum,
                detail.max_self_size,
                detail.min_self_size,
                detail.avg_self_size,
                detail.skip,
                detail.limit,
                detail.total_ids,
            )),
            ids: Some(node_refs_json(&detail.ids)),
            retainers: Some(retainers_json(&detail.retainers)),
            outgoing_edges: Some(outgoing_edges_json(&detail.outgoing_edges)),
            shallow_size_distribution: Some(shallow_size_json(&detail.shallow_size_distribution)),
        },
    };
    serde_json::to_string_pretty(&payload).map_err(SnapshotError::Json)
}

pub fn format_csv(result: &DetailResult) -> String {
    let mut output = String::new();
    output.push_str("section,field,value,extra1,extra2,extra3,extra4,extra5,extra6\n");
    match result {
        DetailResult::ByName(detail) => {
            csv_summary(&mut output, detail.name.as_str(), detail);
            csv_ids(&mut output, &detail.ids);
        }
        DetailResult::ById(detail) => {
            csv_summary(&mut output, detail.name.as_str(), detail);
            push_csv_row(&mut output, &["id", "", detail.id.to_string().as_str()]);
            if let Some(node_type) = detail.node_type.as_deref() {
                push_csv_row(&mut output, &["node_type", "", node_type]);
            }
            push_csv_row(
                &mut output,
                &["self_size_bytes", "", detail.self_size.to_string().as_str()],
            );
            csv_ids(&mut output, &detail.ids);
            csv_retainers(&mut output, &detail.retainers);
            csv_outgoing_edges(&mut output, &detail.outgoing_edges);
            csv_distribution(&mut output, &detail.shallow_size_distribution);
        }
    }
    output
}

pub fn format_html(result: &DetailResult, source_path: &Path) -> String {
    match result {
        DetailResult::ByName(detail) => format_html_name(detail, source_path),
        DetailResult::ById(detail) => format_html_id(detail, source_path),
    }
}

fn format_markdown_name(detail: &DetailByName) -> String {
    let mut output = String::new();
    write_markdown_constructor_header(&mut output, &detail.name, None);
    write_summary_markdown(&mut output, detail);
    let _ = writeln!(output, "");
    let _ = writeln!(output, "## Node IDs");
    write_ids_markdown(&mut output, &detail.ids);
    output
}

fn format_markdown_id(detail: &DetailById) -> String {
    let mut output = String::new();
    write_markdown_constructor_header(&mut output, &detail.name, Some(detail.id));
    if let Some(node_type) = detail.node_type.as_deref() {
        let _ = writeln!(output, "- Node type: {}", node_type);
    }
    let _ = writeln!(output, "- Self size: {}", detail.self_size);
    write_summary_markdown(&mut output, detail);
    let _ = writeln!(output, "");
    let _ = writeln!(output, "## Node IDs");
    write_ids_markdown(&mut output, &detail.ids);
    let _ = writeln!(output, "");
    let _ = writeln!(output, "## Top Retainers");
    write_retainers_markdown(&mut output, &detail.retainers);
    let _ = writeln!(output, "");
    let _ = writeln!(output, "## Top Outgoing Edges");
    write_outgoing_edges_markdown(&mut output, &detail.outgoing_edges);
    let _ = writeln!(output, "");
    let _ = writeln!(output, "## Shallow Size Distribution");
    write_distribution_markdown(&mut output, &detail.shallow_size_distribution);
    output
}

fn write_markdown_constructor_header(output: &mut String, name: &str, id: Option<u64>) {
    let compact = normalize_header_name(name);
    let name_len = compact.chars().count();

    if name_len <= HEADER_PREVIEW_MAX {
        if let Some(id) = id {
            let _ = writeln!(output, "# Detail: {} (id={})", compact, id);
        } else {
            let _ = writeln!(output, "# Detail: {}", compact);
        }
        return;
    }

    let preview = truncate_chars(&compact, HEADER_PREVIEW_MAX);
    if let Some(id) = id {
        let _ = writeln!(output, "# Detail: {}… (id={})", preview, id);
    } else {
        let _ = writeln!(output, "# Detail: {}…", preview);
    }
    let _ = writeln!(output, "- Constructor chars: {}", name_len);
    write_markdown_constructor_limit_note(output, name_len);
    let _ = writeln!(
        output,
        "<details><summary>Full constructor name</summary><div>{}</div></details>",
        escape_html_inline(&compact)
    );
}

fn write_markdown_constructor_limit_note(output: &mut String, name_len: usize) {
    if name_len == 1024 {
        let _ = writeln!(
            output,
            "- Note: `Constructor chars` が 1024 の場合、snapshot 生成時の V8 flag `heap_snapshot_string_limit` により切り詰められている可能性があります: {}",
            V8_HEAP_SNAPSHOT_STRING_LIMIT_DOC_URL
        );
    }
}

fn write_summary_markdown<T>(output: &mut String, detail: &T)
where
    T: DetailSummaryView,
{
    let _ = writeln!(output, "");
    let _ = writeln!(output, "## Constructor Summary");
    let _ = writeln!(output, "- Count: {}", detail.total_count());
    let _ = writeln!(output, "- Self size sum: {}", detail.self_size_sum());
    let _ = writeln!(output, "- Max self size: {}", detail.max_self_size());
    let _ = writeln!(output, "- Min self size: {}", detail.min_self_size());
    let _ = writeln!(output, "- Avg self size: {:.2}", detail.avg_self_size());
    let _ = writeln!(
        output,
        "- IDs (showing {}..{} of {}):",
        detail.skip(),
        detail.skip() + detail.ids().len(),
        detail.total_ids()
    );
}

fn write_ids_markdown(output: &mut String, ids: &[crate::analysis::detail::NodeRef]) {
    let _ = writeln!(output, "| Index | Node ID | Self Size | Node Type |");
    let _ = writeln!(output, "| ---: | ---: | ---: | --- |");
    for item in ids {
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} |",
            item.index,
            item.id.unwrap_or(-1),
            item.self_size,
            item.node_type.as_deref().unwrap_or("")
        );
    }
}

fn write_retainers_markdown(output: &mut String, retainers: &[RetainerSummary]) {
    let _ = writeln!(
        output,
        "| From Index | From ID | From Name | From Type | From Self Size | Edge Type | Edge Name |"
    );
    let _ = writeln!(output, "| ---: | ---: | --- | --- | ---: | --- | --- |");
    for item in retainers {
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} |",
            item.from_index,
            item.from_id.unwrap_or(-1),
            item.from_name.as_deref().unwrap_or(""),
            item.from_node_type.as_deref().unwrap_or(""),
            item.from_self_size,
            item.edge_type.as_deref().unwrap_or(""),
            item.edge_name.as_deref().unwrap_or("")
        );
    }
}

fn write_outgoing_edges_markdown(output: &mut String, edges: &[OutgoingEdgeSummary]) {
    let _ = writeln!(
        output,
        "| Edge Index | Edge Type | Edge Name | To Index | To ID | To Name | To Type | To Self Size |"
    );
    let _ = writeln!(
        output,
        "| ---: | --- | --- | ---: | ---: | --- | --- | ---: |"
    );
    for item in edges {
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            item.edge_index,
            item.edge_type.as_deref().unwrap_or(""),
            item.edge_name.as_deref().unwrap_or(""),
            item.to_index,
            item.to_id.unwrap_or(-1),
            item.to_name.as_deref().unwrap_or(""),
            item.to_node_type.as_deref().unwrap_or(""),
            item.to_self_size
        );
    }
}

fn write_distribution_markdown(output: &mut String, buckets: &[ShallowSizeBucket]) {
    let _ = writeln!(output, "| Bucket | Min | Max | Count |");
    let _ = writeln!(output, "| --- | ---: | ---: | ---: |");
    for item in buckets {
        let _ = writeln!(
            output,
            "| {} | {} | {} | {} |",
            item.label,
            item.min,
            item.max
                .map(|v| v.to_string())
                .unwrap_or_else(|| "".to_string()),
            item.count
        );
    }
}

fn format_html_name(detail: &DetailByName, source_path: &Path) -> String {
    let mut output = String::new();
    let title = "HeapSnapshot Detail";
    let file_label = escape_html_inline(&source_path.display().to_string());

    let _ = writeln!(
        output,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><style>{}</style></head><body>",
        base_styles()
    );
    let _ = writeln!(
        output,
        "<h1>{title}</h1><p><strong>File:</strong> {file_label}</p>"
    );
    write_html_constructor_header(&mut output, &detail.name, None);
    write_summary_html(&mut output, detail);
    let _ = writeln!(output, "<h3>Node IDs</h3>");
    write_ids_html(&mut output, &detail.ids);
    let _ = writeln!(
        output,
        "<p class=\"note\">This HTML is a static report. Run <code>heapsnap detail</code> manually for per-id details.</p>"
    );
    let _ = writeln!(output, "</body></html>");
    output
}

fn format_html_id(detail: &DetailById, source_path: &Path) -> String {
    let mut output = String::new();
    let title = "HeapSnapshot Detail";
    let file_label = escape_html_inline(&source_path.display().to_string());

    let _ = writeln!(
        output,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><style>{}</style></head><body>",
        base_styles()
    );
    let _ = writeln!(
        output,
        "<h1>{title}</h1><p><strong>File:</strong> {file_label}</p>"
    );
    write_html_constructor_header(&mut output, &detail.name, Some(detail.id));
    if let Some(node_type) = detail.node_type.as_deref() {
        let _ = writeln!(
            output,
            "<p><strong>Node type:</strong> {}</p>",
            escape_html_inline(node_type)
        );
    }
    let _ = writeln!(
        output,
        "<p><strong>Self size:</strong> {}</p>",
        detail.self_size
    );
    write_summary_html(&mut output, detail);
    let _ = writeln!(output, "<h3>Node IDs</h3>");
    write_ids_html(&mut output, &detail.ids);
    let _ = writeln!(output, "<h3>Top Retainers</h3>");
    write_retainers_html(&mut output, &detail.retainers);
    let _ = writeln!(output, "<h3>Top Outgoing Edges</h3>");
    write_outgoing_edges_html(&mut output, &detail.outgoing_edges);
    let _ = writeln!(output, "<h3>Shallow Size Distribution</h3>");
    write_distribution_html(&mut output, &detail.shallow_size_distribution);
    let _ = writeln!(
        output,
        "<p class=\"note\">This HTML is a static report.</p>"
    );
    let _ = writeln!(output, "</body></html>");
    output
}

fn write_html_constructor_header(output: &mut String, name: &str, id: Option<u64>) {
    let compact = normalize_header_name(name);
    let name_len = compact.chars().count();

    if name_len <= HEADER_PREVIEW_MAX {
        if let Some(id) = id {
            let _ = writeln!(
                output,
                "<h2>Name: {} (id={})</h2>",
                escape_html_inline(&compact),
                id
            );
        } else {
            let _ = writeln!(output, "<h2>Name: {}</h2>", escape_html_inline(&compact));
        }
        return;
    }

    let preview = truncate_chars(&compact, HEADER_PREVIEW_MAX);
    if let Some(id) = id {
        let _ = writeln!(
            output,
            "<h2>Name: {}… (id={})</h2>",
            escape_html_inline(&preview),
            id
        );
    } else {
        let _ = writeln!(output, "<h2>Name: {}…</h2>", escape_html_inline(&preview));
    }
    let _ = writeln!(
        output,
        "<p><strong>Constructor chars:</strong> {}</p>",
        name_len
    );
    write_html_constructor_limit_note(output, name_len);
    let _ = writeln!(
        output,
        "<details><summary>Full constructor name</summary><div>{}</div></details>",
        escape_html_inline(&compact)
    );
}

fn write_html_constructor_limit_note(output: &mut String, name_len: usize) {
    if name_len == 1024 {
        let _ = writeln!(
            output,
            "<p><strong>Note:</strong> <code>Constructor chars</code> が 1024 の場合、snapshot 生成時の V8 flag <code>heap_snapshot_string_limit</code> により切り詰められている可能性があります: <a href=\"{0}\">{0}</a></p>",
            V8_HEAP_SNAPSHOT_STRING_LIMIT_DOC_URL
        );
    }
}

fn write_summary_html<T>(output: &mut String, detail: &T)
where
    T: DetailSummaryView,
{
    let _ = writeln!(output, "<h3>Constructor Summary</h3>");
    let _ = writeln!(output, "<ul>");
    let _ = writeln!(output, "<li>Count: {}</li>", detail.total_count());
    let _ = writeln!(output, "<li>Self size sum: {}</li>", detail.self_size_sum());
    let _ = writeln!(output, "<li>Max self size: {}</li>", detail.max_self_size());
    let _ = writeln!(output, "<li>Min self size: {}</li>", detail.min_self_size());
    let _ = writeln!(
        output,
        "<li>Avg self size: {:.2}</li>",
        detail.avg_self_size()
    );
    let _ = writeln!(
        output,
        "<li>IDs (showing {}..{} of {}):</li>",
        detail.skip(),
        detail.skip() + detail.ids().len(),
        detail.total_ids()
    );
    let _ = writeln!(output, "</ul>");
}

fn write_ids_html(output: &mut String, ids: &[crate::analysis::detail::NodeRef]) {
    let _ = writeln!(
        output,
        "<table><thead><tr><th>Index</th><th>ID</th><th>Self Size</th><th>Node Type</th></tr></thead><tbody>"
    );
    for item in ids {
        let id_value = item.id.unwrap_or(-1);
        let _ = writeln!(
            output,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            item.index,
            id_value,
            item.self_size,
            escape_html_inline(item.node_type.as_deref().unwrap_or(""))
        );
    }
    let _ = writeln!(output, "</tbody></table>");
}

fn write_retainers_html(output: &mut String, retainers: &[RetainerSummary]) {
    let _ = writeln!(
        output,
        "<table><thead><tr><th>From Index</th><th>From ID</th><th>From Name</th><th>From Type</th><th>From Self Size</th><th>Edge Type</th><th>Edge Name</th></tr></thead><tbody>"
    );
    for item in retainers {
        let _ = writeln!(
            output,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            item.from_index,
            item.from_id.unwrap_or(-1),
            escape_html_inline(item.from_name.as_deref().unwrap_or("")),
            escape_html_inline(item.from_node_type.as_deref().unwrap_or("")),
            item.from_self_size,
            escape_html_inline(item.edge_type.as_deref().unwrap_or("")),
            escape_html_inline(item.edge_name.as_deref().unwrap_or(""))
        );
    }
    let _ = writeln!(output, "</tbody></table>");
}

fn write_outgoing_edges_html(output: &mut String, edges: &[OutgoingEdgeSummary]) {
    let _ = writeln!(
        output,
        "<table><thead><tr><th>Edge Index</th><th>Edge Type</th><th>Edge Name</th><th>To Index</th><th>To ID</th><th>To Name</th><th>To Type</th><th>To Self Size</th></tr></thead><tbody>"
    );
    for item in edges {
        let _ = writeln!(
            output,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            item.edge_index,
            escape_html_inline(item.edge_type.as_deref().unwrap_or("")),
            escape_html_inline(item.edge_name.as_deref().unwrap_or("")),
            item.to_index,
            item.to_id.unwrap_or(-1),
            escape_html_inline(item.to_name.as_deref().unwrap_or("")),
            escape_html_inline(item.to_node_type.as_deref().unwrap_or("")),
            item.to_self_size
        );
    }
    let _ = writeln!(output, "</tbody></table>");
}

fn write_distribution_html(output: &mut String, buckets: &[ShallowSizeBucket]) {
    let _ = writeln!(
        output,
        "<table><thead><tr><th>Bucket</th><th>Min</th><th>Max</th><th>Count</th></tr></thead><tbody>"
    );
    for item in buckets {
        let max = item.max.map(|v| v.to_string()).unwrap_or_default();
        let _ = writeln!(
            output,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html_inline(&item.label),
            item.min,
            max,
            item.count
        );
    }
    let _ = writeln!(output, "</tbody></table>");
}

fn summary_json(
    total_count: u64,
    self_size_sum: i64,
    max_self_size: i64,
    min_self_size: i64,
    avg_self_size: f64,
    skip: usize,
    limit: usize,
    total_ids: u64,
) -> ConstructorSummaryJson {
    ConstructorSummaryJson {
        total_count,
        self_size_sum_bytes: self_size_sum,
        max_self_size_bytes: max_self_size,
        min_self_size_bytes: min_self_size,
        avg_self_size_bytes: avg_self_size,
        skip,
        limit,
        total_ids,
    }
}

fn node_refs_json(nodes: &[crate::analysis::detail::NodeRef]) -> Vec<NodeRefJson> {
    nodes
        .iter()
        .map(|node| NodeRefJson {
            index: node.index,
            id: node.id,
            node_type: node.node_type.clone(),
            self_size_bytes: node.self_size,
        })
        .collect()
}

fn retainers_json(items: &[RetainerSummary]) -> Vec<RetainerJson> {
    items
        .iter()
        .map(|item| RetainerJson {
            from_index: item.from_index,
            from_id: item.from_id,
            from_name: item.from_name.clone(),
            from_node_type: item.from_node_type.clone(),
            from_self_size_bytes: item.from_self_size,
            edge_index: item.edge_index,
            edge_type: item.edge_type.clone(),
            edge_name: item.edge_name.clone(),
        })
        .collect()
}

fn outgoing_edges_json(items: &[OutgoingEdgeSummary]) -> Vec<OutgoingEdgeJson> {
    items
        .iter()
        .map(|item| OutgoingEdgeJson {
            edge_index: item.edge_index,
            edge_type: item.edge_type.clone(),
            edge_name: item.edge_name.clone(),
            to_index: item.to_index,
            to_id: item.to_id,
            to_name: item.to_name.clone(),
            to_node_type: item.to_node_type.clone(),
            to_self_size_bytes: item.to_self_size,
        })
        .collect()
}

fn shallow_size_json(items: &[ShallowSizeBucket]) -> Vec<ShallowSizeBucketJson> {
    items
        .iter()
        .map(|item| ShallowSizeBucketJson {
            label: item.label.clone(),
            min: item.min,
            max: item.max,
            count: item.count,
        })
        .collect()
}

fn csv_summary<T>(output: &mut String, name: &str, detail: &T)
where
    T: DetailSummaryView,
{
    push_csv_row(output, &["summary", "name", name]);
    push_csv_row(
        output,
        &[
            "summary",
            "total_count",
            detail.total_count().to_string().as_str(),
        ],
    );
    push_csv_row(
        output,
        &[
            "summary",
            "self_size_sum_bytes",
            detail.self_size_sum().to_string().as_str(),
        ],
    );
    push_csv_row(
        output,
        &[
            "summary",
            "max_self_size_bytes",
            detail.max_self_size().to_string().as_str(),
        ],
    );
    push_csv_row(
        output,
        &[
            "summary",
            "min_self_size_bytes",
            detail.min_self_size().to_string().as_str(),
        ],
    );
    push_csv_row(
        output,
        &[
            "summary",
            "avg_self_size_bytes",
            format!("{:.2}", detail.avg_self_size()).as_str(),
        ],
    );
    push_csv_row(
        output,
        &["summary", "skip", detail.skip().to_string().as_str()],
    );
    push_csv_row(
        output,
        &["summary", "limit", detail.limit().to_string().as_str()],
    );
    push_csv_row(
        output,
        &[
            "summary",
            "total_ids",
            detail.total_ids().to_string().as_str(),
        ],
    );
}

fn csv_ids(output: &mut String, ids: &[crate::analysis::detail::NodeRef]) {
    for item in ids {
        push_csv_row(
            output,
            &[
                "ids",
                item.index.to_string().as_str(),
                item.id.unwrap_or(-1).to_string().as_str(),
                item.self_size.to_string().as_str(),
                item.node_type.as_deref().unwrap_or(""),
            ],
        );
    }
}

fn csv_retainers(output: &mut String, retainers: &[RetainerSummary]) {
    for item in retainers {
        push_csv_row(
            output,
            &[
                "retainers",
                item.from_index.to_string().as_str(),
                item.from_id.unwrap_or(-1).to_string().as_str(),
                item.from_name.as_deref().unwrap_or(""),
                item.from_node_type.as_deref().unwrap_or(""),
                item.from_self_size.to_string().as_str(),
                item.edge_type.as_deref().unwrap_or(""),
                item.edge_name.as_deref().unwrap_or(""),
            ],
        );
    }
}

fn csv_outgoing_edges(output: &mut String, edges: &[OutgoingEdgeSummary]) {
    for item in edges {
        push_csv_row(
            output,
            &[
                "outgoing_edges",
                item.edge_index.to_string().as_str(),
                item.edge_type.as_deref().unwrap_or(""),
                item.edge_name.as_deref().unwrap_or(""),
                item.to_index.to_string().as_str(),
                item.to_id.unwrap_or(-1).to_string().as_str(),
                item.to_name.as_deref().unwrap_or(""),
                item.to_node_type.as_deref().unwrap_or(""),
                item.to_self_size.to_string().as_str(),
            ],
        );
    }
}

fn csv_distribution(output: &mut String, buckets: &[ShallowSizeBucket]) {
    for item in buckets {
        push_csv_row(
            output,
            &[
                "distribution",
                item.label.as_str(),
                item.min.to_string().as_str(),
                item.max.map(|v| v.to_string()).unwrap_or_default().as_str(),
                item.count.to_string().as_str(),
            ],
        );
    }
}

fn push_csv_row(output: &mut String, fields: &[&str]) {
    let mut first = true;
    for field in fields {
        if !first {
            output.push(',');
        }
        first = false;
        output.push('"');
        output.push_str(&field.replace('"', "\"\""));
        output.push('"');
    }
    output.push('\n');
}

trait DetailSummaryView {
    fn total_count(&self) -> u64;
    fn self_size_sum(&self) -> i64;
    fn max_self_size(&self) -> i64;
    fn min_self_size(&self) -> i64;
    fn avg_self_size(&self) -> f64;
    fn ids(&self) -> &[crate::analysis::detail::NodeRef];
    fn skip(&self) -> usize;
    fn limit(&self) -> usize;
    fn total_ids(&self) -> u64;
}

impl DetailSummaryView for DetailByName {
    fn total_count(&self) -> u64 {
        self.total_count
    }
    fn self_size_sum(&self) -> i64 {
        self.self_size_sum
    }
    fn max_self_size(&self) -> i64 {
        self.max_self_size
    }
    fn min_self_size(&self) -> i64 {
        self.min_self_size
    }
    fn avg_self_size(&self) -> f64 {
        self.avg_self_size
    }
    fn ids(&self) -> &[crate::analysis::detail::NodeRef] {
        &self.ids
    }
    fn skip(&self) -> usize {
        self.skip
    }
    fn limit(&self) -> usize {
        self.limit
    }
    fn total_ids(&self) -> u64 {
        self.total_ids
    }
}

impl DetailSummaryView for DetailById {
    fn total_count(&self) -> u64 {
        self.total_count
    }
    fn self_size_sum(&self) -> i64 {
        self.self_size_sum
    }
    fn max_self_size(&self) -> i64 {
        self.max_self_size
    }
    fn min_self_size(&self) -> i64 {
        self.min_self_size
    }
    fn avg_self_size(&self) -> f64 {
        self.avg_self_size
    }
    fn ids(&self) -> &[crate::analysis::detail::NodeRef] {
        &self.ids
    }
    fn skip(&self) -> usize {
        self.skip
    }
    fn limit(&self) -> usize {
        self.limit
    }
    fn total_ids(&self) -> u64 {
        self.total_ids
    }
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

fn normalize_header_name(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn base_styles() -> &'static str {
    "body{font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;width:100%;margin-top:8px}th,td{border:1px solid #ddd;padding:6px;vertical-align:top}th{text-align:left;background:#f6f6f6}tr:nth-child(even){background:#fafafa}h3{margin-top:18px}.note{margin-top:16px;color:#444;font-size:0.9em}"
}
