use std::collections::{HashMap, HashSet};

use crate::cancel::CancelToken;
use crate::error::SnapshotError;
use crate::snapshot::{NodeView, SnapshotRaw};

#[derive(Debug)]
pub struct RetainersOptions {
    pub max_paths: usize,
    pub max_depth: usize,
    pub cancel: CancelToken,
}

#[derive(Debug, Clone, Copy)]
pub struct RetainerLink {
    pub from_node: usize,
    pub edge_index: usize,
    pub to_node: usize,
}

#[derive(Debug)]
pub struct RetainersResult {
    pub target: usize,
    pub roots: Vec<usize>,
    pub paths: Vec<Vec<RetainerLink>>,
}

pub fn find_target_by_id(
    snapshot: &SnapshotRaw,
    node_id: u64,
) -> Result<usize, SnapshotError> {
    for index in 0..snapshot.node_count() {
        let node = snapshot.node_view(index).ok_or_else(|| SnapshotError::InvalidData {
            details: format!("node index out of range: {index}"),
        })?;
        if node.id() == Some(node_id as i64) {
            return Ok(index);
        }
    }
    Err(SnapshotError::InvalidData {
        details: format!(
            "node id not found: {node_id} (use --name to select a constructor or verify the id)"
        ),
    })
}

pub fn find_target_by_name(
    snapshot: &SnapshotRaw,
    name_filter: &str,
    pick: PickStrategy,
) -> Result<usize, SnapshotError> {
    let mut candidates: HashMap<String, NameCandidate> = HashMap::new();

    for index in 0..snapshot.node_count() {
        let node = snapshot.node_view(index).ok_or_else(|| SnapshotError::InvalidData {
            details: format!("node index out of range: {index}"),
        })?;
        let name = node.name().unwrap_or("<unknown>");
        if !name.contains(name_filter) {
            continue;
        }

        let entry = candidates
            .entry(name.to_string())
            .or_insert_with(|| NameCandidate::new(name.to_string()));
        entry.count += 1;
        entry.self_size_sum += node.self_size().unwrap_or(0);
        let self_size = node.self_size().unwrap_or(0);
        if self_size > entry.largest_self_size {
            entry.largest_self_size = self_size;
            entry.largest_node_index = index;
        }
    }

    if candidates.is_empty() {
        return Err(SnapshotError::InvalidData {
            details: format!(
                "no nodes match name filter: {name_filter} (try a different substring or use --id)"
            ),
        });
    }

    let mut items: Vec<NameCandidate> = candidates.into_values().collect();
    items.sort_by(|a, b| match pick {
        PickStrategy::Largest => b
            .self_size_sum
            .cmp(&a.self_size_sum)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.name.cmp(&b.name)),
        PickStrategy::Count => b
            .count
            .cmp(&a.count)
            .then_with(|| b.self_size_sum.cmp(&a.self_size_sum))
            .then_with(|| a.name.cmp(&b.name)),
    });

    Ok(items[0].largest_node_index)
}

#[derive(Debug, Clone, Copy)]
pub enum PickStrategy {
    Largest,
    Count,
}

#[derive(Debug)]
struct NameCandidate {
    name: String,
    count: u64,
    self_size_sum: i64,
    largest_self_size: i64,
    largest_node_index: usize,
}

impl NameCandidate {
    fn new(name: String) -> Self {
        Self {
            name,
            count: 0,
            self_size_sum: 0,
            largest_self_size: i64::MIN,
            largest_node_index: 0,
        }
    }
}

pub fn find_retaining_paths(
    snapshot: &SnapshotRaw,
    target: usize,
    options: RetainersOptions,
) -> Result<RetainersResult, SnapshotError> {
    let roots = find_roots(snapshot)?;
    let root_set: HashSet<usize> = roots.iter().copied().collect();
    let edge_offsets = compute_edge_offsets(snapshot)?;
    let mut incoming = IncomingIndex::new(snapshot, edge_offsets);

    if root_set.contains(&target) {
        return Ok(RetainersResult {
            target,
            roots,
            paths: vec![vec![]],
        });
    }

    let mut paths: Vec<Vec<RetainerLink>> = Vec::new();
    let mut layer: Vec<PathState> = vec![PathState::new(target)];
    let mut depth = 0usize;

    while depth < options.max_depth && !layer.is_empty() && paths.len() < options.max_paths {
        if options.cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }
        let targets: Vec<usize> = layer.iter().map(|state| state.node).collect();
        incoming.build_for_targets(&targets)?;

        let mut next_layer = Vec::new();
        for state in layer {
            let incoming_edges = incoming.get(state.node)?;
            for edge in incoming_edges {
                if options.cancel.is_cancelled() {
                    return Err(SnapshotError::Cancelled);
                }
                if paths.len() >= options.max_paths {
                    break;
                }
                if state.visited.contains(&edge.from_node) {
                    continue;
                }
                let next_state = state.extend(*edge);
                if root_set.contains(&edge.from_node) {
                    let mut steps = next_state.steps.clone();
                    steps.reverse();
                    paths.push(steps);
                } else {
                    next_layer.push(next_state);
                }
            }
        }
        layer = next_layer;
        depth += 1;
    }

    Ok(RetainersResult {
        target,
        roots,
        paths,
    })
}

pub fn find_roots(snapshot: &SnapshotRaw) -> Result<Vec<usize>, SnapshotError> {
    let mut roots = Vec::new();
    for index in 0..snapshot.node_count() {
        let node = snapshot.node_view(index).ok_or_else(|| SnapshotError::InvalidData {
            details: format!("node index out of range: {index}"),
        })?;
        if is_gc_root(&node) {
            roots.push(index);
        }
    }

    if roots.is_empty() {
        if snapshot.node_count() > 0 {
            roots.push(0);
        }
    }

    if roots.is_empty() {
        return Err(SnapshotError::InvalidData {
            details: "GC roots not found in snapshot (expected name \"GC roots\")".to_string(),
        });
    }
    Ok(roots)
}

fn is_gc_root(node: &NodeView<'_>) -> bool {
    matches!(node.name(), Some("GC roots"))
}

struct IncomingIndex<'a> {
    snapshot: &'a SnapshotRaw,
    edge_offsets: Vec<usize>,
    built: HashSet<usize>,
    incoming: HashMap<usize, Vec<RetainerLink>>,
}

impl<'a> IncomingIndex<'a> {
    fn new(snapshot: &'a SnapshotRaw, edge_offsets: Vec<usize>) -> Self {
        Self {
            snapshot,
            edge_offsets,
            built: HashSet::new(),
            incoming: HashMap::new(),
        }
    }

    fn build_for_targets(&mut self, targets: &[usize]) -> Result<(), SnapshotError> {
        let needed: HashSet<usize> = targets
            .iter()
            .copied()
            .filter(|node| !self.built.contains(node))
            .collect();
        if needed.is_empty() {
            return Ok(());
        }

        for (node_index, start_edge) in self.edge_offsets.iter().enumerate() {
            let node = self.snapshot.node_view(node_index).ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {node_index}"),
            })?;
            let edge_count = node.edge_count().unwrap_or(0);
            let edge_count = usize::try_from(edge_count).map_err(|_| SnapshotError::InvalidData {
                details: format!("edge_count negative at node {node_index}"),
            })?;
            for offset in 0..edge_count {
                let edge_index = start_edge + offset;
                let edge = self.snapshot.edge_view(edge_index).ok_or_else(|| SnapshotError::InvalidData {
                    details: format!("edge index out of range: {edge_index}"),
                })?;
                let to_node = match edge.to_node_index() {
                    Some(value) => value,
                    None => continue,
                };
                if !needed.contains(&to_node) {
                    continue;
                }
                self.incoming
                    .entry(to_node)
                    .or_insert_with(Vec::new)
                    .push(RetainerLink {
                        from_node: node_index,
                        edge_index,
                        to_node,
                    });
            }
        }

        self.built.extend(needed);
        Ok(())
    }

    fn get(&self, node_index: usize) -> Result<&[RetainerLink], SnapshotError> {
        Ok(self
            .incoming
            .get(&node_index)
            .map(Vec::as_slice)
            .unwrap_or(&[]))
    }
}

fn compute_edge_offsets(snapshot: &SnapshotRaw) -> Result<Vec<usize>, SnapshotError> {
    let mut offsets = Vec::with_capacity(snapshot.node_count());
    let mut cursor = 0usize;

    for node_index in 0..snapshot.node_count() {
        offsets.push(cursor);
        let node = snapshot.node_view(node_index).ok_or_else(|| SnapshotError::InvalidData {
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

#[derive(Debug, Clone)]
struct PathState {
    node: usize,
    steps: Vec<RetainerLink>,
    visited: Vec<usize>,
}

impl PathState {
    fn new(node: usize) -> Self {
        Self {
            node,
            steps: Vec::new(),
            visited: vec![node],
        }
    }

    fn extend(&self, edge: RetainerLink) -> Self {
        let mut steps = self.steps.clone();
        steps.push(edge);
        let mut visited = self.visited.clone();
        visited.push(edge.from_node);
        Self {
            node: edge.from_node,
            steps,
            visited,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{MetaType, SnapshotMeta, SnapshotRaw};

    fn sample_snapshot() -> SnapshotRaw {
        let meta = SnapshotMeta {
            node_fields: vec![
                "type".to_string(),
                "name".to_string(),
                "id".to_string(),
                "self_size".to_string(),
                "edge_count".to_string(),
            ],
            node_types: vec![
                MetaType::Array(vec!["synthetic".to_string(), "object".to_string()]),
                MetaType::String("string".to_string()),
                MetaType::String("number".to_string()),
                MetaType::String("number".to_string()),
                MetaType::String("number".to_string()),
            ],
            edge_fields: vec![
                "type".to_string(),
                "name_or_index".to_string(),
                "to_node".to_string(),
            ],
            edge_types: vec![
                MetaType::Array(vec!["property".to_string()]),
                MetaType::String("string_or_number".to_string()),
                MetaType::String("node".to_string()),
            ],
        };
        let index = meta.validate().expect("meta ok");

        SnapshotRaw {
            nodes: vec![
                0, 0, 1, 0, 1, // node 0: GC roots
                1, 1, 2, 0, 0, // node 1: App
            ],
            edges: vec![
                0, 1, 5, // edge 0: from node 0 to node 1
            ],
            strings: vec!["GC roots".to_string(), "App".to_string()],
            meta,
            index,
        }
    }

    #[test]
    fn find_path_from_root() {
        let snapshot = sample_snapshot();
        let result = find_retaining_paths(
            &snapshot,
            1,
            RetainersOptions {
                max_paths: 5,
                max_depth: 5,
                cancel: CancelToken::new(),
            },
        )
        .expect("paths");

        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].len(), 1);
        assert_eq!(result.paths[0][0].from_node, 0);
        assert_eq!(result.paths[0][0].to_node, 1);
    }
}
