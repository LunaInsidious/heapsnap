use std::collections::HashMap;

use serde::Serialize;

use crate::analysis::summary::{summarize, SummaryOptions, SummaryRow};
use crate::error::SnapshotError;
use crate::snapshot::SnapshotRaw;

#[derive(Debug)]
pub struct DiffOptions {
    pub top: usize,
    pub contains: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiffRow {
    pub name: String,
    pub count_a: u64,
    pub count_b: u64,
    pub count_delta: i64,
    pub self_size_sum_a: i64,
    pub self_size_sum_b: i64,
    pub self_size_sum_delta: i64,
}

#[derive(Debug, Serialize)]
pub struct DiffResult {
    pub total_nodes_a: usize,
    pub total_nodes_b: usize,
    pub rows: Vec<DiffRow>,
}

pub fn diff_summaries(
    snapshot_a: &SnapshotRaw,
    snapshot_b: &SnapshotRaw,
    options: DiffOptions,
) -> Result<DiffResult, SnapshotError> {
    let summary_a = summarize(
        snapshot_a,
        SummaryOptions {
            top: usize::MAX,
            contains: None,
        },
    )?;
    let summary_b = summarize(
        snapshot_b,
        SummaryOptions {
            top: usize::MAX,
            contains: None,
        },
    )?;

    let map_a = map_by_name(&summary_a.rows);
    let map_b = map_by_name(&summary_b.rows);

    let mut names: Vec<String> = map_a
        .keys()
        .chain(map_b.keys())
        .cloned()
        .collect();
    names.sort();
    names.dedup();

    let mut rows = Vec::new();
    for name in names {
        if let Some(filter) = options.contains.as_deref() {
            if !name.contains(filter) {
                continue;
            }
        }
        let row_a = map_a.get(&name);
        let row_b = map_b.get(&name);
        let count_a = row_a.map(|r| r.count).unwrap_or(0);
        let count_b = row_b.map(|r| r.count).unwrap_or(0);
        let self_size_sum_a = row_a.map(|r| r.self_size_sum).unwrap_or(0);
        let self_size_sum_b = row_b.map(|r| r.self_size_sum).unwrap_or(0);
        rows.push(DiffRow {
            name,
            count_a,
            count_b,
            count_delta: count_b as i64 - count_a as i64,
            self_size_sum_a,
            self_size_sum_b,
            self_size_sum_delta: self_size_sum_b - self_size_sum_a,
        });
    }

    rows.sort_by(|a, b| {
        b.self_size_sum_delta
            .abs()
            .cmp(&a.self_size_sum_delta.abs())
            .then_with(|| b.count_delta.abs().cmp(&a.count_delta.abs()))
            .then_with(|| a.name.cmp(&b.name))
    });

    if rows.len() > options.top {
        rows.truncate(options.top);
    }

    Ok(DiffResult {
        total_nodes_a: summary_a.total_nodes,
        total_nodes_b: summary_b.total_nodes,
        rows,
    })
}

fn map_by_name(rows: &[SummaryRow]) -> HashMap<String, SummaryRow> {
    rows.iter()
        .map(|row| (row.name.clone(), SummaryRow {
            name: row.name.clone(),
            count: row.count,
            self_size_sum: row.self_size_sum,
        }))
        .collect()
}
