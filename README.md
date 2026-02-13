# HeapSnapshot CLI Analyzer

HeapSnapshot (`*.heapsnapshot`) を **ローカル完結**で解析し、
Constructor Summary と Retainers を Markdown/JSON で出力する CLI です。

- This tool runs fully locally and performs no network access.
- The input heapsnapshot is processed on your machine only.
- Logs are minimal by default; use `--verbose` only when you want detailed names/strings.

## Install

```sh
cargo build --release
```

## Usage

### Summary

Constructor 名ごとの件数・self size を集計して出力します。`--search` で部分一致フィルタが可能です（`--contains` は互換 alias）。

```sh
heapsnap summary app.heapsnapshot --top 50 --format md
heapsnap summary app.heapsnapshot --format json
heapsnap summary app.heapsnapshot --json out/summary.json
heapsnap summary app.heapsnapshot --search Store
```

### Retainers

指定ノードの保持経路（GC Root からの最短経路）を抽出します。

```sh
heapsnap retainers app.heapsnapshot --id 12345 --paths 5 --max-depth 10 --format md
heapsnap retainers app.heapsnapshot --name FooStore --pick largest --format json
```

### Build (UI 用まとめ出力)

UI などで使いやすい形に `summary` と `meta` をまとめて出力します。

```sh
heapsnap build app.heapsnapshot --outdir out
```

出力ファイル:
- `out/summary.json`
- `out/meta.json`

### Diff

2つの snapshots の Summary 差分を出力します。

```sh
heapsnap diff a.heapsnapshot b.heapsnapshot --format md
heapsnap diff a.heapsnapshot b.heapsnapshot --format json
```

### Dominator

指定ノードの dominator chain を出力します。

```sh
heapsnap dominator app.heapsnapshot --id 12345 --format md
heapsnap dominator app.heapsnapshot --name FooStore --pick largest --format json
```

### Detail

Constructor の詳細（集計・ID一覧・retainers/outgoing edges など）を出力します。

```sh
heapsnap detail app.heapsnapshot --name FooObject --format md
heapsnap detail app.heapsnapshot --id 12345 --format json --top-retainers 10 --top-edges 10
```

### Serve

ローカル HTTP サーバを起動し、ブラウザで `summary/detail/retainers/diff/dominator` を閲覧します。

```sh
heapsnap serve app.heapsnapshot --port 7878
```

開いた後の主な URL:
- `http://127.0.0.1:7878/summary`
- `http://127.0.0.1:7878/detail?name=FooObject`
- `http://127.0.0.1:7878/detail?id=12345`
- `http://127.0.0.1:7878/retainers?id=12345`
- `http://127.0.0.1:7878/dominator?id=12345`
- `http://127.0.0.1:7878/diff?file_a=fixtures/a.heapsnapshot&file_b=fixtures/b.heapsnapshot`

`/summary` `/detail` `/retainers` `/diff` `/dominator` 画面では `skip` / `limit` をフォーム（number + select）で変更できます。
クエリパラメータを直接編集する方法も利用できます（例: `/summary?top=100&skip=200&limit=100`, `/detail?name=FooObject&skip=200&limit=100`）。

## Directory Layout

```text
.
├── src
│   ├── main.rs              # CLI entrypoint
│   ├── parser.rs            # streaming parser
│   ├── snapshot.rs          # SnapshotRaw / NodeView / EdgeView
│   ├── serve.rs             # localhost HTTP server
│   ├── analysis
│   │   ├── summary.rs
│   │   ├── retainers.rs
│   │   ├── diff.rs
│   │   ├── dominator.rs
│   │   └── detail.rs
│   └── output
│       ├── summary.rs
│       ├── retainers.rs
│       ├── diff.rs
│       ├── dominator.rs
│       └── detail.rs
├── tests                    # integration/regression tests
├── fixtures                 # test snapshots
└── docs                     # PLAN/TODO/ADR/schema など
```

### Design Intent

- `parser` と `snapshot` を分離し、巨大入力の読み取り責務と参照モデル責務を分ける
- `analysis` を機能別に分割し、`summary/retainers/diff/dominator/detail` のロジックを独立させる
- `output` を解析処理から分離し、同じ解析結果を `md/json/csv` と `serve` で再利用できるようにする
- `main.rs` は orchestration に限定し、業務ロジックは `analysis` / `output` / `serve` に寄せる
- `tests` は CLI/解析/出力の回帰確認を担い、`fixtures` で再現可能な入力を固定する

### Global Options

- `--verbose`: 詳細ログ（オブジェクト名/文字列など）を表示
- `--progress=false`: 進捗表示を無効化（デフォルトは ON）

## Output Schema

JSON 出力のスキーマは `docs/schema.md` に固定しています。

## Security

- No external network access (local HTTP server is allowed for UI workflow)
- ローカルファイルのみを読み取り、外部送信しない
- 出力は明示指定したファイルまたは標準出力のみ
