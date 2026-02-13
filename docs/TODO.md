# TODO.md — HeapSnapshot CLI Analyzer (Rust)

このTODOは **上から順に実装すればMVPが完成**するように並べてある。
各項目は「完了条件」を満たしたらチェックを付ける。

---

## 0. リポジトリ初期化

- [x] Cargo workspace or single crate を決定（MVPは single crate 推奨）
  - 完了条件: `cargo test` が通る
- [x] `clap` を導入して `heapsnap --help` を出せる
  - 完了条件: `heapsnap summary --help` `heapsnap retainers --help` が表示される
- [x] CI（最低限 `cargo fmt` / `cargo test`）を設定
  - 完了条件: PR/ローカルで自動実行できる

---

## 1. 基本方針の固定（コードに落とす“契約”）

- [x] ネットワーク通信をしない方針を README とコードコメントに明記
  - 完了条件: README に「外部通信しない」「ローカル完結」を明記
- [x] ログの方針を決める（デフォルト最小、`--verbose` で詳細）
  - 完了条件: `--verbose` なしで個人情報/大量文字列が出ない

---

## 2. パーサ基盤（最重要）

### 2.1 heapsnapshot の最小読み取り（meta確認）
- [x] `snapshot.meta` を読み取り、nodes/edges のフィールド定義（型・順序）を取得
  - 完了条件: metaの要点（node_fields, edge_fields, node_types, edge_types など）を構造体に格納できる
- [x] meta 互換性チェックを実装（想定外なら明示的エラー）
  - 完了条件: 異なるmeta入力で「何が違うか」付きでエラーになる

### 2.2 ストリーミング JSON パース
- [x] `nodes` / `edges` を **一括デシリアライズせず** 生配列として読み込む
  - 完了条件: 数百MB級で OOM せずに読み込みが進む
- [x] `strings` を読み込む（必要最小限で保持）
  - 完了条件: node/edge が参照する string index を name に解決できる

### 2.3 内部表現（Raw + View）
- [x] `SnapshotRaw { nodes: Vec<i64>, edges: Vec<i64>, strings: Vec<String>, meta }`
  - 完了条件: `SnapshotRaw` を生成し、参照できる
- [x] `NodeView` / `EdgeView` を実装（インデックス参照でフィールドを読む）
  - 完了条件: 任意ノードの `type/name/self_size/edge_count` が取得できる

### 2.4 基本ユーティリティ
- [x] 進捗表示（`--progress` デフォルトON）
  - 完了条件: 大きいファイルで進捗%または処理段階が表示される
- [x] Ctrl-C 中断の安全停止（途中でも破損出力を残さない）
  - 完了条件: 中断しても次回実行に影響しない（tempファイル掃除）

---

## 3. Summary 機能（MVPの先頭）

### 3.1 集計ロジック
- [x] constructor/name ごとの `count` と `self_size_sum` を集計
  - 完了条件: 上位Kを出せる（K=50など）
- [x] フィルタ/検索（`--contains` など最小でOK）
  - 完了条件: 特定文字列を含むconstructorだけ表示できる

### 3.2 出力
- [x] Markdown 出力（`--format md`）
  - 完了条件: そのままIssueに貼れる表が出る
- [x] JSON 出力（`--json out/summary.json` or `--format json`）
  - 完了条件: スキーマが安定（versionフィールド推奨）
- [x] CSV 出力（任意・余裕があれば）
  - 完了条件: 表計算ソフトに読み込める
- [x] HTML 出力（`--format html`）
  - 完了条件: 単一HTMLとして保存でき、Summary表が表示される

### 3.3 CLI統合
- [x] `heapsnap summary <file> --top N --format {md,json,csv}`
  - 完了条件: PLAN.md記載の例コマンドが動く
  - [x] `--format html` が指定できる
    - 完了条件: HTMLが出力される

### 3.4 Summary 検索オプション拡張
- [x] `heapsnap summary <file> --search <substring>` でオブジェクト名の部分一致検索
  - 完了条件: `heapsnap summary app.heapsnapshot --search hoge --format md` が動作し、対象名のみ表示される
- [x] `--search` の動作テストを追加（部分一致・大小文字の扱い・未ヒット）
  - 完了条件: 期待する対象のみ出力され、未ヒット時の出力が安定していることを確認できる

---

## 4. Retainers（保持経路）機能（価値の本丸）

### 4.1 ターゲット指定
- [x] `--id <node_id>` で対象ノードを選べる
  - 完了条件: id→内部node indexへ解決できる
- [x] `--name <constructor>` で候補を列挙し、`--pick {largest,count}` 等で選択できる（最小実装でOK）
  - 完了条件: name指定で対象が確定できる

### 4.2 逆辺（retainers）取得：遅延構築
- [x] “対象ノードの retainers を列挙”できるAPIを作る
  - 完了条件: `incoming_edges(target)` が動く
- [x] 全逆辺を一気に作らず、必要範囲だけ構築する設計にする
  - 完了条件: 小さな探索でメモリ使用が爆発しない

### 4.3 経路探索（BFS）
- [x] GC Root（または root 相当）から target への最短路を N本抽出
  - 完了条件: `--paths 5` で最大5本出る
- [x] 深さ制限（`--max-depth D` デフォルト10）
  - 完了条件: 深すぎる探索で止まる/タイムアウトしない
- [x] 探索の途中キャンセル（Ctrl-C）
  - 完了条件: 即停止し、部分出力を残さない

### 4.4 出力（構造保持）
- [x] Markdown（経路を Path #1/#2… で表示、edge名/種別を含む）
  - 完了条件: 貼って読める、構造が崩れない
- [x] JSON（steps配列 + optional merged tree）
  - 完了条件: Web UI が読める形（target/paths/steps）
- [x] HTML（`--format html`）
  - 完了条件: 単一HTMLとして保存でき、経路が読める

### 4.5 CLI統合
- [x] `heapsnap retainers <file> --id ... --paths N --max-depth D --format md|json`
  - 完了条件: PLAN.md記載の例が動く
  - [x] `--format html` が指定できる
    - 完了条件: HTMLが出力される

---

## 5. 品質・安定化

- [x] エラーメッセージ改善（meta不一致、id未発見、解析不能）
  - 完了条件: ユーザーが次に何をすべきか分かる
- [x] メモリ使用量の観測（簡易でOK、ログ/--verboseで表示）
  - 完了条件: 大きい入力でも挙動が読める
- [x] パフォーマンスのボトルネック計測（`cargo flamegraph`等は任意）
  - 完了条件: 遅い箇所が特定できる

---

## 6. テスト・回帰

- [x] fixtures（複数 heapsnapshot）を用意（small/medium/large）
  - 完了条件: CIで少なくとも small が走る
- [x] スナップショットテスト（JSON出力のスキーマ・主要値）
  - 完了条件: 仕様変更が差分として検知できる
- [x] 異常系テスト（壊れたJSON、meta欠損、巨大で中断）
  - 完了条件: panicせずエラーで返す

---

## 7. ドキュメント（最低限）

- [x] README にインストール/使い方/セキュリティ方針を書く
  - 完了条件: 初見で `summary` と `retainers` が使える
- [x] 出力スキーマ（JSON）を `docs/schema.md` に固定
  - 完了条件: 将来Web UIが読める

---

## 8. 将来拡張の下準備（MVP後）

- [x] `heapsnap build`：UI用に `outdir/` へ JSON をまとめて吐く（任意）
- [x] `heapsnap diff a b`：Class Summary の差分（次の大きな価値）
- [x] `heapsnap diff a b --format html`
  - 完了条件: 単一HTMLとして保存でき、Diff表が表示される
- [x] Dominator Tree：取得可能なら読み取り、無いなら計算（別途設計）
- [x] `heapsnap dominator <file> --format html`
  - 完了条件: 単一HTMLとして保存でき、Dominator chain が読める
- [x] `heapsnap detail <file> --id <id>` / `heapsnap detail <file> --name <name>`
  - 完了条件: `--id`/`--name` のどちらかが必須で動作する
  - 出力形式: `--format md|json|csv|html`
- [x] `detail --name` の id 一覧出力
  - 完了条件: `--name` は該当 id の一覧を出力する（md/json/csv/html）
- [x] `detail --name` の id 一覧に `--limit`/`--skip` を追加
  - 完了条件: id 一覧のページングができる（CLI引数で制御）
- [x] `detail --id` の基本情報出力
  - 完了条件: name/node_type/count/self_size_sum/max_self_size/min_self_size/avg_self_size が出る（md/json/csv/html）
- [x] `detail --id` の補助情報出力
  - 完了条件: node ids 一覧/上位N retainers/上位N outgoing edges/shallow size 分布 が出る（md/json/csv/html）
- [x] `detail` のテスト追加
  - 完了条件: `--name`/`--id` の基本出力とページング、各フォーマットの出力が検証される
- [x] `detail` の HTML 出力（静的レポート）
  - 完了条件: HTML 内に遷移リンクを含めず、静的に閲覧できる

### 8.1 ローカルHTTPサーバ（serve）

- [x] ルーティング設計の固定（エンドポイント/クエリ）
  - 完了条件: `/summary` `/detail` `/retainers` `/diff` `/dominator` が確定し、クエリが決まる
- [x] `heapsnap serve <file> --port <p>` を追加
  - 完了条件: `127.0.0.1` にのみバインドして起動できる
- [x] `summary` の HTML を HTTP で返す
  - 完了条件: `/summary` が 200 で HTML を返す
  - 完了条件: Constructor から `detail --name` 相当（`/detail?name=...`）へ遷移できる
- [x] `detail` の HTML を HTTP で返す
  - 完了条件: `/detail?name=...` または `/detail?id=...` が 200 で HTML を返す
  - 完了条件: `detail --name` 相当の画面で id 一覧から `detail --id` 相当（`/detail?id=...`）へ遷移できる
- [x] `retainers` の HTML を HTTP で返す
  - 完了条件: `/retainers?id=...` が 200 で HTML を返す
  - 完了条件: 表示中の constructor/name から `detail --name` 相当（`/detail?name=...`）へ遷移できる
- [x] `diff` の HTML を HTTP で返す
  - 完了条件: `/diff?file_a=...&file_b=...` が 200 で HTML を返す
  - 完了条件: Constructor から `detail --name` 相当（`/detail?name=...`）へ遷移できる
- [x] `dominator` の HTML を HTTP で返す
  - 完了条件: `/dominator?id=...` が 200 で HTML を返す
  - 完了条件: 表示中の constructor/name から `detail --name` 相当（`/detail?name=...`）へ遷移できる
- [x] `serve` のテスト追加
  - 完了条件: 主要ルートが 200 を返す
- [x] `--format html` の廃止方針を適用
  - 完了条件: `summary`/`retainers`/`diff`/`dominator`/`detail` の `--format html` を削除する
  - 完了条件: HTML は `serve` から提供し、CLI は md/json/csv のみを受け付ける
  - 完了条件: README / schema / help / テストを更新し、`cargo test` が通る

---
