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
| `--scope <s>` | `bug-fix` | 変更カテゴリ。policy の `accepts.scopes` にあること。`bug-fix` 以外に `docs`・`typo`・`test`・`refactor`・`feature` があり、不具合修正ではなく機能提案をするときは `feature` を使う |
| `--repro <text>` | — | 再現手順。`require.reproduction` の policy ではバグ修正に必須 |
| `--backend <b>` | config の `default_backend`、なければ `claude-code` | パッチ生成役: `claude-code`・`human`・config の custom backend |
| `--dry-run` | off | ゲートとプレビューまでで停止。何も提出しない |

パイプラインの順序:

1. **policy 発見。** opt-in なし → 拒否して終了。要求 scope は作業前に
   policy と照合される。
2. **clone** — 新規の一時 workdir へ。
3. **パッチ生成** — backend が行う。`claude-code` はフィードバック・scope・
   サイズ上限を制約として注入して Claude Code を実行し、作業中の進捗
   (ツール呼び出し・コメント)を端末にストリーム表示する。`human` は制約を
   表示し、あなたが workdir を直接編集する間待つ。backend は提出の
   **タイトル**も提案する(`human` は入力を求める)。変更内容の説明は本文の
   「What changed」に、あなたの元のフィードバックは「Original feedback」に
   原文引用で載る。backend がタイトルを返さない場合はフィードバックの先頭行を
   切り詰めて使う。タイトルには常に scope の接頭辞が付く。
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

### `autos status`

最近の `fix` の実行(別の端末で進行中のものを含む)を、現在のフェーズ
(`cloning`、`generating`、`gates`、`awaiting-approval`、`submitted-pr` …)
付きで一覧する。記録は `~/.auto-oss/runs/` にあり、7 日で削除される。

### 設定: `~/.auto-oss/config.yml`

```yaml
default_backend: claude-code

claude_code:
  model: claude-sonnet-5   # `claude --model` に渡る。省略時は Claude Code に任せる

backends:
  codex:
    command: ["codex", "exec", "{prompt}"]
    model: gpt-5-codex     # メタデータへの開示用。コマンドには渡らない
```

custom backend は clone 内で実行される任意のコマンドで、`{prompt}` が
置換される。ファイルを編集して exit 0 することが期待される。`{prompt}` は
**独立した argv 要素として**書くこと — シェル文字列の中に埋め込むと
プロンプトの改行で壊れる(クォートの罠でもある)。

設定したモデルは提出メタデータの `agent.model` に記録され、メンテナが
「何がこのパッチを作ったか」を見られる。custom backend では開示のみで、
ツール側にフラグが必要なら `command` に書くこと。

### 並列実行

`fix` の各実行は独立している: それぞれ自分の workdir に clone し、自分の
status ファイルを書き、自分のブランチ(名前にタイムスタンプと pid が入る)を
push する。別々の端末で、異なるリポジトリにも同じリポジトリにも並列で走らせ
られる。注意点は 2 つ: `limits.per_author_per_week` は全実行を合算して数える
ことと、各実行が承認プロンプトのために端末を必要とすること(=端末を分ける)。

### `autos init [--force]`

メンテナ側: カレントディレクトリに `auto-oss.yml` を対話式で生成する。
scopes・ゲート・diff 上限・再現手順の要否・fallback を質問し、書き出す前に
policy パーサで round-trip 検証するので不正な policy は生成されない。
`--force` で既存ファイルを上書き。

### `autos verify <pr-url>`

メンテナ / CI 側: PR を取得してメタデータブロックを抽出し、リポジトリの
policy と照合する。検証項目: ブロックがちょうど 1 つ、scope が受け入れ範囲、
feedback と backend 開示が非空、必須なら `human_reviewed`、必須なら再現手順、
宣言された全ゲートの `pass` 報告。

メタデータブロックのない PR は通常の貢献であり、そのまま合格する。違反は
列挙され exit code が非ゼロになるので、そのまま CI に組める:

```yaml
- run: cargo run --quiet -- verify "${{ github.event.pull_request.html_url }}"
  env:
    GH_TOKEN: ${{ github.token }}
```

`verify` が検証するのは*申告*の準拠性であること。申告の真実性は、
(このリポジトリの CI がやっているように)ゲートの再実行が検証する。
