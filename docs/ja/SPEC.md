[English](../SPEC.md) | 日本語

# auto-oss プロトコル仕様 — v0(ドラフト)

> **注**: この翻訳は参考情報であり、規範となるのは[英語版](../SPEC.md)。
> 両者に差異がある場合は英語版が優先する。

auto-oss は**利用者側コーディングエージェント**のための貢献プロトコルである。
プロジェクトの(メンテナではなく)*利用者*の側で動くエージェントが、その
利用者のフィードバックをパッチにし、リポジトリが事前に宣言した条件のもとで
upstream に提出する。

プロトコルの成果物は 2 つ:

1. **`auto-oss.yml`** — リポジトリが opt-in と受け入れ条件を宣言するために
   公開する policy ファイル。
2. **提出メタデータブロック** — このプロトコルの下で生成されるすべての
   PR / Issue に埋め込まれる機械可読ブロック。

プロトコルはフォージ非依存である。前提とするのは git と、パッチ/PR および
Issue の存在だけで、この仕様のいかなる部分も GitHub に依存しない。

MUST / MUST NOT / SHOULD / MAY は RFC 2119 の意味で解釈する。

---

## 1. opt-in と発見

リポジトリは policy ファイルの公開によって opt-in する。クライアントは
以下の順で探索し、最初に見つかったものを使わなければならない(MUST):

1. `auto-oss.yml`(リポジトリのルート)
2. `.github/auto-oss.yml`

policy ファイルが存在しないリポジトリは opt-in して**いない**。クライアントは
opt-in していないリポジトリに対してエージェント生成の PR を開いてはならない
(MUST NOT)。利用者が通常の Issue を書く手伝いをしてもよい(MAY)が、それを
auto-oss の提出として表示してはならない(MUST NOT)。

## 2. policy ファイル: `auto-oss.yml`

```yaml
# 最小例
version: 0

accepts:
  scopes: [bug-fix, docs, typo]

gates:
  test: "cargo test"
```

```yaml
# 完全な例
version: 0

accepts:
  # このリポジトリが利用者側エージェントから受け入れる変更カテゴリ。
  # 既知の値: bug-fix, docs, typo, test, refactor, feature
  scopes: [bug-fix, docs, typo]
  # これを超える行数(追加+削除)のパッチは拒否。
  max_diff_lines: 300

gates:
  # リポジトリのルートから実行されるコマンド。提出が PR に足るには、
  # 宣言された全ゲートが exit code 0 で通らなければならない(MUST)。
  build: "cargo build"
  test: "cargo test"
  lint: "cargo clippy -- -D warnings"

require:
  # 提出する人間が、提出前にパッチをレビューしたことを確約する。
  human_review: true
  # (bug-fix scope の)フィードバックには再現手順が必須。
  reproduction: true

# パッチが作れない/ゲートが通らないときのクライアントの動作:
#   issue      - 収集した文脈を構造化 Issue として提出(デフォルト)
#   discussion - フォージの discussion 機能があればそこへ
#   none       - 何も提出しない
fallback: issue

limits:
  # v0 では勧告値: クライアントは自主的に守るべき(SHOULD)、
  # メンテナは強制してもよい(MAY)。
  per_author_per_week: 3

metadata:
  # フォージがラベルに対応していれば、クライアントが付けるべき(SHOULD)ラベル。
  label: "auto-oss"
  # クライアントが提出のタイトル・要約を書くべき(SHOULD)言語。
  # 利用者のフィードバックは常に原文のまま運ばれ、翻訳されない。
  language: "en"
```

### 2.1 フィールドの意味

| フィールド | 必須 | 意味 |
|---|---|---|
| `version` | yes | 仕様のメジャーバージョン。この文書は `0` を定義する。 |
| `accepts.scopes` | yes | 許可する変更カテゴリ。クライアントはパッチをちょうど 1 つの scope に分類し(MUST)、リストにない scope の PR を提出してはならない(MUST NOT)。 |
| `accepts.max_diff_lines` | no | パッチサイズの上限。超過は `fallback` への格下げ。 |
| `gates.*` | no | 名前付きシェルコマンド。PR 提出には宣言された全ゲートの exit 0 が必要(MUST)。ゲート名は自由だが `build` / `test` / `lint` が慣例。 |
| `require.human_review` | no(デフォルト `true`) | クライアントは最終 diff を人間にレビューさせ、メタデータブロックで確約させなければならない(MUST)。 |
| `require.reproduction` | no(デフォルト `false`) | bug-fix の提出には再現手順が必須(MUST)。 |
| `fallback` | no(デフォルト `issue`) | パッチのパイプラインが失敗したときの動作。 |
| `limits.per_author_per_week` | no | 提出者(人間)ごとの勧告レートリミット。 |
| `metadata.label` | no | 提出に付けるラベル。 |
| `metadata.language` | no | BCP 47 タグ。クライアントは提出のタイトルと要約をこの言語で書くべき(SHOULD)。`feedback` は言語に関わらず原文のままでなければならない(MUST)。 |

パースに失敗する policy ファイルは不在として扱わなければならない(MUST。
opt-in なし)。未知のフィールドは無視しなければならない(MUST。前方互換)。

## 3. 提出メタデータブロック

このプロトコルの下で提出されるすべての PR / Issue は、本文に YAML を含む
HTML コメントとして、ちょうど 1 つのメタデータブロックを埋め込まなければ
ならない(MUST):

```markdown
<!-- auto-oss:v0
scope: bug-fix
feedback: |
  この変更の動機となった利用者フィードバックの原文。
reproduction: |
  1. `foo --bar` を実行
  2. panic を観測
environment:
  os: macOS 15.2
  version: foo 1.4.2
agent:
  backend: claude-code
  model: claude-fable-5
gates:
  build: pass
  test: pass
  lint: pass
human_reviewed: true
client: auto-oss/0.1.0
-->
```

要件:

- `scope` はリポジトリの `accepts.scopes` のいずれかでなければならない(MUST)。
- `feedback` は利用者フィードバックの原文でなければならない(MUST)。これは
  来歴の記録であり、パッチを実在の利用者の実在の問題に結びつける。機密情報
  (資格情報・個人情報・プライベートなパス)は伏せてもよい(MAY)が、伏せ字は
  `[redacted]` のように可視でなければならず(MUST)、それ以外の書き換えや
  要約をしてはならない(MUST NOT)。
- `agent` はパッチを生成した backend を開示しなければならない(MUST)。開示は
  正直でなければならない(MUST): 手書きのパッチはエージェント関与を装わず、
  予約 backend `human` を宣言する。プロトコルの機構 — opt-in・ゲート・来歴 —
  は人間のパッチにも同じに適用される。
- `gates` は policy が宣言する全ゲートの結果を報告しなければならない(MUST)。
- policy が要求する場合、`human_reviewed` は `true` でなければならない(MUST)。
  確約するのは提出する人間であり、虚偽の確約はメンテナが拒否・ban する根拠と
  なる。
- `fallback` による提出(Issue)では、`gates` の値は `fail` / `skipped` で
  よく、`patch` フィールドに部分 diff を含めてもよい(MAY)。

メンテナと CI ツールはこのブロックをパースして、準拠の検証や、提出の
ルーティング・ラベリング・自動クローズに使える。

## 4. クライアントの義務

準拠クライアント(`autos` CLI など)は:

1. policy ファイルのないリポジトリにエージェント生成 PR を提出してはならない
   (MUST NOT)。
2. 宣言された全ゲートをローカルで実行し、全部通ったときだけ PR を提出しな
   ければならない(MUST)。それ以外は `fallback` に従わなければならない(MUST)。
3. `require.human_review` が true のとき、提出前に最終 diff・ゲート結果・
   提出本文を人間に提示して承認を得なければならない(MUST)。
4. メタデータブロックを、完全かつ正直に埋め込まなければならない(MUST)。
5. 宣言された `limits` をサーバー側の強制なしに守るべきである(SHOULD)。
6. 記録上の提出者を人間に保たなければならない(MUST): 提出は人間のアカウント
   から行われ、貢献の責任はその人間にあり続ける。

## 5. メンテナの義務

`auto-oss.yml` の公開は以下を意味する:

1. policy に準拠した提出は、人間の貢献と同様に誠実にトリアージされる。
   マージの約束ではない。
2. 非準拠の提出はレビューなしにクローズしてよい(MAY)。
3. メンテナはいつでも policy を変更・削除してよい(MAY)。提出時点で有効な
   policy がその提出を規律する。

## 6. バージョニング

`version` フィールドとメタデータブロックのタグ(`auto-oss:v0`)が仕様の
メジャーバージョンを表す。対応バージョンより高いメジャーに遭遇した
クライアントは、そのリポジトリを自分にとって opt-in なしとして扱わなければ
ならない(MUST)。

この仕様と非互換な拡張は、"auto-oss" の名前および `auto-oss.yml` /
`auto-oss:vN` 識別子を使ってはならない(MUST NOT)。

---

## ライセンス

この仕様は [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) で
ライセンスされる。帰属表示のもとで、共有・翻案・実装は自由。
