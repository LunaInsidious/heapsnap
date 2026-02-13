use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::analysis;
use crate::cancel::CancelToken;
use crate::error::SnapshotError;
use crate::parser::{self, ReadOptions};
use crate::snapshot::SnapshotRaw;

const HEADER_PREVIEW_MAX: usize = 50;
const V8_HEAP_SNAPSHOT_STRING_LIMIT_DOC_URL: &str =
    "https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/flags/flag-definitions.h#3098";

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub file: PathBuf,
    pub bind: String,
    pub port: u16,
    pub progress: bool,
    pub cancel: CancelToken,
}

pub fn run(options: ServeOptions) -> Result<(), SnapshotError> {
    let snapshot = parser::read_snapshot_file(
        &options.file,
        ReadOptions::new(options.progress, options.cancel.clone()),
    )?;
    let context = Arc::new(ServerContext { snapshot });
    let addr = format!("{}:{}", options.bind, options.port);
    let listener = TcpListener::bind(&addr).map_err(SnapshotError::Io)?;
    listener.set_nonblocking(true).map_err(SnapshotError::Io)?;
    eprintln!("serve listening on http://{addr}");

    while !options.cancel.is_cancelled() {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(err) = handle_connection(&mut stream, &context) {
                    let _ = write_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        format!("internal server error: {err}").as_bytes(),
                    );
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(err) => return Err(SnapshotError::Io(err)),
        }
    }
    Ok(())
}

struct ServerContext {
    snapshot: SnapshotRaw,
}

fn handle_connection(
    stream: &mut std::net::TcpStream,
    context: &Arc<ServerContext>,
) -> Result<(), SnapshotError> {
    let mut buffer = [0u8; 8192];
    let read = stream.read(&mut buffer).map_err(SnapshotError::Io)?;
    if read == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..read]);
    let first_line = request.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" {
        return write_response(
            stream,
            405,
            "text/plain; charset=utf-8",
            b"method not allowed",
        );
    }

    let (path, query_raw) = split_target(target);
    let query = parse_query(query_raw);
    let response = route(path, &query, context)?;
    write_response(
        stream,
        response.status,
        "text/html; charset=utf-8",
        response.body.as_bytes(),
    )
}

fn route(
    path: &str,
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<HttpResponse, SnapshotError> {
    match path {
        "/" => Ok(HttpResponse::ok(render_index())),
        "/summary" => Ok(HttpResponse::ok(render_summary(query, context)?)),
        "/detail" => Ok(HttpResponse::ok(render_detail(query, context)?)),
        "/retainers" => Ok(HttpResponse::ok(render_retainers(query, context)?)),
        "/diff" => Ok(HttpResponse::ok(render_diff(query)?)),
        "/dominator" => Ok(HttpResponse::ok(render_dominator(query, context)?)),
        _ => Ok(HttpResponse::not_found(render_not_found(path))),
    }
}

struct HttpResponse {
    status: u16,
    body: String,
}

impl HttpResponse {
    fn ok(body: String) -> Self {
        Self { status: 200, body }
    }

    fn not_found(body: String) -> Self {
        Self { status: 404, body }
    }
}

fn render_index() -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>heapsnap serve</title><style>{}</style></head><body>",
        base_styles()
    );
    let _ = writeln!(out, "<h1>heapsnap serve</h1>");
    let _ = writeln!(out, "<ul>");
    let _ = writeln!(out, "<li><a href=\"/summary\">Summary</a></li>");
    let _ = writeln!(
        out,
        "<li><a href=\"/detail?name=Object\">Detail by name example</a></li>"
    );
    let _ = writeln!(
        out,
        "<li><a href=\"/retainers?id=1\">Retainers by id example</a></li>"
    );
    let _ = writeln!(
        out,
        "<li><a href=\"/dominator?id=1\">Dominator by id example</a></li>"
    );
    let _ = writeln!(out, "</ul></body></html>");
    out
}

fn render_summary(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<String, SnapshotError> {
    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 50);
    let top = query_usize(query, "top", 50);
    let search = query.get("search").cloned();
    let scan_top = std::cmp::max(top, skip.saturating_add(limit));
    let result = analysis::summary::summarize(
        &context.snapshot,
        analysis::summary::SummaryOptions {
            top: scan_top,
            contains: search.clone(),
        },
    )?;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Summary</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(
        out,
        "<h1>Summary</h1><p><strong>Total nodes:</strong> {}</p><p><strong>Rows:</strong> showing {}..{} (max {})</p>",
        result.total_nodes,
        skip,
        skip + std::cmp::min(limit, result.rows.len().saturating_sub(skip)),
        result.rows.len()
    );
    write_summary_controls(&mut out, top, search.as_deref(), skip, limit);
    let _ = writeln!(
        out,
        "<table><thead><tr><th>Constructor</th><th>Count</th><th>Self Size Sum (bytes)</th></tr></thead><tbody>"
    );
    for row in result.rows.iter().skip(skip).take(limit) {
        let name = if row.name.is_empty() {
            "(empty)".to_string()
        } else {
            row.name.clone()
        };
        let link = format!("/detail?name={}", url_encode(&name));
        let _ = writeln!(
            out,
            "<tr><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td></tr>",
            link,
            escape_html(&name),
            row.count,
            row.self_size_sum
        );
    }
    let _ = writeln!(out, "</tbody></table></body></html>");
    Ok(out)
}

fn render_detail(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<String, SnapshotError> {
    let id = query_u64_opt(query, "id");
    let name = query.get("name").cloned();
    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 200);
    let detail = analysis::detail::detail(
        &context.snapshot,
        analysis::detail::DetailOptions {
            id,
            name,
            skip,
            limit,
            top_retainers: query_usize(query, "top_retainers", 10),
            top_edges: query_usize(query, "top_edges", 10),
        },
    )?;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Detail</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    match detail {
        analysis::detail::DetailResult::ByName(ref data) => {
            write_detail_header(&mut out, &data.name, None);
            write_detail_controls(&mut out, Some(data.name.as_str()), None, skip, limit);
            let _ = writeln!(
                out,
                "<p>Count={} SelfSizeSum={} Avg={:.2}</p>",
                data.total_count, data.self_size_sum, data.avg_self_size
            );
            let _ = writeln!(
                out,
                "<table><thead><tr><th>Index</th><th>ID</th><th>Type</th><th>Self Size</th></tr></thead><tbody>"
            );
            for item in &data.ids {
                let id_value = item.id.unwrap_or(-1);
                let link = format!("/detail?id={id_value}");
                let _ = writeln!(
                    out,
                    "<tr><td>{}</td><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td></tr>",
                    item.index,
                    link,
                    id_value,
                    escape_html(item.node_type.as_deref().unwrap_or("")),
                    item.self_size
                );
            }
            let _ = writeln!(out, "</tbody></table>");
        }
        analysis::detail::DetailResult::ById(ref data) => {
            write_detail_header(&mut out, &data.name, Some(data.id));
            write_detail_controls(&mut out, None, Some(data.id), skip, limit);
            let _ = writeln!(
                out,
                "<p>Type={} SelfSize={} Count={} SelfSizeSum={} Avg={:.2}</p>",
                escape_html(data.node_type.as_deref().unwrap_or("")),
                data.self_size,
                data.total_count,
                data.self_size_sum,
                data.avg_self_size
            );
            let _ = writeln!(
                out,
                "<h2>Top Retainers</h2><table><thead><tr><th>From Name</th><th>From ID</th><th>From Size</th><th>Edge</th></tr></thead><tbody>"
            );
            for item in &data.retainers {
                let detail_link = item
                    .from_id
                    .map(|idv| format!("<a href=\"/detail?id={idv}\">{idv}</a>"))
                    .unwrap_or_else(|| "-".to_string());
                let name_link = item
                    .from_name
                    .as_deref()
                    .map(|n| {
                        format!(
                            "<a href=\"/detail?name={}\">{}</a>",
                            url_encode(n),
                            escape_html(n)
                        )
                    })
                    .unwrap_or_else(|| "-".to_string());
                let _ = writeln!(
                    out,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}:{}</td></tr>",
                    name_link,
                    detail_link,
                    item.from_self_size,
                    escape_html(item.edge_type.as_deref().unwrap_or("")),
                    escape_html(item.edge_name.as_deref().unwrap_or(""))
                );
            }
            let _ = writeln!(out, "</tbody></table>");
            let _ = writeln!(
                out,
                "<h2>Top Outgoing Edges</h2><table><thead><tr><th>To Name</th><th>To ID</th><th>To Size</th><th>Edge</th></tr></thead><tbody>"
            );
            for item in &data.outgoing_edges {
                let detail_link = item
                    .to_id
                    .map(|idv| format!("<a href=\"/detail?id={idv}\">{idv}</a>"))
                    .unwrap_or_else(|| "-".to_string());
                let name_link = item
                    .to_name
                    .as_deref()
                    .map(|n| {
                        format!(
                            "<a href=\"/detail?name={}\">{}</a>",
                            url_encode(n),
                            escape_html(n)
                        )
                    })
                    .unwrap_or_else(|| "-".to_string());
                let _ = writeln!(
                    out,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}:{}</td></tr>",
                    name_link,
                    detail_link,
                    item.to_self_size,
                    escape_html(item.edge_type.as_deref().unwrap_or("")),
                    escape_html(item.edge_name.as_deref().unwrap_or(""))
                );
            }
            let _ = writeln!(out, "</tbody></table>");
        }
    }
    let _ = writeln!(out, "</body></html>");
    Ok(out)
}

fn write_detail_header(out: &mut String, name: &str, id: Option<u64>) {
    let compact = normalize_header_name(name);
    let len = compact.chars().count();
    let preview = truncate_chars(&compact, HEADER_PREVIEW_MAX);
    let truncated = len > HEADER_PREVIEW_MAX;
    let suffix = if truncated { "..." } else { "" };

    if let Some(id) = id {
        let name_link = format!("/detail?name={}", url_encode(&compact));
        let _ = writeln!(
            out,
            "<h1>Detail: <a href=\"{}\">{}{}</a> (id={})</h1>",
            name_link,
            escape_html(&preview),
            suffix,
            id
        );
    } else {
        let _ = writeln!(out, "<h1>Detail: {}{}</h1>", escape_html(&preview), suffix);
    }

    if truncated {
        let _ = writeln!(out, "<p><strong>Constructor chars:</strong> {}</p>", len);
        write_constructor_limit_note(out, len);
        let _ = writeln!(
            out,
            "<details><summary>Full constructor name</summary><div>{}</div></details>",
            escape_html(&compact)
        );
    }
}

fn write_constructor_limit_note(out: &mut String, constructor_chars: usize) {
    if constructor_chars == 1024 {
        let _ = writeln!(
            out,
            "<p><strong>Note:</strong> <code>Constructor chars</code> が 1024 の場合、snapshot 生成時の V8 flag <code>heap_snapshot_string_limit</code> により切り詰められている可能性があります: <a href=\"{0}\">{0}</a></p>",
            V8_HEAP_SNAPSHOT_STRING_LIMIT_DOC_URL
        );
    }
}

fn render_retainers(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<String, SnapshotError> {
    let id = query_u64(query, "id")?;
    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 5);
    let paths = query_usize(query, "paths", 5);
    let max_depth = query_usize(query, "max_depth", 10);
    let target = analysis::retainers::find_target_by_id(&context.snapshot, id)?;
    let result = analysis::retainers::find_retaining_paths(
        &context.snapshot,
        target,
        analysis::retainers::RetainersOptions {
            max_paths: std::cmp::max(paths, skip.saturating_add(limit)),
            max_depth,
            cancel: CancelToken::new(),
        },
    )?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Retainers</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(out, "<h1>Retainers (id={id})</h1>");
    write_retainers_controls(&mut out, id, paths, max_depth, skip, limit);
    for (index, path) in result.paths.iter().skip(skip).take(limit).enumerate() {
        let _ = writeln!(out, "<h2>Path #{}</h2><ol>", skip + index + 1);
        for step in path {
            let from = context.snapshot.node_view(step.from_node);
            let to = context.snapshot.node_view(step.to_node);
            let from_name = from.and_then(|n| n.name()).unwrap_or("<unknown>");
            let to_name = to.and_then(|n| n.name()).unwrap_or("<unknown>");
            let line = format!(
                "<a href=\"/detail?name={}\">{}</a> -> <a href=\"/detail?name={}\">{}</a>",
                url_encode(from_name),
                escape_html(from_name),
                url_encode(to_name),
                escape_html(to_name)
            );
            let _ = writeln!(out, "<li>{line}</li>");
        }
        let _ = writeln!(out, "</ol>");
    }
    let _ = writeln!(out, "</body></html>");
    Ok(out)
}

fn render_diff(query: &HashMap<String, String>) -> Result<String, SnapshotError> {
    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 50);
    let top = query_usize(query, "top", 50);
    let search = query.get("search").cloned();
    let file_a = query
        .get("file_a")
        .ok_or_else(|| SnapshotError::InvalidData {
            details: "missing file_a query parameter".to_string(),
        })?;
    let file_b = query
        .get("file_b")
        .ok_or_else(|| SnapshotError::InvalidData {
            details: "missing file_b query parameter".to_string(),
        })?;
    let snapshot_a = parser::read_snapshot_file(
        Path::new(file_a),
        ReadOptions::new(false, CancelToken::new()),
    )?;
    let snapshot_b = parser::read_snapshot_file(
        Path::new(file_b),
        ReadOptions::new(false, CancelToken::new()),
    )?;
    let result = analysis::diff::diff_summaries(
        &snapshot_a,
        &snapshot_b,
        analysis::diff::DiffOptions {
            top: std::cmp::max(top, skip.saturating_add(limit)),
            contains: search.clone(),
        },
    )?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Diff</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    write_diff_controls(
        &mut out,
        file_a,
        file_b,
        top,
        search.as_deref(),
        skip,
        limit,
    );
    let _ = writeln!(
        out,
        "<h1>Diff</h1><table><thead><tr><th>Constructor</th><th>Count Δ</th><th>Self Size Δ</th></tr></thead><tbody>"
    );
    for row in result.rows.iter().skip(skip).take(limit) {
        let _ = writeln!(
            out,
            "<tr><td><a href=\"/detail?name={}\">{}</a></td><td>{}</td><td>{}</td></tr>",
            url_encode(&row.name),
            escape_html(&row.name),
            row.count_delta,
            row.self_size_sum_delta
        );
    }
    let _ = writeln!(out, "</tbody></table></body></html>");
    Ok(out)
}

fn render_dominator(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<String, SnapshotError> {
    let id = query_u64(query, "id")?;
    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 50);
    let max_depth = query_usize(query, "max_depth", 50);
    let target = analysis::retainers::find_target_by_id(&context.snapshot, id)?;
    let result = analysis::dominator::dominator_chain(
        &context.snapshot,
        target,
        analysis::dominator::DominatorOptions {
            max_depth,
            cancel: CancelToken::new(),
        },
    )?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Dominator</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(out, "<h1>Dominator (id={id})</h1><ol>");
    write_dominator_controls(&mut out, id, max_depth, skip, limit);
    for node_index in result.chain.iter().skip(skip).take(limit) {
        if let Some(node) = context.snapshot.node_view(*node_index) {
            let name = node.name().unwrap_or("<unknown>");
            let _ = writeln!(
                out,
                "<li><a href=\"/detail?name={}\">{}</a> (id={})</li>",
                url_encode(name),
                escape_html(name),
                node.id().unwrap_or(-1)
            );
        }
    }
    let _ = writeln!(out, "</ol></body></html>");
    Ok(out)
}

fn write_nav(out: &mut String) {
    let _ = writeln!(
        out,
        "<p><a href=\"/\">Home</a> | <a href=\"/summary\">Summary</a></p>"
    );
}

fn write_summary_controls(
    out: &mut String,
    top: usize,
    search: Option<&str>,
    skip: usize,
    limit: usize,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/summary\" class=\"controls\">"
    );
    let _ = writeln!(
        out,
        "<label>Top <input type=\"number\" min=\"1\" name=\"top\" value=\"{}\"></label>",
        top
    );
    let _ = writeln!(
        out,
        "<label>Search <input type=\"text\" name=\"search\" value=\"{}\"></label>",
        escape_html(search.unwrap_or(""))
    );
    write_skip_limit_controls(out, skip, limit);
    let _ = writeln!(out, "<button type=\"submit\">Apply</button></form>");
}

fn normalize_header_name(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn write_retainers_controls(
    out: &mut String,
    id: u64,
    paths: usize,
    max_depth: usize,
    skip: usize,
    limit: usize,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/retainers\" class=\"controls\">"
    );
    let _ = writeln!(out, "<input type=\"hidden\" name=\"id\" value=\"{}\">", id);
    let _ = writeln!(
        out,
        "<label>Paths <input type=\"number\" min=\"1\" name=\"paths\" value=\"{}\"></label>",
        paths
    );
    let _ = writeln!(
        out,
        "<label>Max Depth <input type=\"number\" min=\"1\" name=\"max_depth\" value=\"{}\"></label>",
        max_depth
    );
    write_skip_limit_controls(out, skip, limit);
    let _ = writeln!(out, "<button type=\"submit\">Apply</button></form>");
}

fn write_diff_controls(
    out: &mut String,
    file_a: &str,
    file_b: &str,
    top: usize,
    search: Option<&str>,
    skip: usize,
    limit: usize,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/diff\" class=\"controls\">"
    );
    let _ = writeln!(
        out,
        "<input type=\"hidden\" name=\"file_a\" value=\"{}\">",
        escape_html(file_a)
    );
    let _ = writeln!(
        out,
        "<input type=\"hidden\" name=\"file_b\" value=\"{}\">",
        escape_html(file_b)
    );
    let _ = writeln!(
        out,
        "<label>Top <input type=\"number\" min=\"1\" name=\"top\" value=\"{}\"></label>",
        top
    );
    let _ = writeln!(
        out,
        "<label>Search <input type=\"text\" name=\"search\" value=\"{}\"></label>",
        escape_html(search.unwrap_or(""))
    );
    write_skip_limit_controls(out, skip, limit);
    let _ = writeln!(out, "<button type=\"submit\">Apply</button></form>");
}

fn write_dominator_controls(
    out: &mut String,
    id: u64,
    max_depth: usize,
    skip: usize,
    limit: usize,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/dominator\" class=\"controls\">"
    );
    let _ = writeln!(out, "<input type=\"hidden\" name=\"id\" value=\"{}\">", id);
    let _ = writeln!(
        out,
        "<label>Max Depth <input type=\"number\" min=\"1\" name=\"max_depth\" value=\"{}\"></label>",
        max_depth
    );
    write_skip_limit_controls(out, skip, limit);
    let _ = writeln!(out, "<button type=\"submit\">Apply</button></form>");
}

fn write_skip_limit_controls(out: &mut String, skip: usize, limit: usize) {
    let _ = writeln!(
        out,
        "<label>Skip <input type=\"number\" min=\"0\" name=\"skip\" value=\"{}\"></label>",
        skip
    );
    let _ = writeln!(out, "<label>Limit <select name=\"limit\">");
    for option in [10usize, 25, 50, 100, 200, 500, 1000] {
        let selected = if option == limit { " selected" } else { "" };
        let _ = writeln!(
            out,
            "<option value=\"{}\"{}>{}</option>",
            option, selected, option
        );
    }
    if ![10usize, 25, 50, 100, 200, 500, 1000].contains(&limit) {
        let _ = writeln!(
            out,
            "<option value=\"{}\" selected>{}</option>",
            limit, limit
        );
    }
    let _ = writeln!(out, "</select></label>");
}

fn write_detail_controls(
    out: &mut String,
    name: Option<&str>,
    id: Option<u64>,
    skip: usize,
    limit: usize,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/detail\" class=\"controls\">"
    );
    if let Some(name) = name {
        let _ = writeln!(
            out,
            "<input type=\"hidden\" name=\"name\" value=\"{}\">",
            escape_html(name)
        );
    }
    if let Some(id) = id {
        let _ = writeln!(out, "<input type=\"hidden\" name=\"id\" value=\"{}\">", id);
    }
    let _ = writeln!(
        out,
        "<label>Skip <input type=\"number\" min=\"0\" name=\"skip\" value=\"{}\"></label>",
        skip
    );
    let _ = writeln!(out, "<label>Limit <select name=\"limit\">");
    for option in [10usize, 25, 50, 100, 200, 500, 1000] {
        let selected = if option == limit { " selected" } else { "" };
        let _ = writeln!(
            out,
            "<option value=\"{}\"{}>{}</option>",
            option, selected, option
        );
    }
    if ![10usize, 25, 50, 100, 200, 500, 1000].contains(&limit) {
        let _ = writeln!(
            out,
            "<option value=\"{}\" selected>{}</option>",
            limit, limit
        );
    }
    let _ = writeln!(out, "</select></label>");
    let _ = writeln!(out, "<button type=\"submit\">Apply</button>");
    let _ = writeln!(out, "</form>");
}

fn render_not_found(path: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>{}</style></head><body><h1>404</h1><p>not found: {}</p></body></html>",
        base_styles(),
        escape_html(path)
    )
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

fn parse_query(query_raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query_raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        map.insert(url_decode(key), url_decode(value));
    }
    map
}

fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &value[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(' ');
        } else {
            out.push(bytes[i] as char);
        }
        i += 1;
    }
    out
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
            out.push(ch);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

fn query_usize(query: &HashMap<String, String>, key: &str, default: usize) -> usize {
    query
        .get(key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn query_u64(query: &HashMap<String, String>, key: &str) -> Result<u64, SnapshotError> {
    query
        .get(key)
        .ok_or_else(|| SnapshotError::InvalidData {
            details: format!("missing {key} query parameter"),
        })
        .and_then(|value| {
            value
                .parse::<u64>()
                .map_err(|_| SnapshotError::InvalidData {
                    details: format!("invalid {key} query parameter: {value}"),
                })
        })
}

fn query_u64_opt(query: &HashMap<String, String>, key: &str) -> Option<u64> {
    query.get(key).and_then(|v| v.parse::<u64>().ok())
}

fn write_response(
    stream: &mut std::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), SnapshotError> {
    let status_text = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .map_err(SnapshotError::Io)?;
    stream.write_all(body).map_err(SnapshotError::Io)?;
    stream.flush().map_err(SnapshotError::Io)?;
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn base_styles() -> &'static str {
    "body{font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;width:100%;margin-top:12px}th,td{border:1px solid #ddd;padding:8px;vertical-align:top}th{text-align:left;background:#f6f6f6}tr:nth-child(even){background:#fafafa}a{color:#0b5fff;text-decoration:none}a:hover{text-decoration:underline}.controls{display:flex;gap:12px;align-items:end;margin:12px 0}.controls label{display:flex;gap:6px;align-items:center}"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::parser::{self, ReadOptions};

    #[test]
    fn parse_query_decodes_values() {
        let q = parse_query("name=Foo%20Bar&id=123");
        assert_eq!(q.get("name").map(String::as_str), Some("Foo Bar"));
        assert_eq!(q.get("id").map(String::as_str), Some("123"));
    }

    #[test]
    fn split_target_handles_query() {
        let (path, query) = split_target("/detail?id=1");
        assert_eq!(path, "/detail");
        assert_eq!(query, "id=1");
    }

    #[test]
    fn major_routes_return_200() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = ServerContext { snapshot };

        let res = route("/summary", &HashMap::new(), &context).expect("summary");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("<table>"));

        let mut detail_query = HashMap::new();
        detail_query.insert("name".to_string(), "Node1".to_string());
        let res = route("/detail", &detail_query, &context).expect("detail");
        assert_eq!(res.status, 200);

        let mut ret_query = HashMap::new();
        ret_query.insert("id".to_string(), "3".to_string());
        let res = route("/retainers", &ret_query, &context).expect("retainers");
        assert_eq!(res.status, 200);

        let mut dom_query = HashMap::new();
        dom_query.insert("id".to_string(), "3".to_string());
        let res = route("/dominator", &dom_query, &context).expect("dominator");
        assert_eq!(res.status, 200);

        let mut diff_query = HashMap::new();
        diff_query.insert(
            "file_a".to_string(),
            "fixtures/small.heapsnapshot".to_string(),
        );
        diff_query.insert(
            "file_b".to_string(),
            "fixtures/small.heapsnapshot".to_string(),
        );
        let res = route("/diff", &diff_query, &context).expect("diff");
        assert_eq!(res.status, 200);
    }

    #[test]
    fn detail_controls_reflect_query_values() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = ServerContext { snapshot };

        let mut query = HashMap::new();
        query.insert("name".to_string(), "Node1".to_string());
        query.insert("skip".to_string(), "1".to_string());
        query.insert("limit".to_string(), "50".to_string());
        let res = route("/detail", &query, &context).expect("detail");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("name=\"skip\" value=\"1\""));
        assert!(
            res.body
                .contains("<option value=\"50\" selected>50</option>")
        );
    }

    #[test]
    fn summary_controls_reflect_query_values() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = ServerContext { snapshot };

        let mut query = HashMap::new();
        query.insert("top".to_string(), "99".to_string());
        query.insert("search".to_string(), "Node".to_string());
        query.insert("skip".to_string(), "2".to_string());
        query.insert("limit".to_string(), "25".to_string());
        let res = route("/summary", &query, &context).expect("summary");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("name=\"top\" value=\"99\""));
        assert!(res.body.contains("name=\"search\" value=\"Node\""));
        assert!(res.body.contains("name=\"skip\" value=\"2\""));
        assert!(
            res.body
                .contains("<option value=\"25\" selected>25</option>")
        );
    }
}
