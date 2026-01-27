# TODO.md — HeapSnapshot CLI Analyzer (Rust)

このTODOは **上から順に実装すればMVPが完成**するように並べてある。
各項目は「完了条件」を満たしたらチェックを付ける。

---

## 0. リポジトリ初期化

- [ ] Cargo workspace or single crate を決定（MVPは single crate 推奨）
  - 完了条件: `cargo test` が通る
- [ ] `clap` を導入して `heapsnap --help` を出せる
  - 完了条件: `heapsnap summary --help` `heapsnap retainers --help` が表示される
- [ ] CI（最低限 `cargo fmt` / `cargo test`）を設定
  - 完了条件: PR/ローカルで自動実行できる

---

## 1. 基本方針の固定（コードに落とす“契約”）

- [ ] ネットワーク通信をしない方針を README とコードコメントに明記
  - 完了条件: README に「外部通信しない」「ローカル完結」を明記
- [ ] ログの方針を決める（デフォルト最小、`--verbose` で詳細）
  - 完了条件: `--verbose` なしで個人情報/大量文字列が出ない

---

## 2. パーサ基盤（最重要）

### 2.1 heapsnapshot の最小読み取り（meta確認）
- [ ] `snapshot.meta` を読み取り、nodes/edges のフィールド定義（型・順序）を取得
  - 完了条件: metaの要点（node_fields, edge_fields, node_types, edge_types など）を構造体に格納できる
- [ ] meta 互換性チェックを実装（想定外なら明示的エラー）
  - 完了条件: 異なるmeta入力で「何が違うか」付きでエラーになる

### 2.2 ストリーミング JSON パース
- [ ] `nodes` / `edges` を **一括デシリアライズせず** 生配列として読み込む
  - 完了条件: 数百MB級で OOM せずに読み込みが進む
- [ ] `strings` を読み込む（必要最小限で保持）
  - 完了条件: node/edge が参照する string index を name に解決できる

### 2.3 内部表現（Raw + View）
- [ ] `SnapshotRaw { nodes: Vec<i64>, edges: Vec<i64>, strings: Vec<String>, meta }`
  - 完了条件: `SnapshotRaw` を生成し、参照できる
- [ ] `NodeView` / `EdgeView` を実装（インデックス参照でフィールドを読む）
  - 完了条件: 任意ノードの `type/name/self_size/edge_count` が取得できる

### 2.4 基本ユーティリティ
- [ ] 進捗表示（`--progress` デフォルトON）
  - 完了条件: 大きいファイルで進捗%または処理段階が表示される
- [ ] Ctrl-C 中断の安全停止（途中でも破損出力を残さない）
  - 完了条件: 中断しても次回実行に影響しない（tempファイル掃除）

---

## 3. Summary 機能（MVPの先頭）

### 3.1 集計ロジック
- [ ] constructor/name ごとの `count` と `self_size_sum` を集計
  - 完了条件: 上位Kを出せる（K=50など）
- [ ] フィルタ/検索（`--contains` など最小でOK）
  - 完了条件: 特定文字列を含むconstructorだけ表示できる

### 3.2 出力
- [ ] Markdown 出力（`--format md`）
  - 完了条件: そのままIssueに貼れる表が出る
- [ ] JSON 出力（`--json out/summary.json` or `--format json`）
  - 完了条件: スキーマが安定（versionフィールド推奨）
- [ ] CSV 出力（任意・余裕があれば）
  - 完了条件: 表計算ソフトに読み込める

### 3.3 CLI統合
- [ ] `heapsnap summary <file> --top N --format {md,json,csv}`
  - 完了条件: PLAN.md記載の例コマンドが動く

---

## 4. Retainers（保持経路）機能（価値の本丸）

### 4.1 ターゲット指定
- [ ] `--id <node_id>` で対象ノードを選べる
  - 完了条件: id→内部node indexへ解決できる
- [ ] `--name <constructor>` で候補を列挙し、`--pick {largest,count}` 等で選択できる（最小実装でOK）
  - 完了条件: name指定で対象が確定できる

### 4.2 逆辺（retainers）取得：遅延構築
- [ ] “対象ノードの retainers を列挙”できるAPIを作る
  - 完了条件: `incoming_edges(target)` が動く
- [ ] 全逆辺を一気に作らず、必要範囲だけ構築する設計にする
  - 完了条件: 小さな探索でメモリ使用が爆発しない

### 4.3 経路探索（BFS）
- [ ] GC Root（または root 相当）から target への最短路を N本抽出
  - 完了条件: `--paths 5` で最大5本出る
- [ ] 深さ制限（`--max-depth D` デフォルト10）
  - 完了条件: 深すぎる探索で止まる/タイムアウトしない
- [ ] 探索の途中キャンセル（Ctrl-C）
  - 完了条件: 即停止し、部分出力を残さない

### 4.4 出力（構造保持）
- [ ] Markdown（経路を Path #1/#2… で表示、edge名/種別を含む）
  - 完了条件: 貼って読める、構造が崩れない
- [ ] JSON（steps配列 + optional merged tree）
  - 完了条件: Web UI が読める形（target/paths/steps）

### 4.5 CLI統合
- [ ] `heapsnap retainers <file> --id ... --paths N --max-depth D --format md|json`
  - 完了条件: PLAN.md記載の例が動く

---

## 5. 品質・安定化

- [ ] エラーメッセージ改善（meta不一致、id未発見、解析不能）
  - 完了条件: ユーザーが次に何をすべきか分かる
- [ ] メモリ使用量の観測（簡易でOK、ログ/--verboseで表示）
  - 完了条件: 大きい入力でも挙動が読める
- [ ] パフォーマンスのボトルネック計測（`cargo flamegraph`等は任意）
  - 完了条件: 遅い箇所が特定できる

---

## 6. テスト・回帰

- [ ] fixtures（複数 heapsnapshot）を用意（small/medium/large）
  - 完了条件: CIで少なくとも small が走る
- [ ] スナップショットテスト（JSON出力のスキーマ・主要値）
  - 完了条件: 仕様変更が差分として検知できる
- [ ] 異常系テスト（壊れたJSON、meta欠損、巨大で中断）
  - 完了条件: panicせずエラーで返す

---

## 7. ドキュメント（最低限）

- [ ] README にインストール/使い方/セキュリティ方針を書く
  - 完了条件: 初見で `summary` と `retainers` が使える
- [ ] 出力スキーマ（JSON）を `docs/schema.md` に固定
  - 完了条件: 将来Web UIが読める

---

## 8. 将来拡張の下準備（MVP後）

- [ ] `heapsnap build`：UI用に `outdir/` へ JSON をまとめて吐く（任意）
- [ ] `heapsnap diff a b`：Class Summary の差分（次の大きな価値）
- [ ] Dominator Tree：取得可能なら読み取り、無いなら計算（別途設計）

---
