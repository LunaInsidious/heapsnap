use std::fmt::Write as _;

use serde::Serialize;

use crate::analysis::dominator::DominatorResult;
use crate::error::SnapshotError;
use crate::snapshot::SnapshotRaw;

#[derive(Debug, Serialize)]
struct DominatorJson {
    version: u32,
    target: NodeJson,
    chain: Vec<NodeJson>,
}

#[derive(Debug, Serialize)]
struct NodeJson {
    index: usize,
    id: Option<i64>,
    name: Option<String>,
    node_type: Option<String>,
}

pub fn format_markdown(snapshot: &SnapshotRaw, result: &DominatorResult) -> String {
    let mut output = String::new();
    let target = snapshot.node_view(result.target);
    let target_name = target.and_then(|node| node.name()).unwrap_or("<unknown>");
    let target_id = target.and_then(|node| node.id()).unwrap_or(-1);
    let _ = writeln!(output, "- Dominator chain for {target_name} (id={target_id})");
    for (idx, node_index) in result.chain.iter().enumerate() {
        let node = snapshot.node_view(*node_index);
        let name = node.and_then(|value| value.name()).unwrap_or("<unknown>");
        let id = node.and_then(|value| value.id()).unwrap_or(-1);
        let _ = writeln!(output, "  - #{} {} (id={})", idx + 1, name, id);
    }
    output
}

pub fn format_json(snapshot: &SnapshotRaw, result: &DominatorResult) -> Result<String, SnapshotError> {
    let payload = DominatorJson {
        version: 1,
        target: node_json(snapshot, result.target),
        chain: result
            .chain
            .iter()
            .map(|index| node_json(snapshot, *index))
            .collect(),
    };
    serde_json::to_string_pretty(&payload).map_err(SnapshotError::Json)
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
