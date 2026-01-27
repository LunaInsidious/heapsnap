use crate::analysis::retainers::find_roots;
use crate::cancel::CancelToken;
use crate::error::SnapshotError;
use crate::snapshot::SnapshotRaw;

#[derive(Debug)]
pub struct DominatorOptions {
    pub max_depth: usize,
    pub cancel: CancelToken,
}

#[derive(Debug)]
pub struct DominatorResult {
    pub target: usize,
    pub roots: Vec<usize>,
    pub chain: Vec<usize>,
}

pub fn dominator_chain(
    snapshot: &SnapshotRaw,
    target: usize,
    options: DominatorOptions,
) -> Result<DominatorResult, SnapshotError> {
    let roots = find_roots(snapshot)?;
    let (succs, preds) = build_graph(snapshot)?;
    let (rpo, rpo_index) = reverse_postorder(&succs, &roots);
    let idom = compute_idom(&rpo, &rpo_index, &preds, &roots, &options.cancel)?;

    let mut chain = Vec::new();
    let mut current = target;
    for _ in 0..=options.max_depth {
        if options.cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }
        chain.push(current);
        let next = match idom.get(current).copied().flatten() {
            Some(value) => value,
            None => break,
        };
        if next == current {
            break;
        }
        current = next;
    }

    if chain.is_empty() {
        return Err(SnapshotError::InvalidData {
            details: "target is not reachable from roots".to_string(),
        });
    }

    chain.reverse();
    Ok(DominatorResult {
        target,
        roots,
        chain,
    })
}

fn build_graph(
    snapshot: &SnapshotRaw,
) -> Result<(Vec<Vec<usize>>, Vec<Vec<usize>>), SnapshotError> {
    let node_count = snapshot.node_count();
    let mut succs = vec![Vec::new(); node_count];
    let mut preds = vec![Vec::new(); node_count];

    let edge_offsets = compute_edge_offsets(snapshot)?;
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
            if to_node >= node_count {
                continue;
            }
            succs[node_index].push(to_node);
            preds[to_node].push(node_index);
        }
    }

    Ok((succs, preds))
}

fn reverse_postorder(succs: &[Vec<usize>], roots: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let node_count = succs.len();
    let mut visited = vec![false; node_count];
    let mut postorder = Vec::new();

    for &root in roots {
        if root >= node_count || visited[root] {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        stack.push((root, 0));
        visited[root] = true;

        while let Some((node, idx)) = stack.pop() {
            if idx < succs[node].len() {
                stack.push((node, idx + 1));
                let next = succs[node][idx];
                if next < node_count && !visited[next] {
                    visited[next] = true;
                    stack.push((next, 0));
                }
            } else {
                postorder.push(node);
            }
        }
    }

    postorder.reverse();
    let mut index = vec![usize::MAX; node_count];
    for (i, node) in postorder.iter().enumerate() {
        index[*node] = i;
    }
    (postorder, index)
}

fn compute_idom(
    rpo: &[usize],
    rpo_index: &[usize],
    preds: &[Vec<usize>],
    roots: &[usize],
    cancel: &CancelToken,
) -> Result<Vec<Option<usize>>, SnapshotError> {
    let node_count = preds.len();
    let mut idom = vec![None; node_count];

    for &root in roots {
        if root < node_count {
            idom[root] = Some(root);
        }
    }

    if rpo.is_empty() {
        return Ok(idom);
    }

    let mut changed = true;
    while changed {
        if cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }
        changed = false;
        for &node in rpo {
            if roots.contains(&node) {
                continue;
            }
            let mut new_idom = None;
            for &pred in &preds[node] {
                if idom[pred].is_none() {
                    continue;
                }
                new_idom = Some(match new_idom {
                    None => pred,
                    Some(current) => intersect(pred, current, rpo_index, &idom),
                });
            }

            if new_idom.is_some() && idom[node] != new_idom {
                idom[node] = new_idom;
                changed = true;
            }
        }
    }

    Ok(idom)
}

fn intersect(
    mut finger1: usize,
    mut finger2: usize,
    rpo_index: &[usize],
    idom: &[Option<usize>],
) -> usize {
    while finger1 != finger2 {
        while rpo_index[finger1] < rpo_index[finger2] {
            finger1 = idom[finger1].unwrap_or(finger1);
        }
        while rpo_index[finger2] < rpo_index[finger1] {
            finger2 = idom[finger2].unwrap_or(finger2);
        }
    }
    finger1
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::retainers::find_target_by_id;
    use crate::parser::{ReadOptions, read_snapshot_file};
    use std::path::Path;

    #[test]
    fn dominator_chain_fixture_small() {
        let snapshot = read_snapshot_file(
            Path::new("fixtures/small.heapsnapshot"),
            ReadOptions::new(false, CancelToken::new()),
        )
        .expect("snapshot");
        let target = find_target_by_id(&snapshot, 3).expect("target");
        let result = dominator_chain(
            &snapshot,
            target,
            DominatorOptions {
                max_depth: 10,
                cancel: CancelToken::new(),
            },
        )
        .expect("dominator");
        assert!(result.chain.len() >= 2);
    }
}
