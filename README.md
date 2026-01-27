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

```sh
heapsnap summary app.heapsnapshot --top 50 --format md
heapsnap summary app.heapsnapshot --format json
heapsnap summary app.heapsnapshot --json out/summary.json
heapsnap summary app.heapsnapshot --contains Store
```

### Retainers

```sh
heapsnap retainers app.heapsnapshot --id 12345 --paths 5 --max-depth 10 --format md
heapsnap retainers app.heapsnapshot --name FooStore --pick largest --format json
```

### Build (UI 用まとめ出力)

```sh
heapsnap build app.heapsnapshot --outdir out
```

出力ファイル:
- `out/summary.json`
- `out/meta.json`

### Diff

```sh
heapsnap diff a.heapsnapshot b.heapsnapshot --format md
heapsnap diff a.heapsnapshot b.heapsnapshot --format json
```

### Dominator

```sh
heapsnap dominator app.heapsnapshot --id 12345 --format md
heapsnap dominator app.heapsnapshot --name FooStore --pick largest --format json
```

### Global Options

- `--verbose`: 詳細ログ（オブジェクト名/文字列など）を表示
- `--progress=false`: 進捗表示を無効化（デフォルトは ON）

## Output Schema

JSON 出力のスキーマは `docs/schema.md` に固定しています。

## Security

- No network access
- ローカルファイルのみを読み取り、外部送信しない
- 出力は明示指定したファイルまたは標準出力のみ
