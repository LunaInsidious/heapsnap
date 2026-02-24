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

## ADR-007: `serve /dominator` は非同期ジョブ方式で計算する

- 日付: 2026-02-19
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
`/dominator` は全グラフ構築と IDOM 計算を毎回同期で行うため、
大きい snapshot では初回アクセスが極端に遅くなり、画面表示が実用にならない。

### 決定 / Decision
- `serve` 起動時に `id -> node_index` インデックスを作成する
- `/dominator` は初回アクセス時にバックグラウンド計算ジョブを開始し、
  フロントには「計算中」画面を即時返す
- 進捗は SSE (`/dominator/events`) で配信し、完了時に画面を更新する
- 進捗値は経過時間ではなく、解析フェーズ内で処理済み node/edge 数から算出する
- 同一 session で `id` / `max_depth` を変更して Apply した場合、旧ジョブは cancel する

### 採用理由 / Rationale
- 初回アクセスでも HTTP 応答を即時返せる
- 単一スレッドサーバでも、重い計算でフロントが固まる体感を回避できる
- 既存 `analysis::dominator` 実装を変更せず導入できる

### 検討した代替案 / Alternatives
- 同期計算のまま最適化のみ実施 → 初回表示遅延の根本解決にならない
- 永続キャッシュのみ導入 → 初回アクセスには効果がない
- 別プロセス化 → 構成が複雑化し MVP 範囲を超える

### 影響 / Consequences
- `serve` 内にジョブ状態管理（メモリ上）が追加される
- 初回は結果ではなく「計算中」画面が返る
- UI は SSE に対応したブラウザを前提とする

---

## ADR-008: Dominator 計算を Lengauer-Tarjan に変更し、serve で index をキャッシュする

- 日付: 2026-02-19
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md
- 補足: ADR-005 を置き換える

### 背景 / Context
Cooper アルゴリズムは大規模 snapshot の `/dominator` で計算時間が長く、
`max_depth` を小さくしても体感改善が小さい問題があった。
また、`serve` では同じ snapshot に対する dominator 計算を繰り返し実行していた。

### 決定 / Decision
- Dominator の IDOM 計算を Lengauer-Tarjan に変更する
- `serve` は初回計算で得た dominator index（roots + idom 配列）をメモリキャッシュし、
  2回目以降は chain 抽出のみ実行する
- progress は実際の node/edge 処理数をフェーズ別に SSE へ配信する

### 採用理由 / Rationale
- 大規模グラフで計算速度が向上しやすい
- 計算結果キャッシュで `id` / `max_depth` 変更時の再計算コストを削減できる
- progress 表示の説明性が上がる（経過時間推定ではなく実処理件数）

### 検討した代替案 / Alternatives
- Cooper のままループ微調整のみ実施 → 根本的な計算コストは下がりにくい
- 毎回再計算して progress 表示だけ改善 → 体感性能が改善しない
- 外部プロセス化・永続DBキャッシュ → 構成が重く MVP の範囲を超える

### 影響 / Consequences
- 実装複雑度は上がる
- `serve` は dominator index キャッシュ分のメモリを使用する
- 1回目計算後の dominator 画面応答は大幅に軽くなる

---

## ADR-009: `serve diff` のアップロード入力は一時ファイルとして保持する

- 日付: 2026-02-19
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
`/diff` ではブラウザの `input type=\"file\"` から `after` を受け取り、同じ画面で
`top/search/skip/limit` を再適用できる必要がある。

### 決定 / Decision
- ブラウザから受け取った upload バイト列は `TMPDIR/heapsnap-serve/` に一時ファイルとして保存する
- 一時ファイル名は upload 内容の fingerprint（同一プロセス内重複回避用）を含める
- 同一内容 upload は同じ一時ファイルパスを再利用する
- `serve` 実行中は再利用し、`serve` 停止時に掃除する

### 採用理由 / Rationale
- ブラウザはローカルファイルの実パスをサーバへ渡せないため、サーバ側で参照可能な実体が必要
- upload 内容を都度再送させずに、フィルタ再適用（`top/search/skip/limit`）を即時実行できる
- 巨大データでメモリ常駐を避け、再利用データをファイルで扱える

### 検討した代替案 / Alternatives
- メモリ (`Vec<u8>`) のみ保持 → 大容量でメモリ圧迫しやすく再起動時に消える
- 毎回再アップロード要求 → UX が悪く、通信量・待ち時間が増える
- クライアント側でローカルパスを送る → ブラウザのセキュリティモデル上不可能

### 影響 / Consequences
- 一時ファイル管理（作成/掃除）の責務が増える
- `serve` 停止時クリーンアップ実装が必要になる
- fingerprint 衝突時に誤って既存ファイルを再利用するリスクが理論上はある
- fingerprint 再利用・停止時掃除・session 維持はユニットテストで仕様固定する

---

## ADR-010: `serve diff` の解析結果をメモリキャッシュする

- 日付: 2026-02-19
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
`/diff` は同じ `before/after` で `top/search/skip/limit` を変える操作が多い。
毎回 snapshot を再パース・再集計すると、再アップロード不要でも待ち時間が長い。

### 決定 / Decision
- `before` が `serve` 起動時ファイルと同じ場合は起動時ロード済み snapshot を再利用する
- `after` snapshot はパス単位でメモリキャッシュする
- diff 集計結果は `(before, after, top, search)` をキーにメモリキャッシュする

### 採用理由 / Rationale
- 同一入力に対する再解析・再集計を回避できる
- フィルタ調整時の体感速度を改善できる
- 既存 CLI ロジックを流用できる

### 検討した代替案 / Alternatives
- キャッシュなしで毎回再計算 → 体感性能が悪い
- 永続キャッシュ（DB/ファイル） → 構成が重く、MVPの範囲を超える

### 影響 / Consequences
- `serve` 実行中のメモリ使用量は増える
- 同一クエリでは高速応答できる
- キャッシュ挙動はユニットテストで固定する

---

## ADR-011: `serve` のメモリ最適化は段階的ハイブリッド方式で進める

- 日付: 2026-02-19
- ステータス: Accepted
- 関連ドキュメント: PLAN.md, TODO.md

### 背景 / Context
大きい heapsnapshot（例: 131MB）を `serve` した際、実行時メモリ使用量が
入力サイズより大きくなる（例: 約270MB）ことが確認された。
`serve` は `summary/detail/retainers/dominator/diff` を同一プロセスで提供しており、
特に retainers/dominator はランダムアクセス前提のデータ保持が必要である。

### 決定 / Decision
- `serve` 全体を一気にストリーミング化しない
- まずはハイブリッド方式とする
  - `before` の常駐スナップショットは維持（既存機能互換を優先）
  - `after` 側キャッシュには上限・解放方針を導入する
  - `summary/diff` のみ段階的にストリーミング最適化を検討する

### 採用理由 / Rationale
- retainers/dominator を同等機能で維持する限り、完全ストリーミング化の
  メモリ削減効果は限定的になりやすい
- 全機能一括改修は実装リスクが高く、性能回帰や互換性問題を招きやすい
- 段階導入なら効果測定とロールバックが容易

### 検討した代替案 / Alternatives
- `serve` 全機能を一括ストリーミング化
  - 実装複雑度が高く、既存機能への影響範囲が大きい
- 現状維持（最適化なし）
  - 大容量入力時のメモリ課題が残る

### 影響 / Consequences
- 短期的にはメモリ削減は段階的（限定的）になる
- 既存機能（detail/retainers/dominator）の互換性は維持しやすい
- 最適化は計測ベースで順次適用する前提になる

---
