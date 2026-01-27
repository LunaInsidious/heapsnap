use serde::Deserialize;

use crate::error::SnapshotError;

#[derive(Debug, Deserialize)]
pub struct SnapshotRoot {
    pub meta: Option<SnapshotMeta>,
}

#[derive(Debug, Deserialize)]
pub struct SnapshotMeta {
    pub node_fields: Vec<String>,
    pub node_types: Vec<MetaType>,
    pub edge_fields: Vec<String>,
    pub edge_types: Vec<MetaType>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum MetaType {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug)]
pub struct MetaIndex {
    pub node_type_names: Vec<String>,
    pub edge_type_names: Vec<String>,
    pub node_field_index: NodeFieldIndex,
    pub edge_field_index: EdgeFieldIndex,
    pub node_field_count: usize,
    pub edge_field_count: usize,
}

#[derive(Debug)]
pub struct NodeFieldIndex {
    pub type_idx: usize,
    pub name_idx: usize,
    pub id_idx: usize,
    pub self_size_idx: usize,
    pub edge_count_idx: usize,
}

#[derive(Debug)]
pub struct EdgeFieldIndex {
    pub type_idx: usize,
    pub name_or_index_idx: usize,
    pub to_node_idx: usize,
}

impl SnapshotMeta {
    pub fn validate(&self) -> Result<MetaIndex, SnapshotError> {
        let node_field_count = self.node_fields.len();
        let edge_field_count = self.edge_fields.len();

        if self.node_types.len() != node_field_count {
            return Err(SnapshotError::MetaMismatch {
                details: format!(
                    "node_types length ({}) does not match node_fields length ({})",
                    self.node_types.len(),
                    node_field_count
                ),
            });
        }
        if self.edge_types.len() != edge_field_count {
            return Err(SnapshotError::MetaMismatch {
                details: format!(
                    "edge_types length ({}) does not match edge_fields length ({})",
                    self.edge_types.len(),
                    edge_field_count
                ),
            });
        }

        let node_field_index = NodeFieldIndex {
            type_idx: find_field(&self.node_fields, "type")?,
            name_idx: find_field(&self.node_fields, "name")?,
            id_idx: find_field(&self.node_fields, "id")?,
            self_size_idx: find_field(&self.node_fields, "self_size")?,
            edge_count_idx: find_field(&self.node_fields, "edge_count")?,
        };

        let edge_field_index = EdgeFieldIndex {
            type_idx: find_field(&self.edge_fields, "type")?,
            name_or_index_idx: find_field(&self.edge_fields, "name_or_index")?,
            to_node_idx: find_field(&self.edge_fields, "to_node")?,
        };

        let node_type_names = match &self.node_types[node_field_index.type_idx] {
            MetaType::Array(values) => values.clone(),
            MetaType::String(value) => {
                return Err(SnapshotError::MetaMismatch {
                    details: format!(
                        "node_types[{}] expected array, got string ({value})",
                        node_field_index.type_idx
                    ),
                });
            }
        };

        let edge_type_names = match &self.edge_types[edge_field_index.type_idx] {
            MetaType::Array(values) => values.clone(),
            MetaType::String(value) => {
                return Err(SnapshotError::MetaMismatch {
                    details: format!(
                        "edge_types[{}] expected array, got string ({value})",
                        edge_field_index.type_idx
                    ),
                });
            }
        };

        Ok(MetaIndex {
            node_type_names,
            edge_type_names,
            node_field_index,
            edge_field_index,
            node_field_count,
            edge_field_count,
        })
    }
}

fn find_field(fields: &[String], name: &str) -> Result<usize, SnapshotError> {
    fields
        .iter()
        .position(|field| field == name)
        .ok_or_else(|| SnapshotError::MetaMismatch {
            details: format!("missing required field: {name}"),
        })
}

#[derive(Debug)]
pub struct SnapshotRaw {
    pub nodes: Vec<i64>,
    pub edges: Vec<i64>,
    pub strings: Vec<String>,
    pub meta: SnapshotMeta,
    pub index: MetaIndex,
}

impl SnapshotRaw {
    pub fn node_count(&self) -> usize {
        self.nodes.len() / self.index.node_field_count
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len() / self.index.edge_field_count
    }

    pub fn node_view(&self, node_index: usize) -> Option<NodeView<'_>> {
        if node_index >= self.node_count() {
            return None;
        }
        Some(NodeView {
            snapshot: self,
            node_index,
        })
    }

    pub fn edge_view(&self, edge_index: usize) -> Option<EdgeView<'_>> {
        if edge_index >= self.edge_count() {
            return None;
        }
        Some(EdgeView {
            snapshot: self,
            edge_index,
        })
    }

    pub fn memory_estimate_bytes(&self) -> u64 {
        let nodes_bytes = self.nodes.len() * std::mem::size_of::<i64>();
        let edges_bytes = self.edges.len() * std::mem::size_of::<i64>();
        let strings_bytes: usize = self.strings.iter().map(|s| s.capacity()).sum();
        (nodes_bytes + edges_bytes + strings_bytes) as u64
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NodeView<'a> {
    snapshot: &'a SnapshotRaw,
    node_index: usize,
}

impl<'a> NodeView<'a> {
    pub fn node_index(&self) -> usize {
        self.node_index
    }

    pub fn node_type(&self) -> Option<&'a str> {
        let idx = self.field_value(self.snapshot.index.node_field_index.type_idx)?;
        self.snapshot
            .index
            .node_type_names
            .get(idx as usize)
            .map(String::as_str)
    }

    pub fn name(&self) -> Option<&'a str> {
        let idx = self.field_value(self.snapshot.index.node_field_index.name_idx)?;
        self.snapshot.strings.get(idx as usize).map(String::as_str)
    }

    pub fn name_index(&self) -> Option<usize> {
        let idx = self.field_value(self.snapshot.index.node_field_index.name_idx)?;
        usize::try_from(idx).ok()
    }

    pub fn id(&self) -> Option<i64> {
        self.field_value(self.snapshot.index.node_field_index.id_idx)
    }

    pub fn self_size(&self) -> Option<i64> {
        self.field_value(self.snapshot.index.node_field_index.self_size_idx)
    }

    pub fn edge_count(&self) -> Option<i64> {
        self.field_value(self.snapshot.index.node_field_index.edge_count_idx)
    }

    fn field_value(&self, field_index: usize) -> Option<i64> {
        let base = self.node_index * self.snapshot.index.node_field_count;
        self.snapshot.nodes.get(base + field_index).copied()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EdgeView<'a> {
    snapshot: &'a SnapshotRaw,
    edge_index: usize,
}

impl<'a> EdgeView<'a> {
    pub fn edge_index(&self) -> usize {
        self.edge_index
    }

    pub fn edge_type(&self) -> Option<&'a str> {
        let idx = self.field_value(self.snapshot.index.edge_field_index.type_idx)?;
        self.snapshot
            .index
            .edge_type_names
            .get(idx as usize)
            .map(String::as_str)
    }

    pub fn name_or_index(&self) -> Option<i64> {
        self.field_value(self.snapshot.index.edge_field_index.name_or_index_idx)
    }

    pub fn to_node(&self) -> Option<i64> {
        self.field_value(self.snapshot.index.edge_field_index.to_node_idx)
    }

    pub fn to_node_index(&self) -> Option<usize> {
        let to_node = self.to_node()?;
        if to_node < 0 {
            return None;
        }
        let to_node = to_node as usize;
        if to_node % self.snapshot.index.node_field_count != 0 {
            return None;
        }
        Some(to_node / self.snapshot.index.node_field_count)
    }

    fn field_value(&self, field_index: usize) -> Option<i64> {
        let base = self.edge_index * self.snapshot.index.edge_field_count;
        self.snapshot.edges.get(base + field_index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_meta_indexes() {
        let meta = SnapshotMeta {
            node_fields: vec![
                "type".to_string(),
                "name".to_string(),
                "id".to_string(),
                "self_size".to_string(),
                "edge_count".to_string(),
            ],
            node_types: vec![
                MetaType::Array(vec!["object".to_string(), "string".to_string()]),
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
                MetaType::Array(vec!["property".to_string(), "element".to_string()]),
                MetaType::String("string_or_number".to_string()),
                MetaType::String("node".to_string()),
            ],
        };

        let index = meta.validate().expect("meta valid");
        assert_eq!(index.node_field_count, 5);
        assert_eq!(index.edge_field_count, 3);
        assert_eq!(index.node_type_names.len(), 2);
        assert_eq!(index.edge_type_names.len(), 2);
    }
}
