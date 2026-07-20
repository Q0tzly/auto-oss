[English](../CLI.md) | 日本語

# `autos` CLI リファレンス

[auto-oss プロトコル](SPEC.md)の参照クライアント。

```
cargo install auto-oss   # `autos` バイナリが入る
```

外部要件: `git`、`curl`、認証済みの `gh`(GitHub CLI)。
`claude-code` backend はさらに `claude` コマンドが必要。`human` backend は
追加要件なし。

## コマンド

### `autos policy <repo>`

リポジトリの受け入れ policy を表示する。opt-in していなければそう表示する。

`<repo>` はローカルパス・`owner/repo`・GitHub URL を受け付ける(`<repo>` を
取る全コマンド共通)。policy ファイルは SPEC §1 の通り、ルートの
`auto-oss.yml` → `.github/auto-oss.yml` の順に探索する。

結果は 3 種類に区別される:

- **Opted in** — policy を表示: scopes、ゲート、diff 上限、要件、fallback、ラベル。
- **Not opted in** — policy ファイルなし。プロトコル上、このリポジトリへの
  エージェント PR 提出は禁止であり、出力にもそう明記される。
- **Unusable** — ファイルはあるがパース不能、または未対応の spec バージョン。
  opt-in なし扱いで、理由を表示する。

リポジトリに到達できない場合はエラーであり、決して「not opted in」ではない。

### `autos fix <repo> "<feedback>" [オプション]`

本体パイプライン: フィードバックを policy 準拠の提出に変える。

| オプション | デフォルト | 意味 |
|---|---|---|
| `--scope <s>` | `bug-fix` | 変更カテゴリ。policy の `accepts.scopes` にあること |
| `--repro <text>` | — | 再現手順。`require.reproduction` の policy ではバグ修正に必須 |
| `--backend <b>` | `claude-code` | パッチ生成役: `claude-code` または `human` |
| `--dry-run` | off | ゲートとプレビューまでで停止。何も提出しない |

パイプラインの順序:

1. **policy 発見。** opt-in なし → 拒否して終了。要求 scope は作業前に
   policy と照合される。続いて全ゲートと untrusted repository の警告を表示し、
   明示的な `y` が入力されるまで clone もコマンド実行も行わない。
2. **clone** — 新規の一時 workdir へ。
3. **パッチ生成** — backend が行う。`claude-code` はフィードバック・scope・
   サイズ上限を制約として注入し Claude Code を非対話実行。`human` は制約を
   表示し、あなたが workdir を直接編集する間待つ。
4. **サイズ検査。** `accepts.max_diff_lines` を超える diff は policy の
   fallback に格下げ。
5. **ゲート。** `gates.*` の全コマンドを clone 内で実行。出力は端末に流れる。
6. **プレビューと確認。** 全 diff・ゲート結果・提出本文(メタデータブロック
   込み)を提示。明示的な `y` なしには何も提出されない。`--dry-run` はここで
   停止。
7. **提出。** 対象への push 権限があればリポジトリ本体にブランチを push、
   なければ fork を作ってクロスリポジトリで PR。どちらも**あなたの**
   アカウントからの提出で、SPEC §3 のメタデータブロックが埋め込まれ、
   policy のラベルが best-effort で付く。
8. **fallback。** ゲート不通過・サイズ超過の場合、収集した文脈(と部分 diff)
   を構造化 Issue として提出 — policy の `fallback` が許す場合のみ、これも
   確認後に。

ローカルリポジトリでも同じパイプラインが走るが、提出の手前で停止する。

宣言された `limits.per_author_per_week` は SPEC §4 の求める通り自主遵守される:
提出は `~/.auto-oss/submissions.tsv` にローカル記録され、対象リポジトリへの
直近 7 日間の提出数が上限に達していると `fix` は開始を拒否する。

### `autos init [--force]`

メンテナ側: カレントディレクトリに `auto-oss.yml` を対話式で生成する。
scopes・ゲート・diff 上限・再現手順の要否・fallback を質問し、書き出す前に
policy パーサで round-trip 検証するので不正な policy は生成されない。
`--force` で既存ファイルを上書き。

### `autos verify <pr-url>`

メンテナ / CI 側: PR を取得してメタデータブロックを抽出し、リポジトリの
policy と照合する。検証項目: ブロックがちょうど 1 つ、scope が受け入れ範囲、
実際の変更行数が `accepts.max_diff_lines` 以下、feedback と backend 開示が非空、
必須なら `human_reviewed`、必須なら再現手順、宣言された全ゲートの `pass` 報告。

メタデータブロックのない PR は通常の貢献であり、そのまま合格する。違反は
列挙され exit code が非ゼロになるので、そのまま CI に組める:

```yaml
- run: cargo run --quiet -- verify "${{ github.event.pull_request.html_url }}"
  env:
    GH_TOKEN: ${{ github.token }}
```

ゲート結果と human review は自己申告のままであり、その真実性は
(このリポジトリの CI がやっているように)ゲートの再実行が検証する。
変更行数はメタデータの申告ではなく PR から直接取得する。
