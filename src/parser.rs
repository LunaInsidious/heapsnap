use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};

use crate::cancel::CancelToken;
use crate::error::SnapshotError;
use crate::lenient::LenientJsonReader;
use crate::progress::ProgressReader;
use crate::snapshot::{SnapshotMeta, SnapshotRaw, SnapshotRoot};

pub struct ReadOptions {
    pub progress: bool,
    pub cancel: CancelToken,
}

impl ReadOptions {
    pub fn new(progress: bool, cancel: CancelToken) -> Self {
        Self { progress, cancel }
    }
}

pub fn read_snapshot_file(path: &Path, options: ReadOptions) -> Result<SnapshotRaw, SnapshotError> {
    let file = File::open(path)?;
    let total = file.metadata().ok().map(|metadata| metadata.len());
    let reader = BufReader::new(file);
    let mut progress_reader = ProgressReader::new(reader, options.progress, total, options.cancel);
    let snapshot = read_snapshot(&mut progress_reader)?;
    progress_reader.finish();
    Ok(snapshot)
}

pub fn read_snapshot<R: Read>(reader: &mut R) -> Result<SnapshotRaw, SnapshotError> {
    let mut lenient = LenientJsonReader::new(reader);
    let mut deserializer = serde_json::Deserializer::from_reader(&mut lenient);
    let mut visitor = SnapshotVisitor::default();
    match deserializer.deserialize_map(&mut visitor) {
        Ok(()) => visitor.into_snapshot(),
        Err(err) => Err(map_json_error(err)),
    }
}

#[derive(Default)]
struct SnapshotVisitor {
    meta: Option<SnapshotMeta>,
    nodes: Vec<i64>,
    edges: Vec<i64>,
    strings: Vec<String>,
}

impl SnapshotVisitor {
    fn into_snapshot(self) -> Result<SnapshotRaw, SnapshotError> {
        let meta = self.meta.ok_or_else(|| SnapshotError::InvalidData {
            details:
                "missing snapshot.meta (ensure the file is a Chrome DevTools heapsnapshot)".to_string(),
        })?;
        let index = meta.validate()?;

        if self.nodes.len() % index.node_field_count != 0 {
            return Err(SnapshotError::InvalidData {
                details: format!(
                    "nodes length ({}) is not divisible by node field count ({})",
                    self.nodes.len(),
                    index.node_field_count
                ),
            });
        }
        if self.edges.len() % index.edge_field_count != 0 {
            return Err(SnapshotError::InvalidData {
                details: format!(
                    "edges length ({}) is not divisible by edge field count ({})",
                    self.edges.len(),
                    index.edge_field_count
                ),
            });
        }

        Ok(SnapshotRaw {
            nodes: self.nodes,
            edges: self.edges,
            strings: self.strings,
            meta,
            index,
        })
    }
}

impl<'de> Visitor<'de> for &mut SnapshotVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("heapsnapshot top-level object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "snapshot" => {
                    let root = map.next_value::<SnapshotRoot>()?;
                    if let Some(meta) = root.meta {
                        self.meta = Some(meta);
                    }
                }
                "nodes" => {
                    map.next_value_seed(I64VecSeed(&mut self.nodes))?;
                }
                "edges" => {
                    map.next_value_seed(I64VecSeed(&mut self.edges))?;
                }
                "strings" => {
                    map.next_value_seed(StringVecSeed(&mut self.strings))?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

struct I64VecSeed<'a>(&'a mut Vec<i64>);

impl<'de, 'a> DeserializeSeed<'de> for I64VecSeed<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(I64VecVisitor(self.0))
    }
}

struct I64VecVisitor<'a>(&'a mut Vec<i64>);

impl<'de, 'a> Visitor<'de> for I64VecVisitor<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("array of integers")
    }

    fn visit_seq<M>(self, mut seq: M) -> Result<Self::Value, M::Error>
    where
        M: serde::de::SeqAccess<'de>,
    {
        while let Some(value) = seq.next_element::<i64>()? {
            self.0.push(value);
        }
        Ok(())
    }
}

struct StringVecSeed<'a>(&'a mut Vec<String>);

impl<'de, 'a> DeserializeSeed<'de> for StringVecSeed<'a> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(StringVecVisitor(self.0))
    }
}

struct StringVecVisitor<'a>(&'a mut Vec<String>);

impl<'de, 'a> Visitor<'de> for StringVecVisitor<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("array of strings")
    }

    fn visit_seq<M>(self, mut seq: M) -> Result<Self::Value, M::Error>
    where
        M: serde::de::SeqAccess<'de>,
    {
        while let Some(value) = seq.next_element::<String>()? {
            self.0.push(value);
        }
        Ok(())
    }
}

fn map_json_error(err: serde_json::Error) -> SnapshotError {
    if err.io_error_kind() == Some(std::io::ErrorKind::Interrupted) {
        return SnapshotError::Cancelled;
    }
    if err.is_io() && err.to_string().contains("cancelled") {
        return SnapshotError::Cancelled;
    }
    SnapshotError::Json(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_snapshot() {
        let json = r#"
        {
          "snapshot": {
            "meta": {
              "node_fields": ["type","name","id","self_size","edge_count"],
              "node_types": [
                ["object","string"],
                "string",
                "number",
                "number",
                "number"
              ],
              "edge_fields": ["type","name_or_index","to_node"],
              "edge_types": [
                ["property","element"],
                "string_or_number",
                "node"
              ]
            }
          },
          "nodes": [0, 0, 1, 10, 0],
          "edges": [],
          "strings": ["Root"]
        }
        "#;

        let mut reader = json.as_bytes();
        let snapshot = read_snapshot(&mut reader).expect("parse ok");
        assert_eq!(snapshot.node_count(), 1);
        let node = snapshot.node_view(0).expect("node");
        assert_eq!(node.node_type(), Some("object"));
        assert_eq!(node.name(), Some("Root"));
        assert_eq!(node.id(), Some(1));
        assert_eq!(node.self_size(), Some(10));
    }

    #[test]
    fn parse_lone_surrogate() {
        let json = r#"
        {
          "snapshot": {
            "meta": {
              "node_fields": ["type","name","id","self_size","edge_count"],
              "node_types": [
                ["object"],
                "string",
                "number",
                "number",
                "number"
              ],
              "edge_fields": ["type","name_or_index","to_node"],
              "edge_types": [
                ["property"],
                "string_or_number",
                "node"
              ]
            }
          },
          "nodes": [0, 0, 1, 10, 0],
          "edges": [],
          "strings": ["\uD800"]
        }
        "#;

        let mut reader = json.as_bytes();
        let snapshot = read_snapshot(&mut reader).expect("parse ok");
        assert_eq!(snapshot.strings[0], "\u{FFFD}");
    }
}
