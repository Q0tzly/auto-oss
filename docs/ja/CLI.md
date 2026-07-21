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

### `autos <動詞> <repo> "<feedback>" [オプション]`

本体パイプライン: フィードバックを policy 準拠の提出に変える。動詞が scope を
決めるので、よくあるケースでは `--scope` を書く必要がない——
[Conventional Commits](https://www.conventionalcommits.org/) にならった
命名で、多くの人がすでに馴染んでいる慣習に合わせた:

| 動詞 | scope |
|---|---|
| `fix` | `bug-fix` |
| `feat` | `feature` |
| `docs` | `docs` |
| `refactor` | `refactor` |
| `test` | `test` |
| `typo` | `typo` |

対象リポジトリがこれら以外の scope(`accepts.scopes` には任意の文字列を
宣言できる)を宣言している場合は `autos fix --scope <独自の値>` を使う。
`--scope` フラグを持つのは `fix` だけ——汎用の逃げ道として存在している。

| オプション | デフォルト | 意味 |
|---|---|---|
| `--repro <text>` | — | 再現手順。`require.reproduction` の policy ではバグ修正に必須、それ以外の動詞でも文脈として歓迎される |
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
5. **ゲート。** パッチ生成後に `gates.*` の全コマンドを表示し、実行直前に
   明示的な `y` を要求する。同意後、clone 内で実行し、出力は端末に流れる。
6. **プレビューと確認。** 全 diff・ゲート結果・提出本文(メタデータブロック
   込み)を提示。明示的な `y` なしには何も提出されない。`--dry-run` はここで
   停止。
7. **提出。** 対象への push 権限があればリポジトリ本体にブランチを push、
   なければ fork を作ってクロスリポジトリで PR。どちらも**あなたの**
   アカウントからの提出で、SPEC §3 のメタデータブロックが埋め込まれ、
   policy のラベルが best-effort で付く。
8. **fallback。** ここでハードエラーになるものはない: diff 超過・ゲート
   不通過・backend のエラー・backend が何も変更しなかった場合、すべて同じ
   扱いになる。収集した文脈(フィードバック・再現手順・あれば部分 diff・
   不合格の理由)を、policy の `fallback` に従って提出する(これも確認後に):
   - `issue`(デフォルト)— GitHub issue を作成。
   - `discussion` — GraphQL API 経由で GitHub Discussion を作成。カテゴリは
     優先順(`ideas`・`feedback`・`general`・`q&a`、なければリポジトリの
     先頭カテゴリ)で選ぶ。discussion カテゴリが存在しない(Discussions
     無効)場合はその旨を表示し、何も提出しない。
   - `none` — 何も提出しない。ローカルの diff と本文はディスクに残る。

ローカルリポジトリでも同じパイプラインが走るが、提出の手前で停止する。

宣言された `limits.per_author_per_week` は SPEC §4 の求める通り自主遵守される:
提出は `~/.auto-oss/submissions.tsv` にローカル記録され、対象リポジトリへの
直近 7 日間の提出数が上限に達していると `fix` は開始を拒否する。

### `autos status`

最近の `fix` の実行(別の端末で進行中のものを含む)を、現在のフェーズ
(`cloning`、`generating`、`awaiting-gate-approval`、`gates`、
`awaiting-approval`、`submitted-pr` …)
付きで一覧する。記録は `~/.auto-oss/runs/` にあり、7 日で削除される。
終了フェーズに達していない run(Ctrl-C・端末を閉じた・クラッシュ)は、
再開に使う正確な `autos resume` コマンドと一緒に表示される。

### `autos resume <workdir>`

終了フェーズ(`submitted-pr`・`submitted-issue`・`aborted`・`failed`・
`dry-run-done`)に達する前に中断された `fix` の実行を再開する——典型的には、
ゲート確認プロンプトで Ctrl-C を押してプロセスが死に、待機・提出・拒否の
いずれも記録されなかったケース。`<workdir>` は `autos status` がその実行に
対して表示する workdir。

再開は clone も backend の再実行も**しない**: clone と、backend がそこまでに
書き込んだ内容を、ディスク上にある状態のまま読む。フィードバック・scope・
その他の元の引数は追跡された実行から復元され、backend がタイトルと変更概要を
生成し終えていればそれも復元される。そこから先は通常の `fix` とまったく同じ
パイプラインが走る——ゲートは再実行される(以前の実行がゲートの途中で死んだ
かもしれないし、ゲートは冪等であることが前提)。提出は通常の実行と同じ確認
なしには行われない。

対象 policy がその scope をもう受け付けなくなっている場合や、work directory が
片付けられてしまっている場合は、おかしな状態で再開するのではなく、はっきりと
失敗する。この機能が存在する前の `autos` が記録した実行は自動的には再開できない
——work directory はディスクに残っているので、手作業で仕上げることはできる。

### `autos config [show | set <key> <value> | unset <key>]`

`~/.auto-oss/config.yml` をエディタで開かずに読み書きする。引数なしの
`autos config`(= `autos config show`)は設定ファイルの場所と、キーが
未設定のときに使われる既定値を含めた現在の値を表示する。

```
autos config set default_backend human
autos config set claude_code.model claude-sonnet-5
autos config unset claude_code.model
```

設定できるキーは `default_backend` と `claude_code.model`。
`default_backend` は書き込む前にバックエンド名を解決するので、存在しない
名前は次の提出時ではなくその場で弾かれる。ファイル(と `~/.auto-oss/`)は
最初の書き込み時に作られ、未設定のキーは書き出されない。

custom backend は `command` が単一の値ではなくリストなので、従来どおり
`backends:` ブロックを直接書く。

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
