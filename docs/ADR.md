# ADR — Architecture Decision Record

このドキュメントは、本プロジェクトにおける **設計・技術選択の決定事項**を記録する。
ここに書かれた内容は「後から見て覆す理由が無い限り、従う前提」とする。

- 実装詳細やコード断片は書かない
- 「なぜそうしたか」を最重要視する
- 軽微なメモや調査途中の知見は MEMO.md に書く

---

## ADR-000: 初期方針（テンプレート）

- 日付: YYYY-MM-DD
- ステータス: Accepted / Superseded / Deprecated
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
なぜこの判断が必要になったのか。
どのような制約・前提・問題があったのか。

（例）
- heapsnapshot が巨大である
- ブラウザ実装では OOM や UI フリーズの懸念がある
- ローカル完結が必須要件である

---

### 決定 / Decision
何を決めたかを **一文で明確に**書く。

（例）
- CLI 実装は Rust を採用する
- JSON は一括デシリアライズせずストリーミング処理とする

---

### 採用理由 / Rationale
なぜこの決定を採用したか。
判断基準（性能、安全性、将来拡張など）を明示する。

---

### 検討した代替案 / Alternatives
検討したが採用しなかった案と、その理由。

（例）
- Node.js 実装 → メモリ使用量と安定性の懸念
- serde_json 全展開 → 巨大データで OOM の可能性

---

### 影響 / Consequences
この決定によって生じる影響。

- 良い影響
- 悪い影響（制約・コスト・後回しになる機能）

---

### 補足 / Notes（任意）
将来見直す可能性や、注意点があれば記載。

---

## ADR-001: CLI 引数パースに clap を採用

- 日付: 2026-01-27
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
`heapsnap summary` / `heapsnap retainers` のサブコマンドと
オプションを安定して扱う必要がある。

### 決定 / Decision
CLI 引数パースに `clap` を採用する。

### 採用理由 / Rationale
- Rust 標準に近い形でサブコマンド/ヘルプ生成を行える
- 依存を最小限に保ちつつ、要件を満たせる

### 検討した代替案 / Alternatives
- 自前の引数パーサ → 仕様拡張時の保守コストが高い
- `argh` / `pico-args` → ヘルプやサブコマンドの整合性に手数がかかる

### 影響 / Consequences
- 依存クレートが1つ増える
- `--help` の挙動は `clap` に準拠する

---

## ADR-002: heapsnapshot パースに serde_json のストリーミングを採用

- 日付: 2026-01-27
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
巨大 heapsnapshot を OOM せずに解析するため、
トップレベルの一括デシリアライズは避ける必要がある。

### 決定 / Decision
`serde_json::Deserializer` を用いたストリーミングパースで
`nodes` / `edges` / `strings` を配列として読み込む。

### 採用理由 / Rationale
- Rust 標準のエコシステムで読み取りが完結する
- `Value` 全展開を避けつつ、必要な配列を確実に保持できる

### 検討した代替案 / Alternatives
- `serde_json::from_reader` による構造体一括デシリアライズ
  → 解析がブラックボックス化しやすい
- `simd-json` など別クレート
  → 依存と運用コスト増、要件に対して過剰

### 影響 / Consequences
- パース層は `serde`/`serde_json` に依存する
- 進捗表示やキャンセル制御は別途設計が必要

---

## ADR-003: ローカルHTTPサーバの許可

- 日付: 2026-02-13
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md, README.md

### 背景 / Context
HTML 出力から detail などのコマンドを実行して遷移したいという要望がある。
しかし静的 HTML だけではブラウザが CLI を実行できず、ローカルの実行環境が必要になる。

### 決定 / Decision
ローカル環境に限って HTTP サーバを起動する実装を許可する。

### 採用理由 / Rationale
- ブラウザからの操作性を改善できる
- ローカル完結というプロジェクトの前提は維持できる

### 検討した代替案 / Alternatives
- 静的 HTML でコピーリンクのみ提供 → 操作性が低い
- OS の URL ハンドラ登録 → ユーザー環境依存で導入コストが高い

### 影響 / Consequences
- 「ネットワーク通信禁止」ポリシーを緩和する必要がある
- サーバはローカル限定で、外部送信は行わないことを明記する

---

## ADR-003: Ctrl-C キャンセル対応に ctrlc を採用

- 日付: 2026-01-27
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
長時間処理を安全に中断できる必要がある。
標準ライブラリだけではクロスプラットフォームの SIGINT 処理が難しい。

### 決定 / Decision
`ctrlc` クレートで Ctrl-C ハンドラを実装する。

### 採用理由 / Rationale
- クロスプラットフォームで安定した SIGINT 対応ができる
- 実装が小さく、依存追加が最小限で済む

### 検討した代替案 / Alternatives
- `signal-hook` などの低レベル実装 → 実装負荷が高い
- Ctrl-C 無視 → 要件に反する

### 影響 / Consequences
- 依存クレートが1つ増える
- SIGINT 受付後は安全に中断する設計が必要

---

## ADR-004: `heapsnap build` の出力構成

- 日付: 2026-01-27
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
UI 連携用に JSON をまとめて出力する必要がある。
最小構成で安定スキーマを提供したい。

### 決定 / Decision
`build` は `summary.json` と `meta.json` を `outdir/` に出力する。

### 採用理由 / Rationale
- Summary は最も有用な集計結果であり、UI の基礎データになる
- Meta は入力全体の規模を把握するために必要

### 検討した代替案 / Alternatives
- Retainers を同時出力 → 対象ノードが未指定のため不可能
- 1ファイルにまとめる → UI 側の分割読み込みが困難

### 影響 / Consequences
- 出力ファイルが 2 つになる
- `docs/schema.md` の更新が必要

---

## ADR-005: Dominator Tree 計算に Cooper アルゴリズムを採用

- 日付: 2026-01-27
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
スナップショットに Dominator 情報が含まれない場合でも取得できる必要がある。
巨大データでも OOM を避けつつ実装可能な手法が必要。

### 決定 / Decision
`Cooper et al.` の反復的な即時支配者（IDOM）計算を採用する。

### 採用理由 / Rationale
- 実装が比較的シンプルで検証しやすい
- 既存の edge 情報から構築でき、外部依存が不要

### 検討した代替案 / Alternatives
- Lengauer-Tarjan の完全実装 → 実装コストが高い
- Dominator を扱わない → TODO 要件に反する

### 影響 / Consequences
- 全ノードの前後関係（succ/pred）を構築するため一時メモリが増える
- 大規模データでは計算時間が増える可能性がある

---

## ADR-006: 不正なサロゲートを `\\uFFFD` に正規化してパースを継続

- 日付: 2026-01-27
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
実データの heapsnapshot に単独サロゲート（例: `\\uD800`）が含まれる場合、
標準 JSON パーサはエラーで停止する。

### 決定 / Decision
入力ストリームを前処理し、単独サロゲートは `\\uFFFD` に置換してパースを継続する。

### 採用理由 / Rationale
- 実データの解析を止めないため
- 置換は最小限で、文字列の整合性を大きく崩さない

### 検討した代替案 / Alternatives
- パーサを変更せずエラー扱い → 実運用で解析不能
- 全 JSON を一度書き換える → 大容量データで非現実的
- 別パーサ導入 → 依存増と運用コスト増

### 影響 / Consequences
- 文字列に `U+FFFD` が含まれる場合がある
- 変換の存在をドキュメントで明記する必要がある

---
