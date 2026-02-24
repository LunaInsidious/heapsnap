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

---

## 2026-02-19: serve diff の file upload 経路メモ

### 背景
`heapsnap serve <file>` 起動後に、index から diff へ遷移し、
ブラウザの `input type="file"` で比較対象を選択したい要件が追加された。

### 内容 / 観察結果
- `GET /diff` はアップロードフォームを表示する
- `POST /diff` は `multipart/form-data` を受け取り、`after` フィールドのファイルを解析する
- アップロードされた内容は一時ファイル（`$TMPDIR/heapsnap-serve/`）に保存し、
  `before=<serve起動時ファイル>` と `after=<一時ファイル>` で既存 diff 描画処理を再利用する
- ブラウザ画面には upload 進捗（%）を表示し、upload 完了後は解析中メッセージを表示する
  （解析そのものはサーバ側同期処理のため、件数ベースの厳密な進捗率は未提供）
- upload された `after` ファイルはフィルタ再適用（skip/limit/search/top）で再利用するため
  即時削除せず、`serve` 停止時に一括掃除する
- `/diff` は `before` の起動時 snapshot 再利用・`after` snapshot のパスキャッシュ・
  diff結果キャッシュ（before/after/top/search）を追加し、再解析時間を短縮した

### 気になる点 / TODO
- 一時ファイルの掃除戦略（TTL / 明示削除）の整理は今後の改善余地

### 備考
- 関連コード: `src/serve.rs`

---

## 2026-02-19: serve の Ctrl-C 応答改善メモ

### 背景
`heapsnap serve` 実行中に Ctrl-C で終了できる時とできない時があった。

### 内容 / 観察結果
- 接続処理中の `read` がブロッキングすると、キャンセルフラグを即時確認できない
- `serve` 内の一部重い処理（diff/retainers/dominator）で `CancelToken::new()` を使っており、Ctrl-C が処理に伝播しない
- `ServerContext` に cancel token を保持し、read timeout + ループ内キャンセル確認を追加した
- diff/retainers/dominator の処理に `context.cancel` を渡すよう統一した

### 備考
- 関連コード: `src/serve.rs`, `src/cancel.rs`

---

## 2026-02-19: serve dominator の初回表示遅延対策メモ

### 背景
`/dominator` 初回アクセス時に計算が重く、画面が返ってこない体感があった。

### 内容 / 観察結果
- 初回アクセスは同期計算を行わず、バックグラウンドジョブを開始して
  「calculating」画面を即時返すようにした
- 進捗は SSE (`/dominator/events`) で配信し、完了イベントで画面更新する
- 同一 session で再 Apply 時は旧ジョブを cancel して新ジョブを開始する
- 進捗表示は経過時間ベースではなく、解析処理で実際に走査した node/edge 数を使う
- `id -> node_index` インデックスを起動時に構築し、`id` 検索の全走査を避けた
- `/dominator` 表示時は URL に `session` を固定し、ブラウザ reload で別ジョブが増殖しないようにした
- 同一 session で新条件を Apply した時は旧ジョブを cancel し、ジョブマップから削除する
- SSE (`/dominator/events`) は接続を長く保持するため、サーバ接続処理をリクエストごとスレッド化して
  1 接続で accept ループ全体が詰まる問題を回避した
- Dominator の中核アルゴリズムは Cooper から Lengauer-Tarjan へ変更した
- `serve` は初回 dominator 計算で得た index（roots + idom）をキャッシュし、
  2回目以降は chain 抽出のみ実行する

### 備考
- 関連コード: `src/serve.rs`, `src/analysis/dominator.rs`
