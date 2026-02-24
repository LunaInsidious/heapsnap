use std::sync::mpsc::Sender;

use crate::analysis::retainers::find_roots;
use crate::cancel::CancelToken;
use crate::error::SnapshotError;
use crate::snapshot::SnapshotRaw;

pub struct DominatorOptions {
    pub max_depth: usize,
    pub cancel: CancelToken,
    pub progress: Option<Sender<DominatorProgress>>,
}

#[derive(Debug, Clone)]
pub struct DominatorResult {
    pub target: usize,
    pub roots: Vec<usize>,
    pub chain: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct DominatorIndex {
    pub roots: Vec<usize>,
    pub idom: Vec<Option<usize>>,
}

#[derive(Debug, Clone)]
pub enum DominatorPhase {
    BuildGraph,
    ReversePostorder,
    ComputeIdom,
    Done,
}

#[derive(Debug, Clone)]
pub struct DominatorProgress {
    pub phase: DominatorPhase,
    pub nodes_done: u64,
    pub nodes_total: u64,
    pub edges_done: u64,
    pub edges_total: u64,
    pub idom_iteration: u64,
}

pub fn dominator_chain(
    snapshot: &SnapshotRaw,
    target: usize,
    options: DominatorOptions,
) -> Result<DominatorResult, SnapshotError> {
    let index = compute_dominator_index(snapshot, options.cancel.clone(), options.progress)?;
    dominator_chain_from_index(&index, target, options.max_depth, options.cancel)
}

pub fn compute_dominator_index(
    snapshot: &SnapshotRaw,
    cancel: CancelToken,
    progress: Option<Sender<DominatorProgress>>,
) -> Result<DominatorIndex, SnapshotError> {
    let roots = find_roots(snapshot)?;
    let node_total = snapshot.node_count() as u64;
    let edge_total = snapshot.edge_count() as u64;

    let (succs, preds) = build_graph(snapshot, progress.as_ref(), node_total, edge_total)?;
    if cancel.is_cancelled() {
        return Err(SnapshotError::Cancelled);
    }

    let n = succs.len();
    let super_root = n;

    let mut succs_ext = succs;
    succs_ext.push(Vec::new());
    for &root in &roots {
        if root < n {
            succs_ext[super_root].push(root);
        }
    }

    let mut preds_ext = preds;
    preds_ext.push(Vec::new());
    for &root in &roots {
        if root < n {
            preds_ext[root].push(super_root);
        }
    }

    let lt = lengauer_tarjan(
        &succs_ext,
        &preds_ext,
        super_root,
        &cancel,
        progress.as_ref(),
        node_total,
        edge_total,
    )?;

    let mut idom = vec![None; n];
    for node in 0..n {
        if lt.dfs_num[node] == 0 {
            continue;
        }
        let dom = lt.idom[node];
        if dom == usize::MAX {
            continue;
        }
        if dom == super_root {
            idom[node] = Some(node);
        } else if dom < n {
            idom[node] = Some(dom);
        }
    }

    emit_progress(
        progress.as_ref(),
        DominatorProgress {
            phase: DominatorPhase::Done,
            nodes_done: node_total,
            nodes_total: node_total,
            edges_done: edge_total,
            edges_total: edge_total,
            idom_iteration: 0,
        },
    );

    Ok(DominatorIndex { roots, idom })
}

pub fn dominator_chain_from_index(
    index: &DominatorIndex,
    target: usize,
    max_depth: usize,
    cancel: CancelToken,
) -> Result<DominatorResult, SnapshotError> {
    let mut chain = Vec::new();
    let mut current = target;

    for _ in 0..=max_depth {
        if cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }
        chain.push(current);
        let next = match index.idom.get(current).copied().flatten() {
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
        roots: index.roots.clone(),
        chain,
    })
}

fn build_graph(
    snapshot: &SnapshotRaw,
    progress: Option<&Sender<DominatorProgress>>,
    nodes_total: u64,
    edges_total: u64,
) -> Result<(Vec<Vec<usize>>, Vec<Vec<usize>>), SnapshotError> {
    let node_count = snapshot.node_count();
    let mut succs = vec![Vec::new(); node_count];
    let mut preds = vec![Vec::new(); node_count];

    emit_progress(
        progress,
        DominatorProgress {
            phase: DominatorPhase::BuildGraph,
            nodes_done: 0,
            nodes_total,
            edges_done: 0,
            edges_total,
            idom_iteration: 0,
        },
    );

    let edge_offsets = compute_edge_offsets(snapshot)?;
    let mut processed_edges = 0u64;

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

        processed_edges = processed_edges.saturating_add(edge_count as u64);
        if node_index % 1024 == 0 || node_index + 1 == node_count {
            emit_progress(
                progress,
                DominatorProgress {
                    phase: DominatorPhase::BuildGraph,
                    nodes_done: (node_index + 1) as u64,
                    nodes_total,
                    edges_done: processed_edges,
                    edges_total,
                    idom_iteration: 0,
                },
            );
        }
    }

    Ok((succs, preds))
}

struct LtState {
    dfs_num: Vec<usize>,
    idom: Vec<usize>,
}

fn lengauer_tarjan(
    succs: &[Vec<usize>],
    preds: &[Vec<usize>],
    super_root: usize,
    cancel: &CancelToken,
    progress: Option<&Sender<DominatorProgress>>,
    nodes_total: u64,
    edges_total: u64,
) -> Result<LtState, SnapshotError> {
    let n = succs.len();
    let mut semi = vec![usize::MAX; n];
    let mut parent = vec![usize::MAX; n];
    let mut ancestor = vec![usize::MAX; n];
    let mut label: Vec<usize> = (0..n).collect();
    let mut dfs_num = vec![0usize; n];
    let mut vertex = vec![usize::MAX; n + 1];

    emit_progress(
        progress,
        DominatorProgress {
            phase: DominatorPhase::ReversePostorder,
            nodes_done: 0,
            nodes_total,
            edges_done: edges_total,
            edges_total,
            idom_iteration: 0,
        },
    );

    let mut time = 0usize;
    time += 1;
    dfs_num[super_root] = time;
    semi[super_root] = time;
    vertex[time] = super_root;
    parent[super_root] = super_root;

    let mut stack: Vec<(usize, usize)> = vec![(super_root, 0)];
    while let Some((node, idx)) = stack.pop() {
        if cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }
        if idx < succs[node].len() {
            stack.push((node, idx + 1));
            let next = succs[node][idx];
            if dfs_num[next] == 0 {
                time += 1;
                dfs_num[next] = time;
                semi[next] = time;
                vertex[time] = next;
                parent[next] = node;
                stack.push((next, 0));
                if time % 2048 == 0 {
                    emit_progress(
                        progress,
                        DominatorProgress {
                            phase: DominatorPhase::ReversePostorder,
                            nodes_done: time as u64,
                            nodes_total,
                            edges_done: edges_total,
                            edges_total,
                            idom_iteration: 0,
                        },
                    );
                }
            }
        }
    }

    let reachable = time;
    emit_progress(
        progress,
        DominatorProgress {
            phase: DominatorPhase::ReversePostorder,
            nodes_done: reachable as u64,
            nodes_total,
            edges_done: edges_total,
            edges_total,
            idom_iteration: 0,
        },
    );

    let mut idom = vec![usize::MAX; n];
    let mut bucket: Vec<Vec<usize>> = vec![Vec::new(); n];

    emit_progress(
        progress,
        DominatorProgress {
            phase: DominatorPhase::ComputeIdom,
            nodes_done: 0,
            nodes_total: reachable.saturating_sub(1) as u64,
            edges_done: edges_total,
            edges_total,
            idom_iteration: 0,
        },
    );

    for i in (2..=reachable).rev() {
        if cancel.is_cancelled() {
            return Err(SnapshotError::Cancelled);
        }

        let w = vertex[i];
        for &v in &preds[w] {
            if dfs_num[v] == 0 {
                continue;
            }
            let u = eval(v, &mut ancestor, &mut label, &semi);
            if semi[u] < semi[w] {
                semi[w] = semi[u];
            }
        }

        let semiv = vertex[semi[w]];
        if semiv < bucket.len() {
            bucket[semiv].push(w);
        }

        link(parent[w], w, &mut ancestor);
        let pw = parent[w];
        if pw < bucket.len() {
            let mut drained = Vec::new();
            std::mem::swap(&mut drained, &mut bucket[pw]);
            for v in drained {
                let u = eval(v, &mut ancestor, &mut label, &semi);
                if semi[u] < semi[v] {
                    idom[v] = u;
                } else {
                    idom[v] = pw;
                }
            }
        }

        let done = (reachable - i + 1) as u64;
        if done % 1024 == 0 || i == 2 {
            emit_progress(
                progress,
                DominatorProgress {
                    phase: DominatorPhase::ComputeIdom,
                    nodes_done: done,
                    nodes_total: reachable.saturating_sub(1) as u64,
                    edges_done: edges_total,
                    edges_total,
                    idom_iteration: 1,
                },
            );
        }
    }

    for i in 2..=reachable {
        let w = vertex[i];
        if idom[w] != vertex[semi[w]] {
            let parent_idom = idom[w];
            if parent_idom != usize::MAX {
                idom[w] = idom[parent_idom];
            }
        }
    }
    idom[super_root] = super_root;

    Ok(LtState { dfs_num, idom })
}

fn link(parent: usize, node: usize, ancestor: &mut [usize]) {
    ancestor[node] = parent;
}

fn eval(v: usize, ancestor: &mut [usize], label: &mut [usize], semi: &[usize]) -> usize {
    if ancestor[v] == usize::MAX {
        return label[v];
    }

    let mut path = Vec::new();
    let mut cur = v;
    while ancestor[cur] != usize::MAX && ancestor[ancestor[cur]] != usize::MAX {
        path.push(cur);
        cur = ancestor[cur];
    }

    while let Some(node) = path.pop() {
        let parent = ancestor[node];
        if semi[label[parent]] < semi[label[node]] {
            label[node] = label[parent];
        }
        ancestor[node] = ancestor[parent];
    }

    label[v]
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

fn emit_progress(progress: Option<&Sender<DominatorProgress>>, update: DominatorProgress) {
    if let Some(tx) = progress {
        let _ = tx.send(update);
    }
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
                progress: None,
            },
        )
        .expect("dominator");
        assert!(result.chain.len() >= 2);
    }
}
