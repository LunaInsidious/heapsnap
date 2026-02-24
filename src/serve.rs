use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::analysis;
use crate::cancel::CancelToken;
use crate::error::SnapshotError;
use crate::parser::{self, ReadOptions};
use crate::snapshot::SnapshotRaw;

const HEADER_PREVIEW_MAX: usize = 50;
const MAX_REQUEST_HEAD_BYTES: usize = 64 * 1024;
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
    let snapshot = Arc::new(parser::read_snapshot_file(
        &options.file,
        ReadOptions::new(options.progress, options.cancel.clone()),
    )?);
    let id_index = build_id_index(&snapshot);
    let context = Arc::new(ServerContext {
        snapshot,
        before_path: options.file,
        cancel: options.cancel.clone(),
        id_index,
        dominator_jobs: Arc::new(Mutex::new(HashMap::new())),
        dominator_session_active: Arc::new(Mutex::new(HashMap::new())),
        dominator_index_cache: Arc::new(Mutex::new(None)),
        uploaded_temp_files: Arc::new(Mutex::new(Vec::new())),
        uploaded_display_names: Arc::new(Mutex::new(HashMap::new())),
        snapshot_cache: Arc::new(Mutex::new(HashMap::new())),
        diff_cache: Arc::new(Mutex::new(HashMap::new())),
    });
    let (listener, selected_port) = bind_listener_with_retry(&options.bind, options.port)?;
    let addr = format!("{}:{}", options.bind, selected_port);
    listener.set_nonblocking(true).map_err(SnapshotError::Io)?;
    eprintln!("serve listening on http://{addr}");

    while !options.cancel.is_cancelled() {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let context = Arc::clone(&context);
                std::thread::spawn(move || {
                    if let Err(err) = handle_connection(&mut stream, &context) {
                        if matches!(err, SnapshotError::Cancelled) {
                            return;
                        }
                        let _ = write_response(
                            &mut stream,
                            500,
                            "text/plain; charset=utf-8",
                            format!("internal server error: {err}").as_bytes(),
                        );
                    }
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(err) => return Err(SnapshotError::Io(err)),
        }
    }
    cleanup_uploaded_temp_files(&context);
    Ok(())
}

fn cleanup_uploaded_temp_files(context: &ServerContext) {
    let paths = {
        let mut guard = match context.uploaded_temp_files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *guard)
    };
    for path in paths {
        if let Err(err) = fs::remove_file(&path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("failed to remove temp upload {}: {err}", path.display());
            }
        }
    }
}

fn build_id_index(snapshot: &SnapshotRaw) -> HashMap<u64, usize> {
    let mut index = HashMap::new();
    for node_index in 0..snapshot.node_count() {
        let node = match snapshot.node_view(node_index) {
            Some(node) => node,
            None => continue,
        };
        let node_id = match node.id() {
            Some(value) if value >= 0 => value as u64,
            _ => continue,
        };
        index.insert(node_id, node_index);
    }
    index
}

fn bind_listener_with_retry(
    bind: &str,
    start_port: u16,
) -> Result<(TcpListener, u16), SnapshotError> {
    bind_with_retry(start_port, |port| {
        let addr = format!("{bind}:{port}");
        TcpListener::bind(&addr)
    })
}

fn bind_with_retry<T, F>(start_port: u16, mut bind_port: F) -> Result<(T, u16), SnapshotError>
where
    F: FnMut(u16) -> Result<T, std::io::Error>,
{
    let mut port = start_port;
    loop {
        match bind_port(port) {
            Ok(value) => return Ok((value, port)),
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                if port == u16::MAX {
                    return Err(SnapshotError::InvalidData {
                        details: "no available port: reached 65535 while retrying".to_string(),
                    });
                }
                let next_port = port + 1;
                eprintln!("port {port} is in use, retrying with {next_port}");
                port = next_port;
            }
            Err(err) => return Err(SnapshotError::Io(err)),
        }
    }
}

struct ServerContext {
    snapshot: Arc<SnapshotRaw>,
    before_path: PathBuf,
    cancel: CancelToken,
    id_index: HashMap<u64, usize>,
    dominator_jobs: Arc<Mutex<HashMap<DominatorJobKey, Arc<Mutex<DominatorJob>>>>>,
    dominator_session_active: Arc<Mutex<HashMap<String, DominatorJobKey>>>,
    dominator_index_cache: Arc<Mutex<Option<analysis::dominator::DominatorIndex>>>,
    uploaded_temp_files: Arc<Mutex<Vec<PathBuf>>>,
    uploaded_display_names: Arc<Mutex<HashMap<PathBuf, String>>>,
    snapshot_cache: Arc<Mutex<HashMap<PathBuf, Arc<SnapshotRaw>>>>,
    diff_cache: Arc<Mutex<HashMap<DiffCacheKey, Arc<analysis::diff::DiffResult>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DominatorJobKey {
    session: String,
    target: usize,
    max_depth: usize,
}

#[derive(Debug, Clone)]
struct DominatorJob {
    cancel: CancelToken,
    status: DominatorProgressView,
    result: Option<analysis::dominator::DominatorResult>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct DominatorProgressView {
    phase: String,
    percent: u8,
    completed: u64,
    total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiffCacheKey {
    before: PathBuf,
    after: PathBuf,
    top: usize,
    search: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SizeUnit {
    Bytes,
    KiB,
    MiB,
    GiB,
}

impl SizeUnit {
    fn from_query(query: &HashMap<String, String>) -> Self {
        Self::from_value(query.get("size_unit").map(String::as_str))
    }

    fn from_value(value: Option<&str>) -> Self {
        match value.unwrap_or_default().to_ascii_lowercase().as_str() {
            "kib" => Self::KiB,
            "mib" => Self::MiB,
            "gib" => Self::GiB,
            _ => Self::Bytes,
        }
    }

    fn as_query_value(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::KiB => "kib",
            Self::MiB => "mib",
            Self::GiB => "gib",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::KiB => "KiB",
            Self::MiB => "MiB",
            Self::GiB => "GiB",
        }
    }

    fn factor(self) -> f64 {
        match self {
            Self::Bytes => 1.0,
            Self::KiB => 1024.0,
            Self::MiB => 1024.0 * 1024.0,
            Self::GiB => 1024.0 * 1024.0 * 1024.0,
        }
    }

    fn format_i64(self, value: i64) -> String {
        match self {
            Self::Bytes => value.to_string(),
            _ => format!("{:.2}", value as f64 / self.factor()),
        }
    }
}

fn handle_connection(
    stream: &mut std::net::TcpStream,
    context: &Arc<ServerContext>,
) -> Result<(), SnapshotError> {
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(SnapshotError::Io)?;
    let request = match read_http_request(stream, &context.cancel)? {
        Some(request) => request,
        None => return Ok(()),
    };
    if request.method != "GET" && request.method != "POST" {
        return write_response(
            stream,
            405,
            "text/plain; charset=utf-8",
            b"method not allowed",
        );
    }

    let (path, query_raw) = split_target(&request.target);
    let query = parse_query(query_raw);
    if request.method == "GET" && path == "/dominator/events" {
        return write_dominator_events(stream, &query, context);
    }
    let response = route(
        &request.method,
        path,
        &query,
        &request.headers,
        &request.body,
        context,
    )?;
    write_response(
        stream,
        response.status,
        "text/html; charset=utf-8",
        response.body.as_bytes(),
    )
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_http_request(
    stream: &mut std::net::TcpStream,
    cancel: &CancelToken,
) -> Result<Option<HttpRequest>, SnapshotError> {
    let mut raw = Vec::with_capacity(8192);
    let header_end = loop {
        if let Some(idx) = find_subslice(&raw, b"\r\n\r\n") {
            break idx;
        }
        if raw.len() > MAX_REQUEST_HEAD_BYTES {
            return Err(SnapshotError::InvalidData {
                details: "HTTP request header too large".to_string(),
            });
        }

        let mut chunk = [0u8; 8192];
        let read = match stream.read(&mut chunk) {
            Ok(read) => read,
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                if cancel.is_cancelled() {
                    return Err(SnapshotError::Cancelled);
                }
                continue;
            }
            Err(err) => return Err(SnapshotError::Io(err)),
        };
        if read == 0 {
            if raw.is_empty() {
                return Ok(None);
            }
            return Err(SnapshotError::InvalidData {
                details: "unexpected EOF while reading HTTP request header".to_string(),
            });
        }
        raw.extend_from_slice(&chunk[..read]);
    };

    let head_raw = &raw[..header_end];
    let head_text = String::from_utf8_lossy(head_raw);
    let mut lines = head_text.lines();
    let request_line = lines.next().ok_or_else(|| SnapshotError::InvalidData {
        details: "missing HTTP request line".to_string(),
    })?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or("").to_string();
    let target = request_parts.next().unwrap_or("/").to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;

    while raw.len().saturating_sub(body_start) < content_length {
        let mut chunk = [0u8; 8192];
        let read = match stream.read(&mut chunk) {
            Ok(read) => read,
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                if cancel.is_cancelled() {
                    return Err(SnapshotError::Cancelled);
                }
                continue;
            }
            Err(err) => return Err(SnapshotError::Io(err)),
        };
        if read == 0 {
            return Err(SnapshotError::InvalidData {
                details: "unexpected EOF while reading HTTP request body".to_string(),
            });
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let body_end = body_start + content_length;
    let body = raw.get(body_start..body_end).unwrap_or_default().to_vec();

    Ok(Some(HttpRequest {
        method,
        target,
        headers,
        body,
    }))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn route(
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    body: &[u8],
    context: &ServerContext,
) -> Result<HttpResponse, SnapshotError> {
    match (method, path) {
        ("GET", "/") => Ok(HttpResponse::ok(render_index())),
        ("GET", "/summary") => Ok(HttpResponse::ok(render_summary(query, context)?)),
        ("GET", "/detail") => Ok(HttpResponse::ok(render_detail(query, context)?)),
        ("GET", "/retainers") => Ok(HttpResponse::ok(render_retainers(query, context)?)),
        ("GET", "/diff") => Ok(HttpResponse::ok(render_diff(query, context)?)),
        ("POST", "/diff") => render_diff_post(headers, body, context),
        ("GET", "/dominator") => Ok(HttpResponse::ok(render_dominator(query, context)?)),
        _ => Ok(HttpResponse::not_found(render_not_found(path))),
    }
}

fn render_diff_post(
    headers: &HashMap<String, String>,
    body: &[u8],
    context: &ServerContext,
) -> Result<HttpResponse, SnapshotError> {
    let form = match parse_multipart_form(headers, body) {
        Ok(form) => form,
        Err(err) => {
            return Ok(HttpResponse::bad_request(render_diff_upload(
                context,
                Some(&err.to_string()),
            )));
        }
    };
    let uploaded = match form.file {
        Some(file) => file,
        None => {
            return Ok(HttpResponse::bad_request(render_diff_upload(
                context,
                Some("invalid data: missing `after` file field"),
            )));
        }
    };
    if uploaded.content.is_empty() {
        return Ok(HttpResponse::bad_request(render_diff_upload(
            context,
            Some("invalid data: empty upload. Select a .heapsnapshot file and retry"),
        )));
    }

    let temp_path = write_uploaded_after_snapshot(&uploaded)?;
    {
        let mut guard = match context.uploaded_temp_files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !guard.iter().any(|path| path == &temp_path) {
            guard.push(temp_path.clone());
        }
    }
    {
        let display_name = uploaded
            .filename
            .as_deref()
            .and_then(|name| Path::new(name).file_name().and_then(|n| n.to_str()))
            .map(ToString::to_string)
            .unwrap_or_else(|| {
                temp_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("uploaded.heapsnapshot")
                    .to_string()
            });
        let mut guard = match context.uploaded_display_names.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(temp_path.clone(), display_name);
    }
    let mut query = HashMap::new();
    query.insert(
        "before".to_string(),
        context.before_path.display().to_string(),
    );
    query.insert("after".to_string(), temp_path.display().to_string());
    if let Some(value) = form.fields.get("top") {
        query.insert("top".to_string(), value.clone());
    }
    if let Some(value) = form.fields.get("search") {
        query.insert("search".to_string(), value.clone());
    }
    if let Some(value) = form.fields.get("skip") {
        query.insert("skip".to_string(), value.clone());
    }
    if let Some(value) = form.fields.get("limit") {
        query.insert("limit".to_string(), value.clone());
    }
    if let Some(value) = form.fields.get("size_unit") {
        query.insert("size_unit".to_string(), value.clone());
    }
    Ok(HttpResponse::ok(render_diff(&query, context)?))
}

struct HttpResponse {
    status: u16,
    body: String,
}

impl HttpResponse {
    fn ok(body: String) -> Self {
        Self { status: 200, body }
    }

    fn bad_request(body: String) -> Self {
        Self { status: 400, body }
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
    let _ = writeln!(out, "<li><a href=\"/diff\">Diff (upload file)</a></li>");
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
    let size_unit = SizeUnit::from_query(query);
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
    write_summary_controls(&mut out, top, search.as_deref(), skip, limit, size_unit);
    let _ = writeln!(
        out,
        "<table class=\"resizable-table\"><thead><tr><th>Constructor</th><th>Count</th><th>Self Size Sum ({})</th></tr></thead><tbody>",
        size_unit.label()
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
            size_unit.format_i64(row.self_size_sum)
        );
    }
    let _ = writeln!(out, "</tbody></table>");
    let _ = writeln!(out, "<script>{}</script>", table_column_resize_script());
    let _ = writeln!(out, "</body></html>");
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
    let size_unit = SizeUnit::from_query(query);
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
            write_detail_controls(
                &mut out,
                Some(data.name.as_str()),
                None,
                skip,
                limit,
                size_unit,
            );
            let _ = writeln!(
                out,
                "<p>Count={} SelfSizeSum({})={} Avg({})={:.2}</p>",
                data.total_count,
                size_unit.label(),
                size_unit.format_i64(data.self_size_sum),
                size_unit.label(),
                data.avg_self_size / size_unit.factor()
            );
            let _ = writeln!(
                out,
                "<table class=\"resizable-table\"><thead><tr><th>Index</th><th>ID</th><th>Type</th><th>Self Size ({})</th></tr></thead><tbody>",
                size_unit.label()
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
                    size_unit.format_i64(item.self_size)
                );
            }
            let _ = writeln!(out, "</tbody></table>");
        }
        analysis::detail::DetailResult::ById(ref data) => {
            write_detail_header(&mut out, &data.name, Some(data.id));
            write_detail_controls(&mut out, None, Some(data.id), skip, limit, size_unit);
            let _ = writeln!(
                out,
                "<p>Type={} SelfSize({})={} Count={} SelfSizeSum({})={} Avg({})={:.2}</p>",
                escape_html(data.node_type.as_deref().unwrap_or("")),
                size_unit.label(),
                size_unit.format_i64(data.self_size),
                data.total_count,
                size_unit.label(),
                size_unit.format_i64(data.self_size_sum),
                size_unit.label(),
                data.avg_self_size / size_unit.factor()
            );
            let _ = writeln!(
                out,
                "<h2>Top Retainers</h2><table class=\"resizable-table\"><thead><tr><th>From Name</th><th>From ID</th><th>From Size ({})</th><th>Edge</th></tr></thead><tbody>",
                size_unit.label()
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
                    size_unit.format_i64(item.from_self_size),
                    escape_html(item.edge_type.as_deref().unwrap_or("")),
                    escape_html(item.edge_name.as_deref().unwrap_or(""))
                );
            }
            let _ = writeln!(out, "</tbody></table>");
            let _ = writeln!(
                out,
                "<h2>Top Outgoing Edges</h2><table class=\"resizable-table\"><thead><tr><th>To Name</th><th>To ID</th><th>To Size ({})</th><th>Edge</th></tr></thead><tbody>",
                size_unit.label()
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
                    size_unit.format_i64(item.to_self_size),
                    escape_html(item.edge_type.as_deref().unwrap_or("")),
                    escape_html(item.edge_name.as_deref().unwrap_or(""))
                );
            }
            let _ = writeln!(out, "</tbody></table>");
        }
    }
    let _ = writeln!(out, "<script>{}</script>", table_column_resize_script());
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
            cancel: context.cancel.clone(),
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

fn render_diff(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<String, SnapshotError> {
    if !query.contains_key("before") && !query.contains_key("after") {
        return Ok(render_diff_upload(context, None));
    }

    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 50);
    let top = query_usize(query, "top", 50);
    let search = query.get("search").cloned();
    let size_unit = SizeUnit::from_query(query);
    let scan_top = std::cmp::max(top, skip.saturating_add(limit));
    let before = query
        .get("before")
        .ok_or_else(|| SnapshotError::InvalidData {
            details: "missing before query parameter".to_string(),
        })?;
    let after = query
        .get("after")
        .ok_or_else(|| SnapshotError::InvalidData {
            details: "missing after query parameter".to_string(),
        })?;
    let before_path = PathBuf::from(before);
    let after_path = PathBuf::from(after);
    let cache_key = DiffCacheKey {
        before: before_path.clone(),
        after: after_path.clone(),
        top: scan_top,
        search: search.clone(),
    };
    let result = {
        let guard = match context.diff_cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(&cache_key).cloned()
    };
    let result = match result {
        Some(result) => result,
        None => {
            let snapshot_a = load_snapshot_cached(context, &before_path)?;
            let snapshot_b = load_snapshot_cached(context, &after_path)?;
            let computed = Arc::new(analysis::diff::diff_summaries(
                &snapshot_a,
                &snapshot_b,
                analysis::diff::DiffOptions {
                    top: scan_top,
                    contains: search.clone(),
                },
            )?);
            {
                let mut guard = match context.diff_cache.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.insert(cache_key, Arc::clone(&computed));
            }
            computed
        }
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Diff</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let before_label = display_name_for_path(context, &before_path);
    let after_label = display_name_for_path(context, &after_path);
    let _ = writeln!(
        out,
        "<p><strong>Before:</strong> {} ({})</p><p><strong>After:</strong> {} ({})</p>",
        escape_html(&before_label),
        escape_html(before),
        escape_html(&after_label),
        escape_html(after)
    );
    write_diff_upload_controls(&mut out, top, search.as_deref(), skip, limit, size_unit);
    write_diff_controls(
        &mut out,
        before,
        after,
        top,
        search.as_deref(),
        skip,
        limit,
        size_unit,
    );
    let _ = writeln!(
        out,
        "<h1>Diff</h1><table class=\"resizable-table\"><thead><tr><th>Constructor</th><th>Count Δ</th><th>Self Size Δ ({})</th></tr></thead><tbody>",
        size_unit.label()
    );
    for row in result.rows.iter().skip(skip).take(limit) {
        let _ = writeln!(
            out,
            "<tr><td><a href=\"/detail?name={}\">{}</a></td><td>{}</td><td>{}</td></tr>",
            url_encode(&row.name),
            escape_html(&row.name),
            row.count_delta,
            size_unit.format_i64(row.self_size_sum_delta)
        );
    }
    let _ = writeln!(out, "</tbody></table>");
    let _ = writeln!(out, "<script>{}</script>", table_column_resize_script());
    let _ = writeln!(out, "</body></html>");
    Ok(out)
}

fn display_name_for_path(context: &ServerContext, path: &Path) -> String {
    {
        let guard = match context.uploaded_display_names.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(name) = guard.get(path) {
            return name.clone();
        }
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| path.to_str().unwrap_or("<unknown>"))
        .to_string()
}

fn load_snapshot_cached(
    context: &ServerContext,
    path: &Path,
) -> Result<Arc<SnapshotRaw>, SnapshotError> {
    if path == context.before_path {
        return Ok(Arc::clone(&context.snapshot));
    }
    let path_buf = path.to_path_buf();
    {
        let guard = match context.snapshot_cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(snapshot) = guard.get(&path_buf) {
            return Ok(Arc::clone(snapshot));
        }
    }
    let snapshot = Arc::new(parser::read_snapshot_file(
        path,
        ReadOptions::new(false, context.cancel.clone()),
    )?);
    {
        let mut guard = match context.snapshot_cache.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(path_buf, Arc::clone(&snapshot));
    }
    Ok(snapshot)
}

fn render_diff_upload(context: &ServerContext, error: Option<&str>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Diff upload</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(out, "<h1>Diff</h1>");
    let _ = writeln!(
        out,
        "<p><strong>Before (fixed):</strong> {}</p>",
        escape_html(&context.before_path.display().to_string())
    );
    if let Some(err) = error {
        let _ = writeln!(out, "<p><strong>Error:</strong> {}</p>", escape_html(err));
    }
    write_diff_upload_controls(&mut out, 50, None, 0, 50, SizeUnit::Bytes);
    let _ = writeln!(out, "</body></html>");
    out
}

#[derive(Debug)]
struct UploadedFile {
    filename: Option<String>,
    content: Vec<u8>,
}

#[derive(Debug, Default)]
struct MultipartForm {
    fields: HashMap<String, String>,
    file: Option<UploadedFile>,
}

fn parse_multipart_form(
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Result<MultipartForm, SnapshotError> {
    let content_type = headers
        .get("content-type")
        .ok_or_else(|| SnapshotError::InvalidData {
            details: "missing Content-Type header for multipart upload".to_string(),
        })?;
    let boundary = extract_boundary(content_type)?;
    let boundary_start = format!("--{boundary}").into_bytes();
    let boundary_marker = format!("\r\n--{boundary}").into_bytes();

    if !body.starts_with(&boundary_start) {
        return Err(SnapshotError::InvalidData {
            details: "multipart body does not start with boundary".to_string(),
        });
    }

    let mut cursor = 0usize;
    let mut form = MultipartForm::default();
    loop {
        if !body
            .get(cursor..cursor + boundary_start.len())
            .map(|value| value == boundary_start.as_slice())
            .unwrap_or(false)
        {
            return Err(SnapshotError::InvalidData {
                details: "multipart boundary marker not found".to_string(),
            });
        }
        cursor += boundary_start.len();

        if body
            .get(cursor..cursor + 2)
            .map(|value| value == b"--")
            .unwrap_or(false)
        {
            break;
        }
        if body
            .get(cursor..cursor + 2)
            .map(|value| value != b"\r\n")
            .unwrap_or(true)
        {
            return Err(SnapshotError::InvalidData {
                details: "invalid multipart separator".to_string(),
            });
        }
        cursor += 2;

        let tail = &body[cursor..];
        let part_end_rel =
            find_subslice(tail, &boundary_marker).ok_or_else(|| SnapshotError::InvalidData {
                details: "multipart part boundary not found".to_string(),
            })?;
        let part = &tail[..part_end_rel];
        let (headers_raw, value_raw) =
            split_part_headers_and_body(part).ok_or_else(|| SnapshotError::InvalidData {
                details: "multipart part missing header/body separator".to_string(),
            })?;
        let disposition = parse_part_disposition(headers_raw)?;
        if disposition.filename.is_some() {
            if disposition.name == "after" {
                form.file = Some(UploadedFile {
                    filename: disposition.filename,
                    content: value_raw.to_vec(),
                });
            }
        } else {
            let text = String::from_utf8_lossy(value_raw).trim().to_string();
            form.fields.insert(disposition.name, text);
        }

        cursor += part_end_rel + 2;
    }

    Ok(form)
}

fn extract_boundary(content_type: &str) -> Result<String, SnapshotError> {
    for segment in content_type.split(';').map(str::trim) {
        if let Some(value) = segment.strip_prefix("boundary=") {
            return Ok(value.trim_matches('"').to_string());
        }
    }
    Err(SnapshotError::InvalidData {
        details: "multipart boundary not found in Content-Type header".to_string(),
    })
}

fn split_part_headers_and_body(part: &[u8]) -> Option<(&str, &[u8])> {
    let header_end = find_subslice(part, b"\r\n\r\n")?;
    let headers_raw = std::str::from_utf8(&part[..header_end]).ok()?;
    let body = part.get(header_end + 4..)?;
    Some((headers_raw, body))
}

#[derive(Debug)]
struct PartDisposition {
    name: String,
    filename: Option<String>,
}

fn parse_part_disposition(headers_raw: &str) -> Result<PartDisposition, SnapshotError> {
    let disposition_line = headers_raw
        .lines()
        .find(|line| {
            line.to_ascii_lowercase()
                .starts_with("content-disposition:")
        })
        .ok_or_else(|| SnapshotError::InvalidData {
            details: "multipart part missing Content-Disposition".to_string(),
        })?;
    let (_, params_raw) =
        disposition_line
            .split_once(':')
            .ok_or_else(|| SnapshotError::InvalidData {
                details: "invalid Content-Disposition format".to_string(),
            })?;

    let mut name = None;
    let mut filename = None;
    for param in params_raw.split(';').map(str::trim) {
        if let Some(value) = param.strip_prefix("name=") {
            name = Some(value.trim_matches('"').to_string());
        } else if let Some(value) = param.strip_prefix("filename=") {
            let trimmed = value.trim_matches('"');
            if !trimmed.is_empty() {
                filename = Some(trimmed.to_string());
            }
        }
    }

    let name = name.ok_or_else(|| SnapshotError::InvalidData {
        details: "multipart part missing name in Content-Disposition".to_string(),
    })?;
    Ok(PartDisposition { name, filename })
}

fn write_uploaded_after_snapshot(file: &UploadedFile) -> Result<PathBuf, SnapshotError> {
    let mut dir = std::env::temp_dir();
    dir.push("heapsnap-serve");
    fs::create_dir_all(&dir).map_err(SnapshotError::Io)?;

    let fingerprint = content_fingerprint(&file.content);
    let filename = format!("{fingerprint:016x}-{}.heapsnapshot", file.content.len());

    let mut path = dir;
    path.push(filename);
    if path.exists() {
        return Ok(path);
    }
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(SnapshotError::Io)?;
    output.write_all(&file.content).map_err(SnapshotError::Io)?;
    output.flush().map_err(SnapshotError::Io)?;
    Ok(path)
}

fn content_fingerprint(content: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn render_dominator(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<String, SnapshotError> {
    let (key, id, skip, limit) = dominator_job_from_query(query, context)?;
    let max_depth = key.max_depth;
    let session = key.session.clone();
    let job = get_or_start_dominator_job(context, key);
    let (progress, result, error) = {
        let guard = match job.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        (
            guard.status.clone(),
            guard.result.clone(),
            guard.error.clone(),
        )
    };

    if let Some(reason) = error {
        return Ok(render_dominator_failed(
            id, max_depth, skip, limit, &session, &reason,
        ));
    }
    let result = match result {
        Some(result) => result,
        None => {
            return Ok(render_dominator_loading(
                id, max_depth, skip, limit, &session, &progress,
            ));
        }
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Dominator</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(
        out,
        "<script>if (window.location.search.indexOf('session=') === -1) {{ history.replaceState(null, '', '/dominator?id={}&max_depth={}&skip={}&limit={}&session={}'); }}</script>",
        id,
        max_depth,
        skip,
        limit,
        url_encode(&session)
    );
    let _ = writeln!(out, "<h1>Dominator (id={id})</h1><ol>");
    write_dominator_controls(&mut out, id, max_depth, skip, limit, &session);
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

fn get_or_start_dominator_job(
    context: &ServerContext,
    key: DominatorJobKey,
) -> Arc<Mutex<DominatorJob>> {
    {
        let mut active = match context.dominator_session_active.lock() {
            Ok(active) => active,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(prev_key) = active.get(&key.session).cloned() {
            if prev_key != key {
                let mut jobs = match context.dominator_jobs.lock() {
                    Ok(jobs) => jobs,
                    Err(poisoned) => poisoned.into_inner(),
                };
                if let Some(prev_job) = jobs.get(&prev_key) {
                    let guard = match prev_job.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.cancel.cancel();
                }
                jobs.remove(&prev_key);
            }
        }
        active.insert(key.session.clone(), key.clone());
    }

    {
        let jobs = match context.dominator_jobs.lock() {
            Ok(jobs) => jobs,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(existing) = jobs.get(&key) {
            return Arc::clone(existing);
        }
    }

    let job_cancel = CancelToken::new();
    let job = Arc::new(Mutex::new(DominatorJob {
        cancel: job_cancel.clone(),
        status: DominatorProgressView {
            phase: "queued".to_string(),
            percent: 0,
            completed: 0,
            total: 1,
        },
        result: None,
        error: None,
    }));
    {
        let mut jobs = match context.dominator_jobs.lock() {
            Ok(jobs) => jobs,
            Err(poisoned) => poisoned.into_inner(),
        };
        jobs.insert(key.clone(), Arc::clone(&job));
    }

    let snapshot = Arc::clone(&context.snapshot);
    let context_cancel = context.cancel.clone();
    let index_cache = Arc::clone(&context.dominator_index_cache);
    let job_ref = Arc::clone(&job);
    std::thread::spawn(move || {
        let (progress_tx, progress_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let worker_snapshot = Arc::clone(&snapshot);
        let worker_cancel = job_cancel.clone();
        let worker_cache = Arc::clone(&index_cache);
        std::thread::spawn(move || {
            let maybe_cached = {
                let guard = match worker_cache.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.clone()
            };
            let result = if let Some(index) = maybe_cached {
                analysis::dominator::dominator_chain_from_index(
                    &index,
                    key.target,
                    key.max_depth,
                    worker_cancel,
                )
            } else {
                match analysis::dominator::compute_dominator_index(
                    &worker_snapshot,
                    worker_cancel.clone(),
                    Some(progress_tx),
                ) {
                    Ok(index) => {
                        {
                            let mut guard = match worker_cache.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            *guard = Some(index.clone());
                        }
                        analysis::dominator::dominator_chain_from_index(
                            &index,
                            key.target,
                            key.max_depth,
                            worker_cancel,
                        )
                    }
                    Err(err) => Err(err),
                }
            };
            let _ = result_tx.send(result);
        });

        loop {
            if context_cancel.is_cancelled() || job_cancel.is_cancelled() {
                let mut guard = match job_ref.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.error = Some("cancelled".to_string());
                guard.status = DominatorProgressView {
                    phase: "cancelled".to_string(),
                    percent: 100,
                    completed: 100,
                    total: 100,
                };
                break;
            }
            while let Ok(progress) = progress_rx.try_recv() {
                let view = progress_to_view(&progress);
                let mut guard = match job_ref.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.status = view;
            }
            match result_rx.try_recv() {
                Ok(Ok(result)) => {
                    let mut guard = match job_ref.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.result = Some(result);
                    guard.status = DominatorProgressView {
                        phase: "done".to_string(),
                        percent: 100,
                        completed: 100,
                        total: 100,
                    };
                    break;
                }
                Ok(Err(err)) => {
                    let mut guard = match job_ref.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.error = Some(err.to_string());
                    guard.status = DominatorProgressView {
                        phase: "failed".to_string(),
                        percent: 100,
                        completed: 100,
                        total: 100,
                    };
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let mut guard = match job_ref.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.error = Some("dominator worker disconnected".to_string());
                    guard.status = DominatorProgressView {
                        phase: "failed".to_string(),
                        percent: 100,
                        completed: 100,
                        total: 100,
                    };
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(120)),
            }
        }
    });

    job
}

fn progress_to_view(progress: &analysis::dominator::DominatorProgress) -> DominatorProgressView {
    use analysis::dominator::DominatorPhase;
    let phase_percent = |done: u64, total: u64| -> u8 {
        if total == 0 {
            100
        } else {
            ((done.min(total) as f64 / total as f64) * 100.0) as u8
        }
    };
    match progress.phase {
        DominatorPhase::BuildGraph => DominatorProgressView {
            phase: "build_graph".to_string(),
            percent: phase_percent(progress.edges_done, progress.edges_total),
            completed: progress.edges_done,
            total: std::cmp::max(1, progress.edges_total),
        },
        DominatorPhase::ReversePostorder => DominatorProgressView {
            phase: "reverse_postorder".to_string(),
            percent: phase_percent(progress.nodes_done, progress.nodes_total),
            completed: progress.nodes_done,
            total: std::cmp::max(1, progress.nodes_total),
        },
        DominatorPhase::ComputeIdom => DominatorProgressView {
            phase: format!("compute_idom_iter_{}", progress.idom_iteration + 1),
            percent: phase_percent(progress.nodes_done, progress.nodes_total),
            completed: progress.nodes_done,
            total: std::cmp::max(1, progress.nodes_total),
        },
        DominatorPhase::Done => DominatorProgressView {
            phase: "done".to_string(),
            percent: 100,
            completed: 100,
            total: 100,
        },
    }
}

fn render_dominator_loading(
    id: u64,
    max_depth: usize,
    skip: usize,
    limit: usize,
    session: &str,
    progress: &DominatorProgressView,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Dominator</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(
        out,
        "<script>if (window.location.search.indexOf('session=') === -1) {{ history.replaceState(null, '', '/dominator?id={}&max_depth={}&skip={}&limit={}&session={}'); }}</script>",
        id,
        max_depth,
        skip,
        limit,
        url_encode(session)
    );
    let _ = writeln!(out, "<h1>Dominator (id={id})</h1>");
    write_dominator_controls(&mut out, id, max_depth, skip, limit, session);
    let _ = writeln!(
        out,
        "<p id=\"dom-status\">Calculating dominator chain: {}% ({}/{})</p>",
        progress.percent, progress.completed, progress.total
    );
    let _ = writeln!(
        out,
        "<progress id=\"dom-progress\" max=\"100\" value=\"{}\"></progress>",
        progress.percent
    );
    let _ = writeln!(
        out,
        "<script>{}</script>",
        dominator_sse_script(id, max_depth, skip, limit, session)
    );
    let _ = writeln!(out, "</body></html>");
    out
}

fn render_dominator_failed(
    id: u64,
    max_depth: usize,
    skip: usize,
    limit: usize,
    session: &str,
    reason: &str,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Dominator</title><style>{}</style></head><body>",
        base_styles()
    );
    write_nav(&mut out);
    let _ = writeln!(
        out,
        "<script>if (window.location.search.indexOf('session=') === -1) {{ history.replaceState(null, '', '/dominator?id={}&max_depth={}&skip={}&limit={}&session={}'); }}</script>",
        id,
        max_depth,
        skip,
        limit,
        url_encode(session)
    );
    let _ = writeln!(out, "<h1>Dominator (id={id})</h1>");
    write_dominator_controls(&mut out, id, max_depth, skip, limit, session);
    let _ = writeln!(
        out,
        "<p><strong>Calculation failed:</strong> {}</p>",
        escape_html(reason)
    );
    let _ = writeln!(out, "</body></html>");
    out
}

fn dominator_sse_script(
    id: u64,
    max_depth: usize,
    skip: usize,
    limit: usize,
    session: &str,
) -> String {
    let mut url = String::from("/dominator/events?");
    url.push_str(&format!(
        "id={}&max_depth={}&skip={}&limit={}&session={}",
        id,
        max_depth,
        skip,
        limit,
        url_encode(session)
    ));
    format!(
        "(() => {{
  const status = document.getElementById('dom-status');
  const progress = document.getElementById('dom-progress');
  if (!window.EventSource || !status || !progress) return;
  const source = new EventSource('{url}');
  source.addEventListener('progress', (ev) => {{
    const parts = ev.data.split('|');
    if (parts.length < 4) return;
    const phase = parts[0];
    const percent = Number(parts[1]) || 0;
    const completed = parts[2];
    const total = parts[3];
    progress.value = percent;
    status.textContent = 'Calculating dominator chain (' + phase + '): ' + percent + '% (' + completed + '/' + total + ')';
  }});
  source.addEventListener('done', () => {{
    source.close();
    location.reload();
  }});
  source.addEventListener('failed', (ev) => {{
    source.close();
    status.textContent = `Calculation failed: ${{ev.data}}`;
  }});
}})();"
    )
}

fn write_dominator_events(
    stream: &mut std::net::TcpStream,
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<(), SnapshotError> {
    let (key, _, _, _) = dominator_job_from_query(query, context)?;
    let job = get_or_start_dominator_job(context, key);
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream
        .write_all(header.as_bytes())
        .map_err(SnapshotError::Io)?;
    stream.flush().map_err(SnapshotError::Io)?;

    loop {
        if context.cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }
        let (status, has_result, error) = {
            let guard = match job.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            (
                guard.status.clone(),
                guard.result.is_some(),
                guard.error.clone(),
            )
        };

        let frame = format!(
            "event: progress\ndata: {}|{}|{}|{}\n\n",
            status.phase, status.percent, status.completed, status.total
        );
        if let Err(err) = stream.write_all(frame.as_bytes()) {
            if err.kind() == std::io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(SnapshotError::Io(err));
        }
        stream.flush().map_err(SnapshotError::Io)?;

        if has_result {
            let _ = stream.write_all(b"event: done\ndata: ok\n\n");
            let _ = stream.flush();
            return Ok(());
        }
        if let Some(err_msg) = error {
            let frame = format!("event: failed\ndata: {}\n\n", err_msg.replace('\n', " "));
            let _ = stream.write_all(frame.as_bytes());
            let _ = stream.flush();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn dominator_job_from_query(
    query: &HashMap<String, String>,
    context: &ServerContext,
) -> Result<(DominatorJobKey, u64, usize, usize), SnapshotError> {
    let id = query_u64(query, "id")?;
    let skip = query_usize(query, "skip", 0);
    let limit = query_usize(query, "limit", 50);
    let max_depth = query_usize(query, "max_depth", 50);
    let session = query
        .get("session")
        .cloned()
        .unwrap_or_else(generate_session_token);
    let target = context
        .id_index
        .get(&id)
        .copied()
        .ok_or_else(|| SnapshotError::InvalidData {
            details: format!("node id not found: {id}"),
        })?;
    Ok((
        DominatorJobKey {
            session,
            target,
            max_depth,
        },
        id,
        skip,
        limit,
    ))
}

fn generate_session_token() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("s{ts}-{}", std::process::id())
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
    size_unit: SizeUnit,
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
    write_size_unit_control(out, size_unit);
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
    before: &str,
    after: &str,
    top: usize,
    search: Option<&str>,
    skip: usize,
    limit: usize,
    size_unit: SizeUnit,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/diff\" class=\"controls\">"
    );
    let _ = writeln!(
        out,
        "<input type=\"hidden\" name=\"before\" value=\"{}\">",
        escape_html(before)
    );
    let _ = writeln!(
        out,
        "<input type=\"hidden\" name=\"after\" value=\"{}\">",
        escape_html(after)
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
    write_size_unit_control(out, size_unit);
    write_skip_limit_controls(out, skip, limit);
    let _ = writeln!(out, "<button type=\"submit\">Apply</button></form>");
}

fn write_diff_upload_controls(
    out: &mut String,
    top: usize,
    search: Option<&str>,
    skip: usize,
    limit: usize,
    size_unit: SizeUnit,
) {
    let _ = writeln!(
        out,
        "<form id=\"diff-upload-form\" method=\"post\" action=\"/diff\" enctype=\"multipart/form-data\" class=\"controls\">"
    );
    let _ = writeln!(
        out,
        "<label>After file <input type=\"file\" name=\"after\" accept=\".heapsnapshot,.json\" required></label>"
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
    write_size_unit_control(out, size_unit);
    let _ = writeln!(
        out,
        "<label>Skip <input type=\"number\" min=\"0\" name=\"skip\" value=\"{}\"></label>",
        skip
    );
    let _ = writeln!(
        out,
        "<label>Limit <input type=\"number\" min=\"1\" name=\"limit\" value=\"{}\"></label>",
        limit
    );
    let _ = writeln!(
        out,
        "<button type=\"submit\">Upload and Diff</button></form>"
    );
    let _ = writeln!(
        out,
        "<div id=\"diff-upload-status\" class=\"progress-status\" aria-live=\"polite\"></div>"
    );
    let _ = writeln!(
        out,
        "<progress id=\"diff-upload-progress\" max=\"100\" value=\"0\" style=\"display:none\"></progress>"
    );
    let _ = writeln!(
        out,
        "<div id=\"diff-analysis-status\" class=\"progress-status\" style=\"display:none\">Analyzing uploaded snapshot...</div>"
    );
    let _ = writeln!(out, "<script>{}</script>", diff_upload_progress_script());
}

fn write_size_unit_control(out: &mut String, size_unit: SizeUnit) {
    let _ = writeln!(out, "<label>Size Unit <select name=\"size_unit\">");
    for option in [SizeUnit::Bytes, SizeUnit::KiB, SizeUnit::MiB, SizeUnit::GiB] {
        let selected = if option == size_unit { " selected" } else { "" };
        let _ = writeln!(
            out,
            "<option value=\"{}\"{}>{}</option>",
            option.as_query_value(),
            selected,
            option.label()
        );
    }
    let _ = writeln!(out, "</select></label>");
}

fn diff_upload_progress_script() -> &'static str {
    "(() => {
  const form = document.getElementById('diff-upload-form');
  const status = document.getElementById('diff-upload-status');
  const progress = document.getElementById('diff-upload-progress');
  const analyzing = document.getElementById('diff-analysis-status');
  if (!form || !status || !progress || !analyzing || !window.XMLHttpRequest || !window.FormData) {
    return;
  }
  form.addEventListener('submit', (event) => {
    event.preventDefault();
    const fileInput = form.querySelector('input[name=\"after\"]');
    if (!fileInput || !fileInput.files || fileInput.files.length === 0) {
      status.textContent = 'Select a .heapsnapshot file before uploading.';
      progress.style.display = 'none';
      analyzing.style.display = 'none';
      return;
    }

    const request = new XMLHttpRequest();
    request.open('POST', form.action, true);
    request.responseType = 'text';
    progress.style.display = 'block';
    progress.value = 0;
    status.textContent = 'Uploading... 0%';
    analyzing.style.display = 'none';

    request.upload.onprogress = (ev) => {
      if (!ev.lengthComputable || ev.total === 0) {
        status.textContent = 'Uploading...';
        return;
      }
      const percent = Math.min(100, Math.floor((ev.loaded / ev.total) * 100));
      progress.value = percent;
      status.textContent = `Uploading... ${percent}%`;
    };

    request.upload.onload = () => {
      progress.style.display = 'none';
      status.textContent = 'Upload complete.';
      analyzing.style.display = 'block';
    };

    request.onerror = () => {
      status.textContent = 'Upload failed. Retry with a valid .heapsnapshot file.';
      progress.style.display = 'none';
      analyzing.style.display = 'none';
    };

    request.onload = () => {
      if (request.status >= 200 && request.status < 300) {
        document.open();
        document.write(request.responseText);
        document.close();
        return;
      }
      status.textContent = `Request failed (${request.status}). Check file and retry.`;
      progress.style.display = 'none';
      analyzing.style.display = 'none';
    };

    const formData = new FormData(form);
    request.send(formData);
  });
})();"
}

fn table_column_resize_script() -> &'static str {
    "(() => {
  const tables = document.querySelectorAll('table.resizable-table');
  tables.forEach((table) => {
    const headRow = table.querySelector('thead tr');
    if (!headRow) return;
    const headers = Array.from(headRow.children).filter((cell) => cell.tagName === 'TH');
    if (headers.length === 0) return;

    const colGroup = document.createElement('colgroup');
    const cols = headers.map((th) => {
      const col = document.createElement('col');
      const width = Math.max(80, Math.floor(th.getBoundingClientRect().width || 120));
      col.style.width = `${width}px`;
      colGroup.appendChild(col);
      return col;
    });
    table.prepend(colGroup);

    headers.forEach((th, index) => {
      th.style.position = 'relative';
      const handle = document.createElement('div');
      handle.className = 'col-resizer';
      handle.setAttribute('role', 'separator');
      handle.setAttribute('aria-orientation', 'vertical');
      handle.title = 'Drag to resize column';
      th.appendChild(handle);

      let dragging = false;
      let startX = 0;
      let startWidth = 0;

      const onMove = (ev) => {
        if (!dragging) return;
        const delta = ev.clientX - startX;
        const next = Math.max(80, startWidth + delta);
        cols[index].style.width = `${next}px`;
      };

      const onUp = () => {
        if (!dragging) return;
        dragging = false;
        document.body.classList.remove('resizing');
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
      };

      handle.addEventListener('mousedown', (ev) => {
        ev.preventDefault();
        dragging = true;
        startX = ev.clientX;
        startWidth = parseFloat(cols[index].style.width || '120');
        document.body.classList.add('resizing');
        window.addEventListener('mousemove', onMove);
        window.addEventListener('mouseup', onUp);
      });
    });
  });
})();"
}

fn write_dominator_controls(
    out: &mut String,
    id: u64,
    max_depth: usize,
    skip: usize,
    limit: usize,
    session: &str,
) {
    let _ = writeln!(
        out,
        "<form method=\"get\" action=\"/dominator\" class=\"controls\">"
    );
    let _ = writeln!(out, "<input type=\"hidden\" name=\"id\" value=\"{}\">", id);
    let _ = writeln!(
        out,
        "<input type=\"hidden\" name=\"session\" value=\"{}\">",
        escape_html(session)
    );
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
    size_unit: SizeUnit,
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
    write_size_unit_control(out, size_unit);
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
        400 => "Bad Request",
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
    "body{font-family:ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;width:100%;margin-top:12px}.resizable-table{table-layout:fixed}th,td{border:1px solid #ddd;padding:8px;vertical-align:top;overflow-wrap:anywhere;word-break:break-word}th{text-align:left;background:#f6f6f6}.col-resizer{position:absolute;top:0;right:-3px;width:6px;height:100%;cursor:col-resize;user-select:none}.resizing,.resizing *{cursor:col-resize!important;user-select:none!important}tr:nth-child(even){background:#fafafa}a{color:#0b5fff;text-decoration:none}a:hover{text-decoration:underline}.controls{display:flex;gap:12px;align-items:end;margin:12px 0;flex-wrap:wrap}.controls label{display:flex;gap:6px;align-items:center}.progress-status{margin:8px 0}progress{display:block;width:min(520px,100%);height:14px;margin:6px 0 10px}"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;
    use crate::parser::{self, ReadOptions};
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_context(snapshot: SnapshotRaw) -> ServerContext {
        let id_index = build_id_index(&snapshot);
        ServerContext {
            snapshot: Arc::new(snapshot),
            before_path: PathBuf::from("fixtures/small.heapsnapshot"),
            cancel: CancelToken::new(),
            id_index,
            dominator_jobs: Arc::new(Mutex::new(HashMap::new())),
            dominator_session_active: Arc::new(Mutex::new(HashMap::new())),
            dominator_index_cache: Arc::new(Mutex::new(None)),
            uploaded_temp_files: Arc::new(Mutex::new(Vec::new())),
            uploaded_display_names: Arc::new(Mutex::new(HashMap::new())),
            snapshot_cache: Arc::new(Mutex::new(HashMap::new())),
            diff_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn multipart_request(
        parts: &[(&str, Option<&str>, &[u8])],
    ) -> (HashMap<String, String>, Vec<u8>) {
        let boundary = "----heapsnap-boundary";
        let mut body = Vec::new();
        for (name, filename, content) in parts {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            match filename {
                Some(filename) => {
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\r\n",
                            name, filename
                        )
                        .as_bytes(),
                    );
                }
                None => {
                    body.extend_from_slice(
                        format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name)
                            .as_bytes(),
                    );
                }
            }
            body.extend_from_slice(content);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--").as_bytes());
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            format!("multipart/form-data; boundary={boundary}"),
        );
        headers.insert("content-length".to_string(), body.len().to_string());
        (headers, body)
    }

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
        let context = test_context(snapshot);
        let headers = HashMap::new();
        let body = Vec::new();

        let res = route(
            "GET",
            "/summary",
            &HashMap::new(),
            &headers,
            &body,
            &context,
        )
        .expect("summary");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("class=\"resizable-table\""));
        assert!(res.body.contains("Drag to resize column"));

        let mut detail_query = HashMap::new();
        detail_query.insert("name".to_string(), "Node1".to_string());
        let res =
            route("GET", "/detail", &detail_query, &headers, &body, &context).expect("detail");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("class=\"resizable-table\""));

        let mut ret_query = HashMap::new();
        ret_query.insert("id".to_string(), "3".to_string());
        let res =
            route("GET", "/retainers", &ret_query, &headers, &body, &context).expect("retainers");
        assert_eq!(res.status, 200);

        let mut dom_query = HashMap::new();
        dom_query.insert("id".to_string(), "3".to_string());
        let res =
            route("GET", "/dominator", &dom_query, &headers, &body, &context).expect("dominator");
        assert_eq!(res.status, 200);

        let mut diff_query = HashMap::new();
        diff_query.insert(
            "before".to_string(),
            "fixtures/small.heapsnapshot".to_string(),
        );
        diff_query.insert(
            "after".to_string(),
            "fixtures/small.heapsnapshot".to_string(),
        );
        let res = route("GET", "/diff", &diff_query, &headers, &body, &context).expect("diff");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("class=\"resizable-table\""));
    }

    #[test]
    fn detail_controls_reflect_query_values() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let headers = HashMap::new();
        let body = Vec::new();

        let mut query = HashMap::new();
        query.insert("name".to_string(), "Node1".to_string());
        query.insert("skip".to_string(), "1".to_string());
        query.insert("limit".to_string(), "50".to_string());
        let res = route("GET", "/detail", &query, &headers, &body, &context).expect("detail");
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
        let context = test_context(snapshot);
        let headers = HashMap::new();
        let body = Vec::new();

        let mut query = HashMap::new();
        query.insert("top".to_string(), "99".to_string());
        query.insert("search".to_string(), "Node".to_string());
        query.insert("skip".to_string(), "2".to_string());
        query.insert("limit".to_string(), "25".to_string());
        let res = route("GET", "/summary", &query, &headers, &body, &context).expect("summary");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("name=\"top\" value=\"99\""));
        assert!(res.body.contains("name=\"search\" value=\"Node\""));
        assert!(res.body.contains("name=\"skip\" value=\"2\""));
        assert!(
            res.body
                .contains("<option value=\"25\" selected>25</option>")
        );
        assert!(
            res.body
                .contains("<option value=\"bytes\" selected>bytes</option>")
        );
    }

    #[test]
    fn summary_renders_selected_size_unit() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let headers = HashMap::new();
        let body = Vec::new();

        let mut query = HashMap::new();
        query.insert("size_unit".to_string(), "mib".to_string());
        let res = route("GET", "/summary", &query, &headers, &body, &context).expect("summary");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("Self Size Sum (MiB)"));
        assert!(
            res.body
                .contains("<option value=\"mib\" selected>MiB</option>")
        );
    }

    #[test]
    fn index_has_diff_link() {
        let html = render_index();
        assert!(html.contains("<a href=\"/diff\">Diff (upload file)</a>"));
    }

    #[test]
    fn diff_without_query_renders_upload_form() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let headers = HashMap::new();
        let body = Vec::new();

        let res = route("GET", "/diff", &HashMap::new(), &headers, &body, &context).expect("diff");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("type=\"file\""));
        assert!(res.body.contains("name=\"after\""));
        assert!(res.body.contains("id=\"diff-upload-progress\""));
        assert!(res.body.contains("Analyzing uploaded snapshot..."));
    }

    #[test]
    fn content_fingerprint_same_input_same_value() {
        let a = content_fingerprint(b"same");
        let b = content_fingerprint(b"same");
        let c = content_fingerprint(b"different");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn uploaded_file_path_reused_for_same_content() {
        let file_a = UploadedFile {
            filename: None,
            content: b"{\"same\":true}".to_vec(),
        };
        let file_b = UploadedFile {
            filename: None,
            content: b"{\"same\":true}".to_vec(),
        };
        let path_a = write_uploaded_after_snapshot(&file_a).expect("write a");
        let path_b = write_uploaded_after_snapshot(&file_b).expect("write b");
        assert_eq!(path_a, path_b);
        assert!(path_a.exists());
        let _ = fs::remove_file(path_a);
    }

    #[test]
    fn cleanup_uploaded_temp_files_removes_registered_paths() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);

        let mut file1 = std::env::temp_dir();
        file1.push(format!("heapsnap-cleanup-{}-1.tmp", std::process::id()));
        let mut file2 = std::env::temp_dir();
        file2.push(format!("heapsnap-cleanup-{}-2.tmp", std::process::id()));
        fs::write(&file1, b"tmp1").expect("tmp1");
        fs::write(&file2, b"tmp2").expect("tmp2");

        {
            let mut guard = context.uploaded_temp_files.lock().expect("lock");
            guard.push(file1.clone());
            guard.push(file2.clone());
        }

        cleanup_uploaded_temp_files(&context);
        assert!(!file1.exists());
        assert!(!file2.exists());
    }

    #[test]
    fn write_dominator_controls_sets_session_hidden_field() {
        let mut html = String::new();
        write_dominator_controls(&mut html, 3, 50, 0, 25, "session-abc");
        assert!(html.contains("name=\"session\" value=\"session-abc\""));
    }

    #[test]
    fn bind_with_retry_advances_port_on_addr_in_use() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_ref = Arc::clone(&attempts);
        let result = bind_with_retry(9000, move |_port| {
            let n = attempts_ref.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(io::Error::from(io::ErrorKind::AddrInUse))
            } else {
                Ok(())
            }
        })
        .expect("bind");
        assert_eq!(result.1, 9002);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn bind_with_retry_returns_error_on_non_addr_in_use() {
        let err = bind_with_retry(9100, |_port| -> Result<(), io::Error> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .expect_err("expected error");
        assert!(matches!(err, SnapshotError::Io(_)));
    }

    #[test]
    fn bind_with_retry_reports_exhaustion_at_u16_max() {
        let err = bind_with_retry(u16::MAX, |_port| -> Result<(), io::Error> {
            Err(io::Error::from(io::ErrorKind::AddrInUse))
        })
        .expect_err("expected exhaustion");
        assert!(matches!(err, SnapshotError::InvalidData { .. }));
    }

    #[test]
    fn parse_multipart_form_accepts_file_and_fields() {
        let (headers, body) = multipart_request(&[
            ("after", Some("sample.heapsnapshot"), b"{\"a\":1}"),
            ("top", None, b"123"),
        ]);
        let form = parse_multipart_form(&headers, &body).expect("multipart");
        assert!(form.file.is_some());
        assert_eq!(form.fields.get("top").map(String::as_str), Some("123"));
    }

    #[test]
    fn parse_multipart_form_rejects_missing_content_type() {
        let err = parse_multipart_form(&HashMap::new(), b"").expect_err("expected error");
        assert!(matches!(err, SnapshotError::InvalidData { .. }));
    }

    #[test]
    fn render_diff_post_requires_after_file_field() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let (headers, body) = multipart_request(&[("top", None, b"10")]);
        let res = render_diff_post(&headers, &body, &context).expect("response");
        assert_eq!(res.status, 400);
        assert!(res.body.contains("missing `after` file field"));
    }

    #[test]
    fn render_diff_post_registers_uploaded_temp_path() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let bytes = fs::read("fixtures/small.heapsnapshot").expect("fixture bytes");
        let (headers, body) = multipart_request(&[("after", Some("small.heapsnapshot"), &bytes)]);
        let res = render_diff_post(&headers, &body, &context).expect("response");
        assert_eq!(res.status, 200);
        let guard = context.uploaded_temp_files.lock().expect("lock");
        assert_eq!(guard.len(), 1);
        assert!(guard[0].exists());
        let _ = fs::remove_file(&guard[0]);
    }

    #[test]
    fn render_diff_post_keeps_size_unit_field() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let bytes = fs::read("fixtures/small.heapsnapshot").expect("fixture");
        let (headers, body) = multipart_request(&[
            ("after", Some("small.heapsnapshot"), &bytes),
            ("size_unit", None, b"gib"),
        ]);
        let res = render_diff_post(&headers, &body, &context).expect("response");
        assert_eq!(res.status, 200);
        assert!(res.body.contains("Self Size Δ (GiB)"));
        assert!(
            res.body
                .contains("<option value=\"gib\" selected>GiB</option>")
        );
    }

    #[test]
    fn render_diff_shows_file_name_then_path() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let before_path = PathBuf::from("fixtures/small.heapsnapshot");
        let mut after_path = std::env::temp_dir();
        after_path.push(format!(
            "heapsnap-display-{}.heapsnapshot",
            std::process::id()
        ));
        fs::write(
            &after_path,
            fs::read("fixtures/small.heapsnapshot").expect("fixture"),
        )
        .expect("write");
        {
            let mut guard = context.uploaded_display_names.lock().expect("lock");
            guard.insert(after_path.clone(), "from-browser.heapsnapshot".to_string());
        }
        let mut query = HashMap::new();
        query.insert("before".to_string(), before_path.display().to_string());
        query.insert("after".to_string(), after_path.display().to_string());
        let html = render_diff(&query, &context).expect("render");
        assert!(html.contains("Before:</strong> small.heapsnapshot (fixtures/small.heapsnapshot)"));
        assert!(html.contains("After:</strong> from-browser.heapsnapshot ("));
        let _ = fs::remove_file(after_path);
    }

    #[test]
    fn dominator_sse_script_contains_session_query() {
        let script = dominator_sse_script(3, 50, 0, 25, "abc");
        assert!(script.contains(
            "EventSource('/dominator/events?id=3&max_depth=50&skip=0&limit=25&session=abc')"
        ));
    }

    #[test]
    fn render_dominator_loading_contains_session_replace_script() {
        let html = render_dominator_loading(
            3,
            50,
            0,
            25,
            "sess-x",
            &DominatorProgressView {
                phase: "build_graph".to_string(),
                percent: 10,
                completed: 1,
                total: 10,
            },
        );
        assert!(html.contains("history.replaceState"));
        assert!(html.contains("session=sess-x"));
    }

    #[test]
    fn get_or_start_dominator_job_cancels_previous_session_job() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let target = *context.id_index.values().next().expect("any target");

        let first = get_or_start_dominator_job(
            &context,
            DominatorJobKey {
                session: "s1".to_string(),
                target,
                max_depth: 10,
            },
        );
        let second = get_or_start_dominator_job(
            &context,
            DominatorJobKey {
                session: "s1".to_string(),
                target,
                max_depth: 1,
            },
        );
        let first_cancelled = {
            let guard = first.lock().expect("lock");
            guard.cancel.is_cancelled()
        };
        assert!(first_cancelled);
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn progress_to_view_is_phase_local_percentage() {
        let build = progress_to_view(&analysis::dominator::DominatorProgress {
            phase: analysis::dominator::DominatorPhase::BuildGraph,
            nodes_done: 0,
            nodes_total: 100,
            edges_done: 50,
            edges_total: 100,
            idom_iteration: 0,
        });
        assert_eq!(build.percent, 50);
        let idom = progress_to_view(&analysis::dominator::DominatorProgress {
            phase: analysis::dominator::DominatorPhase::ComputeIdom,
            nodes_done: 1,
            nodes_total: 100,
            edges_done: 0,
            edges_total: 1,
            idom_iteration: 0,
        });
        assert_eq!(idom.percent, 1);
    }

    #[test]
    fn load_snapshot_cached_uses_in_memory_before_snapshot() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let before = context.before_path.clone();
        let loaded = load_snapshot_cached(&context, &before).expect("loaded");
        assert!(Arc::ptr_eq(&loaded, &context.snapshot));
    }

    #[test]
    fn render_diff_reuses_diff_cache_for_same_query() {
        let snapshot = parser::read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let context = test_context(snapshot);
        let mut query = HashMap::new();
        query.insert(
            "before".to_string(),
            "fixtures/small.heapsnapshot".to_string(),
        );
        query.insert(
            "after".to_string(),
            "fixtures/small.heapsnapshot".to_string(),
        );
        query.insert("top".to_string(), "50".to_string());
        let _ = render_diff(&query, &context).expect("first");
        let cache_len_after_first = context.diff_cache.lock().expect("lock").len();
        let _ = render_diff(&query, &context).expect("second");
        let cache_len_after_second = context.diff_cache.lock().expect("lock").len();
        assert_eq!(cache_len_after_first, 1);
        assert_eq!(cache_len_after_second, 1);
    }
}
