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
    { "name": "Foo", "count": 10, "self_size_sum": 2048 }
  ]
}
```

### Fields

- `version` (number): スキーマバージョン
- `total_nodes` (number): snapshot 内の総ノード数
- `rows` (array):
  - `name` (string): constructor 名
  - `count` (number): インスタンス数
  - `self_size_sum` (number): self size 合計

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
      "self_size_sum_a": 2048,
      "self_size_sum_b": 3072,
      "self_size_sum_delta": 1024
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
  - `self_size_sum_a` / `self_size_sum_b` (number)
  - `self_size_sum_delta` (number)

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
