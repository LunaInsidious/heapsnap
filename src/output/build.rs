use serde::Serialize;

use crate::error::SnapshotError;
use crate::snapshot::SnapshotRaw;

#[derive(Debug, Serialize)]
pub struct BuildMeta {
    pub version: u32,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub total_strings: usize,
}

impl BuildMeta {
    pub fn from_snapshot(snapshot: &SnapshotRaw) -> Self {
        Self {
            version: 1,
            total_nodes: snapshot.node_count(),
            total_edges: snapshot.edge_count(),
            total_strings: snapshot.strings.len(),
        }
    }

    pub fn to_json(&self) -> Result<String, SnapshotError> {
        serde_json::to_string_pretty(self).map_err(SnapshotError::Json)
    }
}
