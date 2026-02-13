# MEMO — 開発メモ・調査記録

このドキュメントは **決定には至っていない情報**や
**後で振り返りたい知見**を残すためのメモ置き場である。

- 正確さよりも「忘れないこと」を優先
- 未検証・仮説・違和感を書いてよい
- 決定事項になったら ADR に昇格させる

---

## YYYY-MM-DD: タイトル（自由）

### 背景
なぜこのメモを書いたか。
調査・実装中に気づいたことなど。

---

### 内容 / 観察結果
- 調査した事実
- heapsnapshot の構造メモ
- 試して分かった挙動
- DevTools と実データの差異

※ 未確認事項はその旨を明記すること。

---

### 気になる点 / TODO
- 今は対応しないが、将来問題になりそうな点
- 実装を進める上での懸念

---

### 備考
- 関連するコード位置
- 関連する Issue / ADR 番号

---

## 2026-01-27: heapsnapshot meta の前提メモ

### 背景
meta の構造は Chrome バージョンで変化する可能性があり、
最小限の前提だけでパースする必要がある。

### 内容 / 観察結果
- `node_fields` / `edge_fields` に最低限必要なキーを要求する前提で実装している
- `node_types` / `edge_types` は `*_fields` と同じ長さである想定
- `type` フィールドの `*_types` が配列である想定（型名の一覧を参照するため）

※ 未確認事項: 実データで `type` 以外の `*_types` が配列になるケース。

### 気になる点 / TODO
- 実データを観察して `node_fields` / `edge_fields` の必須集合を調整する可能性

### 備考
- 関連コード: `src/snapshot.rs`, `src/parser.rs`

---

## 2026-01-27: Retainers のルート判定と name マッチ方針

### 背景
retainers 探索では GC Root を特定する必要があるが、
heapsnapshot の実データにばらつきがあり得る。

### 内容 / 観察結果
- `GC roots` という名前を持つノードを root として扱う前提で実装
- 見つからない場合は node index 0 を暫定 root として扱う
- `--name` は部分一致として候補を集計し、`--pick` で constructor を選択する

※ 未確認事項: 実データで `GC roots` が存在しないケースの頻度。

### 気になる点 / TODO
- 実データを観察して root 判定ロジックを調整する可能性

### 備考
- 関連コード: `src/analysis/retainers.rs`

---

## 2026-01-27: JSON 文字列の不正なサロゲート対策

### 背景
実データの heapsnapshot に、JSON としては不正な
`\\uD800` などの単独サロゲートが含まれるケースがあった。

### 内容 / 観察結果
- `serde_json` は単独サロゲートをエラーとして扱う
- Lenient な変換で `\\uFFFD` に置換するとパース可能になる

### 気になる点 / TODO
- 変換が許容されるかどうかは実データで確認が必要

### 備考
- 関連コード: `src/lenient.rs`, `src/parser.rs`

---

## 2026-01-27: Markdown 出力の長文省略と展開

### 背景
constructor 名や edge 名に長いコード断片が含まれると
Markdown テーブルやリストの可読性が落ちる。

### 内容 / 観察結果
- Markdown 出力では一定長以上の文字列を省略し、`<details>` で展開可能にした
- 省略はデフォルトで有効

### 気になる点 / TODO
- 省略長の調整や CLI オプション化を検討する余地がある

### 備考
- 関連コード: `src/output/summary.rs`, `src/output/retainers.rs`

---

## 2026-02-13: detail の集計・並び順・分布仕様メモ

### 背景
detail コマンドで表示する retainers / outgoing edges / shallow size 分布の
並び順と区切りはスナップショットに依存しない固定仕様が必要。

### 内容 / 観察結果
- retainers は `from_node` の `self_size` 降順で上位 N を表示する
- outgoing edges は `to_node` の `self_size` 降順で上位 N を表示する
- shallow size 分布は固定バケットで集計する  
  `0`, `1-31`, `32-127`, `128-511`, `512-2047`, `2048-8191`, `8192-32767`, `32768+`

### 気になる点 / TODO
- バケット境界や並び順は UI の要望に合わせて見直す余地がある

### 備考
- 関連コード: `src/analysis/detail.rs`, `src/output/detail.rs`
