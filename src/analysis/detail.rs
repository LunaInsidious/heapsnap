use crate::error::SnapshotError;
use crate::snapshot::{EdgeView, SnapshotRaw};

#[derive(Debug)]
pub struct DetailOptions {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub skip: usize,
    pub limit: usize,
    pub top_retainers: usize,
    pub top_edges: usize,
}

#[derive(Debug)]
pub enum DetailResult {
    ByName(DetailByName),
    ById(DetailById),
}

#[derive(Debug)]
pub struct DetailByName {
    pub name: String,
    pub total_count: u64,
    pub self_size_sum: i64,
    pub max_self_size: i64,
    pub min_self_size: i64,
    pub avg_self_size: f64,
    pub ids: Vec<NodeRef>,
    pub skip: usize,
    pub limit: usize,
    pub total_ids: u64,
}

#[derive(Debug)]
pub struct DetailById {
    pub id: u64,
    pub node_index: usize,
    pub name: String,
    pub node_type: Option<String>,
    pub self_size: i64,
    pub total_count: u64,
    pub self_size_sum: i64,
    pub max_self_size: i64,
    pub min_self_size: i64,
    pub avg_self_size: f64,
    pub ids: Vec<NodeRef>,
    pub skip: usize,
    pub limit: usize,
    pub total_ids: u64,
    pub retainers: Vec<RetainerSummary>,
    pub outgoing_edges: Vec<OutgoingEdgeSummary>,
    pub shallow_size_distribution: Vec<ShallowSizeBucket>,
}

#[derive(Debug, Clone)]
pub struct NodeRef {
    pub index: usize,
    pub id: Option<i64>,
    pub node_type: Option<String>,
    pub self_size: i64,
}

#[derive(Debug, Clone)]
pub struct RetainerSummary {
    pub from_index: usize,
    pub from_id: Option<i64>,
    pub from_name: Option<String>,
    pub from_node_type: Option<String>,
    pub from_self_size: i64,
    pub edge_index: usize,
    pub edge_type: Option<String>,
    pub edge_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OutgoingEdgeSummary {
    pub edge_index: usize,
    pub edge_type: Option<String>,
    pub edge_name: Option<String>,
    pub to_index: usize,
    pub to_id: Option<i64>,
    pub to_name: Option<String>,
    pub to_node_type: Option<String>,
    pub to_self_size: i64,
}

#[derive(Debug, Clone)]
pub struct ShallowSizeBucket {
    pub label: String,
    pub min: i64,
    pub max: Option<i64>,
    pub count: u64,
}

const DEFAULT_BUCKETS: &[(i64, Option<i64>)] = &[
    (0, Some(0)),
    (1, Some(31)),
    (32, Some(127)),
    (128, Some(511)),
    (512, Some(2047)),
    (2048, Some(8191)),
    (8192, Some(32767)),
    (32768, None),
];

pub fn detail(
    snapshot: &SnapshotRaw,
    options: DetailOptions,
) -> Result<DetailResult, SnapshotError> {
    if options.id.is_some() && options.name.is_some() {
        return Err(SnapshotError::InvalidData {
            details: "use either --id or --name, not both".to_string(),
        });
    }
    if options.id.is_none() && options.name.is_none() {
        return Err(SnapshotError::InvalidData {
            details: "either --id or --name must be specified".to_string(),
        });
    }

    if let Some(node_id) = options.id {
        let (node_index, name, node_type, self_size) = find_node_by_id(snapshot, node_id)?;
        let stats = collect_name_stats(snapshot, &name, options.skip, options.limit)?;
        let retainers = top_retainers(snapshot, node_index, options.top_retainers)?;
        let outgoing_edges = top_outgoing_edges(snapshot, node_index, options.top_edges)?;
        let distribution = shallow_size_distribution(snapshot, &name)?;

        return Ok(DetailResult::ById(DetailById {
            id: node_id,
            node_index,
            name,
            node_type,
            self_size,
            total_count: stats.total_count,
            self_size_sum: stats.self_size_sum,
            max_self_size: stats.max_self_size,
            min_self_size: stats.min_self_size,
            avg_self_size: stats.avg_self_size,
            ids: stats.ids,
            skip: stats.skip,
            limit: stats.limit,
            total_ids: stats.total_ids,
            retainers,
            outgoing_edges,
            shallow_size_distribution: distribution,
        }));
    }

    let name = options.name.unwrap_or_default();
    let stats = collect_name_stats(snapshot, &name, options.skip, options.limit)?;
    if stats.total_count == 0 {
        return Err(SnapshotError::InvalidData {
            details: format!("no nodes match name: {name}"),
        });
    }
    Ok(DetailResult::ByName(DetailByName {
        name,
        total_count: stats.total_count,
        self_size_sum: stats.self_size_sum,
        max_self_size: stats.max_self_size,
        min_self_size: stats.min_self_size,
        avg_self_size: stats.avg_self_size,
        ids: stats.ids,
        skip: stats.skip,
        limit: stats.limit,
        total_ids: stats.total_ids,
    }))
}

fn find_node_by_id(
    snapshot: &SnapshotRaw,
    node_id: u64,
) -> Result<(usize, String, Option<String>, i64), SnapshotError> {
    for index in 0..snapshot.node_count() {
        let node = snapshot
            .node_view(index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {index}"),
            })?;
        if node.id() == Some(node_id as i64) {
            let name = node.name().unwrap_or("<unknown>").to_string();
            let node_type = node.node_type().map(str::to_string);
            let self_size = node.self_size().unwrap_or(0);
            return Ok((index, name, node_type, self_size));
        }
    }
    Err(SnapshotError::InvalidData {
        details: format!("node id not found: {node_id} (use --name to select a constructor)"),
    })
}

struct NameStats {
    total_count: u64,
    self_size_sum: i64,
    max_self_size: i64,
    min_self_size: i64,
    avg_self_size: f64,
    ids: Vec<NodeRef>,
    skip: usize,
    limit: usize,
    total_ids: u64,
}

fn collect_name_stats(
    snapshot: &SnapshotRaw,
    target_name: &str,
    skip: usize,
    limit: usize,
) -> Result<NameStats, SnapshotError> {
    let mut total_count: u64 = 0;
    let mut self_size_sum: i64 = 0;
    let mut max_self_size: i64 = i64::MIN;
    let mut min_self_size: i64 = i64::MAX;
    let mut ids: Vec<NodeRef> = Vec::new();

    for index in 0..snapshot.node_count() {
        let node = snapshot
            .node_view(index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {index}"),
            })?;
        let name = node.name().unwrap_or("");
        if name != target_name {
            continue;
        }
        total_count += 1;
        let self_size = node.self_size().unwrap_or(0);
        self_size_sum += self_size;
        if self_size > max_self_size {
            max_self_size = self_size;
        }
        if self_size < min_self_size {
            min_self_size = self_size;
        }
        if total_count as usize > skip && ids.len() < limit {
            ids.push(NodeRef {
                index,
                id: node.id(),
                node_type: node.node_type().map(str::to_string),
                self_size,
            });
        }
    }

    if total_count == 0 {
        return Ok(NameStats {
            total_count: 0,
            self_size_sum: 0,
            max_self_size: 0,
            min_self_size: 0,
            avg_self_size: 0.0,
            ids,
            skip,
            limit,
            total_ids: 0,
        });
    }

    let avg_self_size = self_size_sum as f64 / total_count as f64;
    Ok(NameStats {
        total_count,
        self_size_sum,
        max_self_size,
        min_self_size,
        avg_self_size,
        ids,
        skip,
        limit,
        total_ids: total_count,
    })
}

fn top_retainers(
    snapshot: &SnapshotRaw,
    target: usize,
    limit: usize,
) -> Result<Vec<RetainerSummary>, SnapshotError> {
    let edge_offsets = compute_edge_offsets(snapshot)?;
    let mut items: Vec<RetainerSummary> = Vec::new();

    for (node_index, start_edge) in edge_offsets.iter().enumerate() {
        let node = snapshot
            .node_view(node_index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {node_index}"),
            })?;
        let edge_count = node.edge_count().unwrap_or(0);
        let edge_count = usize::try_from(edge_count).map_err(|_| SnapshotError::InvalidData {
            details: format!("edge_count negative at node {node_index}"),
        })?;
        for offset in 0..edge_count {
            let edge_index = start_edge + offset;
            let edge =
                snapshot
                    .edge_view(edge_index)
                    .ok_or_else(|| SnapshotError::InvalidData {
                        details: format!("edge index out of range: {edge_index}"),
                    })?;
            let to_node = match edge.to_node_index() {
                Some(value) => value,
                None => continue,
            };
            if to_node != target {
                continue;
            }
            let from_self_size = node.self_size().unwrap_or(0);
            items.push(RetainerSummary {
                from_index: node_index,
                from_id: node.id(),
                from_name: node.name().map(str::to_string),
                from_node_type: node.node_type().map(str::to_string),
                from_self_size,
                edge_index,
                edge_type: edge.edge_type().map(str::to_string),
                edge_name: edge_name(snapshot, edge),
            });
        }
    }

    items.sort_by(|a, b| {
        b.from_self_size
            .cmp(&a.from_self_size)
            .then_with(|| a.from_index.cmp(&b.from_index))
    });
    if items.len() > limit {
        items.truncate(limit);
    }
    Ok(items)
}

fn top_outgoing_edges(
    snapshot: &SnapshotRaw,
    node_index: usize,
    limit: usize,
) -> Result<Vec<OutgoingEdgeSummary>, SnapshotError> {
    let edge_offsets = compute_edge_offsets(snapshot)?;
    let start_edge =
        edge_offsets
            .get(node_index)
            .copied()
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {node_index}"),
            })?;
    let node = snapshot
        .node_view(node_index)
        .ok_or_else(|| SnapshotError::InvalidData {
            details: format!("node index out of range: {node_index}"),
        })?;
    let edge_count = node.edge_count().unwrap_or(0);
    let edge_count = usize::try_from(edge_count).map_err(|_| SnapshotError::InvalidData {
        details: format!("edge_count negative at node {node_index}"),
    })?;

    let mut items: Vec<OutgoingEdgeSummary> = Vec::new();
    for offset in 0..edge_count {
        let edge_index = start_edge + offset;
        let edge = snapshot
            .edge_view(edge_index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("edge index out of range: {edge_index}"),
            })?;
        let to_node = match edge.to_node_index() {
            Some(value) => value,
            None => continue,
        };
        let to_node_view = snapshot.node_view(to_node);
        let to_self_size = to_node_view.and_then(|n| n.self_size()).unwrap_or(0);
        items.push(OutgoingEdgeSummary {
            edge_index,
            edge_type: edge.edge_type().map(str::to_string),
            edge_name: edge_name(snapshot, edge),
            to_index: to_node,
            to_id: to_node_view.and_then(|n| n.id()),
            to_name: to_node_view.and_then(|n| n.name()).map(str::to_string),
            to_node_type: to_node_view.and_then(|n| n.node_type()).map(str::to_string),
            to_self_size,
        });
    }

    items.sort_by(|a, b| {
        b.to_self_size
            .cmp(&a.to_self_size)
            .then_with(|| a.edge_index.cmp(&b.edge_index))
    });
    if items.len() > limit {
        items.truncate(limit);
    }
    Ok(items)
}

fn shallow_size_distribution(
    snapshot: &SnapshotRaw,
    target_name: &str,
) -> Result<Vec<ShallowSizeBucket>, SnapshotError> {
    let mut buckets: Vec<ShallowSizeBucket> = DEFAULT_BUCKETS
        .iter()
        .map(|(min, max)| ShallowSizeBucket {
            label: bucket_label(*min, *max),
            min: *min,
            max: *max,
            count: 0,
        })
        .collect();

    for index in 0..snapshot.node_count() {
        let node = snapshot
            .node_view(index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {index}"),
            })?;
        let name = node.name().unwrap_or("");
        if name != target_name {
            continue;
        }
        let size = node.self_size().unwrap_or(0);
        for bucket in buckets.iter_mut() {
            let in_range = match bucket.max {
                Some(max) => size >= bucket.min && size <= max,
                None => size >= bucket.min,
            };
            if in_range {
                bucket.count += 1;
                break;
            }
        }
    }

    Ok(buckets)
}

fn bucket_label(min: i64, max: Option<i64>) -> String {
    match max {
        Some(max) => format!("{min}-{max}"),
        None => format!("{min}+"),
    }
}

fn compute_edge_offsets(snapshot: &SnapshotRaw) -> Result<Vec<usize>, SnapshotError> {
    let mut offsets = Vec::with_capacity(snapshot.node_count());
    let mut cursor = 0usize;

    for node_index in 0..snapshot.node_count() {
        offsets.push(cursor);
        let node = snapshot
            .node_view(node_index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {node_index}"),
            })?;
        let edge_count = node.edge_count().unwrap_or(0);
        let edge_count = usize::try_from(edge_count).map_err(|_| SnapshotError::InvalidData {
            details: format!("edge_count negative at node {node_index}"),
        })?;
        cursor = cursor.saturating_add(edge_count);
    }

    if cursor != snapshot.edge_count() {
        return Err(SnapshotError::InvalidData {
            details: format!(
                "edge_count sum ({}) does not match edges length ({})",
                cursor,
                snapshot.edge_count()
            ),
        });
    }

    Ok(offsets)
}

fn edge_name(snapshot: &SnapshotRaw, edge: EdgeView<'_>) -> Option<String> {
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
