# JSON Schema (v1)

このドキュメントは CLI 出力 JSON の **安定スキーマ**を定義する。
将来的な拡張は `version` の更新で管理する。

---

## Summary

```json
{
  "version": 1,
  "total_nodes": 123,
  "rows": [
    { "name": "Foo", "count": 10, "self_size_sum_bytes": 2048 }
  ]
}
```

### Fields

- `version` (number): スキーマバージョン
- `total_nodes` (number): snapshot 内の総ノード数
- `rows` (array):
  - `name` (string): constructor 名
  - `count` (number): インスタンス数
  - `self_size_sum_bytes` (number): self size 合計（bytes）

---

## Retainers

```json
{
  "version": 1,
  "target": { "index": 10, "id": 12345, "name": "FooStore", "node_type": "object" },
  "paths": [
    {
      "steps": [
        {
          "from": { "index": 0, "id": 1, "name": "GC roots", "node_type": "synthetic" },
          "edge": { "index": 5, "edge_type": "property", "name_or_index": 42, "name": "__APP__" },
          "to": { "index": 10, "id": 12345, "name": "FooStore", "node_type": "object" }
        }
      ]
    }
  ]
}
```

### Fields

- `version` (number): スキーマバージョン
- `target` (object): 対象ノード
  - `index` (number): node index（nodes 配列内の位置）
  - `id` (number | null): node id（存在する場合）
  - `name` (string | null): constructor 名（存在する場合）
  - `node_type` (string | null): node type 名（存在する場合）
- `paths` (array):
  - `steps` (array): root から target への経路
    - `from` / `to`: Node 情報（`target` と同形式）
    - `edge`:
      - `index` (number): edge index（edges 配列内の位置）
      - `edge_type` (string | null): edge type 名
      - `name_or_index` (number | null): 元の `name_or_index` 値
      - `name` (string | null): 解決後の edge 名（property の場合）または `element` 表記

---

## Diff

```json
{
  "version": 1,
  "total_nodes_a": 100,
  "total_nodes_b": 120,
  "rows": [
    {
      "name": "Foo",
      "count_a": 10,
      "count_b": 12,
      "count_delta": 2,
      "self_size_sum_a_bytes": 2048,
      "self_size_sum_b_bytes": 3072,
      "self_size_sum_delta_bytes": 1024
    }
  ]
}
```

### Fields

- `version` (number): スキーマバージョン
- `total_nodes_a` / `total_nodes_b` (number): A/B の総ノード数
- `rows` (array):
  - `name` (string): constructor 名
  - `count_a` / `count_b` (number)
  - `count_delta` (number)
  - `self_size_sum_a_bytes` / `self_size_sum_b_bytes` (number): self size 合計（bytes）
  - `self_size_sum_delta_bytes` (number): self size 合計差分（bytes）

---

## Build meta.json

```json
{
  "version": 1,
  "total_nodes": 100,
  "total_edges": 250,
  "total_strings": 42
}
```

### Fields

- `version` (number): スキーマバージョン
- `total_nodes` (number)
- `total_edges` (number)
- `total_strings` (number)

---

## Dominator

```json
{
  "version": 1,
  "target": { "index": 10, "id": 12345, "name": "FooStore", "node_type": "object" },
  "chain": [
    { "index": 0, "id": 1, "name": "GC roots", "node_type": "synthetic" }
  ]
}
```

### Fields

- `version` (number): スキーマバージョン
- `target` (object): 対象ノード
- `chain` (array): root から target への dominator chain

---

## Detail

### By name

```json
{
  "version": 1,
  "mode": "name",
  "name": "Foo",
  "constructor_summary": {
    "total_count": 10,
    "self_size_sum_bytes": 2048,
    "max_self_size_bytes": 512,
    "min_self_size_bytes": 64,
    "avg_self_size_bytes": 204.8,
    "skip": 0,
    "limit": 200,
    "total_ids": 10
  },
  "ids": [
    { "index": 1, "id": 2, "node_type": "object", "self_size_bytes": 128 }
  ]
}
```

### By id

```json
{
  "version": 1,
  "mode": "id",
  "name": "Foo",
  "id": 123,
  "node_type": "object",
  "self_size_bytes": 128,
  "constructor_summary": {
    "total_count": 10,
    "self_size_sum_bytes": 2048,
    "max_self_size_bytes": 512,
    "min_self_size_bytes": 64,
    "avg_self_size_bytes": 204.8,
    "skip": 0,
    "limit": 200,
    "total_ids": 10
  },
  "ids": [
    { "index": 1, "id": 2, "node_type": "object", "self_size_bytes": 128 }
  ],
  "retainers": [
    {
      "from_index": 0,
      "from_id": 1,
      "from_name": "GC roots",
      "from_node_type": "synthetic",
      "from_self_size_bytes": 0,
      "edge_index": 0,
      "edge_type": "property",
      "edge_name": "__APP__"
    }
  ],
  "outgoing_edges": [
    {
      "edge_index": 1,
      "edge_type": "property",
      "edge_name": "store",
      "to_index": 2,
      "to_id": 3,
      "to_name": "Bar",
      "to_node_type": "object",
      "to_self_size_bytes": 64
    }
  ],
  "shallow_size_distribution": [
    { "label": "0", "min": 0, "max": 0, "count": 1 },
    { "label": "1-31", "min": 1, "max": 31, "count": 0 }
  ]
}
```

### Fields

- `version` (number): スキーマバージョン
- `mode` ("name" | "id")
- `name` (string): constructor 名
- `id` (number | null): node id（idモードのみ）
- `node_type` (string | null): node type 名（idモードのみ）
- `self_size_bytes` (number | null): 対象ノードの self size（idモードのみ）
- `constructor_summary` (object):
  - `total_count` (number)
  - `self_size_sum_bytes` (number)
  - `max_self_size_bytes` (number)
  - `min_self_size_bytes` (number)
  - `avg_self_size_bytes` (number)
  - `skip` / `limit` (number): id 一覧のページング
  - `total_ids` (number)
- `ids` (array): id 一覧
  - `index` (number)
  - `id` (number | null)
  - `node_type` (string | null)
  - `self_size_bytes` (number)
- `retainers` (array): retainers 上位
  - `from_*` (number/string | null)
  - `edge_*` (number/string | null)
- `outgoing_edges` (array): outgoing edges 上位
  - `edge_*` / `to_*` (number/string | null)
- `shallow_size_distribution` (array): shallow size 分布
  - `label` (string), `min` (number), `max` (number | null), `count` (number)
