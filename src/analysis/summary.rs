use std::collections::HashMap;

use serde::Serialize;

use crate::error::SnapshotError;
use crate::snapshot::SnapshotRaw;

#[derive(Debug)]
pub struct SummaryOptions {
    pub top: usize,
    pub contains: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SummaryRow {
    pub name: String,
    pub count: u64,
    pub self_size_sum: i64,
}

#[derive(Debug, Serialize)]
pub struct SummaryResult {
    pub total_nodes: usize,
    pub rows: Vec<SummaryRow>,
    #[serde(skip)]
    pub empty_name_types: Vec<EmptyTypeSummary>,
}

#[derive(Debug, Clone)]
pub struct EmptyTypeSummary {
    pub node_type: String,
    pub count: u64,
    pub self_size_sum: i64,
}

pub fn summarize(
    snapshot: &SnapshotRaw,
    options: SummaryOptions,
) -> Result<SummaryResult, SnapshotError> {
    let mut map: HashMap<usize, SummaryRow> = HashMap::new();
    let mut empty_types: HashMap<String, EmptyTypeSummary> = HashMap::new();

    for index in 0..snapshot.node_count() {
        let node = snapshot
            .node_view(index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("node index out of range: {index}"),
            })?;
        let name_index = match node.name_index() {
            Some(value) => value,
            None => {
                return Err(SnapshotError::InvalidData {
                    details: format!("node missing name index: {index}"),
                });
            }
        };

        let name = snapshot
            .strings
            .get(name_index)
            .ok_or_else(|| SnapshotError::InvalidData {
                details: format!("name index out of range: {name_index}"),
            })?;

        if let Some(filter) = options.contains.as_deref() {
            if !name.contains(filter) {
                continue;
            }
        }

        let entry = map.entry(name_index).or_insert_with(|| SummaryRow {
            name: name.to_string(),
            count: 0,
            self_size_sum: 0,
        });

        entry.count += 1;
        entry.self_size_sum += node.self_size().unwrap_or(0);

        if name.is_empty() {
            let node_type = node.node_type().unwrap_or("unknown");
            let type_entry =
                empty_types
                    .entry(node_type.to_string())
                    .or_insert_with(|| EmptyTypeSummary {
                        node_type: node_type.to_string(),
                        count: 0,
                        self_size_sum: 0,
                    });
            type_entry.count += 1;
            type_entry.self_size_sum += node.self_size().unwrap_or(0);
        }
    }

    let mut rows: Vec<SummaryRow> = map.into_values().collect();
    rows.sort_by(|a, b| {
        b.self_size_sum
            .cmp(&a.self_size_sum)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.name.cmp(&b.name))
    });

    if rows.len() > options.top {
        rows.truncate(options.top);
    }

    let mut empty_name_types: Vec<EmptyTypeSummary> = empty_types.into_values().collect();
    empty_name_types.sort_by(|a, b| {
        b.self_size_sum
            .cmp(&a.self_size_sum)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.node_type.cmp(&b.node_type))
    });

    Ok(SummaryResult {
        total_nodes: snapshot.node_count(),
        rows,
        empty_name_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{MetaType, SnapshotMeta, SnapshotRaw};

    fn minimal_snapshot() -> SnapshotRaw {
        let meta = SnapshotMeta {
            node_fields: vec![
                "type".to_string(),
                "name".to_string(),
                "id".to_string(),
                "self_size".to_string(),
                "edge_count".to_string(),
            ],
            node_types: vec![
                MetaType::Array(vec!["object".to_string()]),
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
        let index = meta.validate().expect("meta valid");

        SnapshotRaw {
            nodes: vec![
                0, 0, 1, 10, 0, // node 0: name index 0
                0, 1, 2, 20, 0, // node 1: name index 1
                0, 0, 3, 5, 0, // node 2: name index 0
            ],
            edges: vec![],
            strings: vec!["Foo".to_string(), "Bar".to_string()],
            meta,
            index,
        }
    }

    #[test]
    fn summarize_counts() {
        let snapshot = minimal_snapshot();
        let result = summarize(
            &snapshot,
            SummaryOptions {
                top: 10,
                contains: None,
            },
        )
        .expect("summary");

        assert_eq!(result.total_nodes, 3);
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0].name, "Bar");
        assert_eq!(result.rows[0].self_size_sum, 20);
        assert_eq!(result.rows[1].name, "Foo");
        assert_eq!(result.rows[1].count, 2);
    }

    #[test]
    fn summarize_contains_filter_matches_partial() {
        let snapshot = minimal_snapshot();
        let result = summarize(
            &snapshot,
            SummaryOptions {
                top: 10,
                contains: Some("Fo".to_string()),
            },
        )
        .expect("summary");

        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].name, "Foo");
        assert_eq!(result.rows[0].count, 2);
    }

    #[test]
    fn summarize_contains_filter_is_case_sensitive() {
        let snapshot = minimal_snapshot();
        let result = summarize(
            &snapshot,
            SummaryOptions {
                top: 10,
                contains: Some("foo".to_string()),
            },
        )
        .expect("summary");

        assert!(result.rows.is_empty());
    }
}
