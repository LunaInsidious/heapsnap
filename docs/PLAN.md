# HeapSnapshot CLI Analyzer 実装計画書（Rust）

## 目的

Chrome/Chromium が生成する `*.heapsnapshot` を **ローカル完結**で解析し、
DevTools では困難な「構造を保ったコピー（Markdown / JSON）」を可能にする CLI ツールを実装する。

本ツールは **バックエンドサーバを持たず**、
将来的に「CLI + ローカル Web UI」へ拡張可能な形を前提とする。

---

## スコープと非スコープ

### スコープ（MVP）

* `*.heapsnapshot` の解析
* 以下の CLI 機能

  * Class / Constructor Summary
  * Retainers（保持経路）抽出
* 構造を保った Markdown / JSON 出力
* 完全ローカル実行（ネットワーク通信なし）

### 非スコープ（MVPではやらない）

* Dominator Tree の完全再構築
* GUI / Web UI
* クラウド送信、リモート解析
* Chrome DevTools との直接連携

---

## 対応フォーマットと互換性方針

* 対象：

  * Chrome / Chromium 系 DevTools が出力する `*.heapsnapshot`
* 方針：

  * `snapshot.meta` を **唯一の信頼情報**としてパース
  * 未知フィールドは無視
  * 重大な構造差異（meta不一致）は **明示的エラー**
* 「将来も必ず動く」ことは保証しない
  → 対応バージョンを README に明記する前提

---

## セキュリティ・安心設計

* ネットワーク通信は **一切行わない**
* 依存クレートによる通信も禁止（要監査）
* ログ出力は最小限

  * オブジェクト名・文字列は `--verbose` のみ
* 入力データはディスク外へ書き出さない（明示指定を除く）

---

## 全体アーキテクチャ

```
heapsnap (CLI)
 ├─ parser
 │   └─ heapsnapshot streaming parser
 ├─ model
 │   ├─ NodeView / EdgeView（配列参照）
 │   └─ Index（constructor / type）
 ├─ analysis
 │   ├─ summary
 │   └─ retainers
 ├─ output
 │   ├─ markdown
 │   └─ json
 └─ cli
```

* **完全同期CLI**
* 巨大データ処理は逐次 / 遅延評価を原則とする

---

## パース方針（重要）

### 禁止事項

* `serde_json` による一括デシリアライズ
* ノード・エッジの完全コピー

### 採用方針

* ストリーミング JSON パース
* `nodes` / `edges` は **Vec<i64> 等の生配列**として保持
* ノード・エッジは **インデックス参照ビュー**で扱う

### 内部表現（概念）

* `SnapshotRaw`

  * `nodes: Vec<i64>`
  * `edges: Vec<i64>`
  * `strings: Vec<String>`
  * `meta`
* `NodeView { index }`
* `EdgeView { index }`

---

## 機能仕様（MVP）

### 1. Class / Constructor Summary

#### 内容

* Constructor 名ごとの

  * オブジェクト数
  * self size 合計
* retained size は **取得可能な場合のみ**

  * 推定値は `*_estimate` として明示的に分離

#### CLI例

```sh
heapsnap summary app.heapsnapshot --top 50 --format md
heapsnap summary app.heapsnapshot --json out/summary.json
```

---

### 2. Retainers（保持経路）抽出

#### 方針

* GC Root → 対象ノードまでの **最短保持経路**
* BFS による探索
* 制限：

  * 最大経路数 N（デフォルト 5）
  * 最大深さ D（デフォルト 10）
* 逆辺（retainers）は **遅延構築**

  * 必要ノード周辺のみ

#### CLI例

```sh
heapsnap retainers app.heapsnapshot --id 12345 --paths 5 --format md
heapsnap retainers app.heapsnapshot --name "FooStore" --pick largest
```

---

## 出力仕様

### Markdown（人間向け）

* ツリー構造を維持
* コピー＆ペースト可能
* Issue / PR / Docs でそのまま使える

例（概念）：

```md
- Retaining paths for FooStore (id=12345)
  - Path #1
    - Window --(property)__APP__--> App
    - App --(property)store--> FooStore
```

### JSON（機械向け）

```json
{
  "target": { "id": 12345, "name": "FooStore" },
  "paths": [
    {
      "steps": [
        { "from": "...", "edge": "...", "to": "..." }
      ]
    }
  ]
}
```

---

## CLI設計方針

* `heapsnap <command> [options]`
* コマンドは副作用なし（入力 → 出力）
* 進捗表示あり（`--progress` デフォルトON）
* Ctrl-C で安全に中断可能

---

## 将来拡張を見据えた設計

* CLI出力（JSON）を **Web UI がそのまま読める**
* Diff（A/B）は別サブコマンドで追加可能
* Dominator Tree は独立モジュールとして後付け

---

## テスト戦略

* 複数の実 heapsnapshot を fixtures として保持

  * サイズ差
  * Chrome バージョン差
* meta 構造が想定外の場合はテストで失敗させる
* 回帰テスト重視（出力 JSON のスキーマ比較）

---

## 成功条件（MVP完了の定義）

* 数百MB級 heapsnapshot を **OOMせず**処理できる
* Retainers を Markdown として構造保持で出力できる
* DevTools でスクショ・コピペしていた情報を **CLI出力だけで共有可能**
* ネットワーク通信ゼロ

---

## 実装優先順位

1. ストリーミングパーサ
2. Summary 出力
3. Retainers 抽出
4. 出力整形（Markdown/JSON）
5. エラーハンドリング・進捗表示
