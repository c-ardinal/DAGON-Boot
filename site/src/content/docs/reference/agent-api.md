---
title: "エージェントとのつなぎ方: serve API"
description: "DAGON が dagon serve を通してエージェントに届く方法です。ルート、同意、キー、送るものを示します。"
---

DAGON は 1 つのインターフェースでエージェントと話します。エージェントはデータです。エージェント（id、名前、提供元）と、そこに届くつなぎ方があり、つなぎ方はそれぞれ DAGON が実装する転送の種類のどれかです（後述の「組み込みのエージェント」と「転送の種類」）。利用者は、つなぐときに 1 度だけ、1 つのエージェントと 1 つのつなぎ方に同意します。その後は、もう一度尋ねずに質問を送ります。ログインはエージェントの側にとどまり、API キーは OS の資格情報ストアにとどまります（「API キーの置き場所」）。どちらもページには届きません。

下のルートはどれも `dagon serve` の下にあり、ほかの `/v1/` のルートと同じ確認があります。Host と Origin の確認（403）と、`Authorization: Bearer <passphrase>`（401）です。

## GET /v1/agent

```json
{
  "agents": [
    { "id": "claude", "name": "Claude", "provider": "Anthropic",
      "ways": [
        { "id": "claude-code", "name": "Claude Code", "kind": "command", "available": true, "key": null },
        { "id": "api", "name": "API キー", "kind": "messages-http", "available": false, "key": false }
      ] }
  ],
  "default": { "agent": "claude", "way": "claude-code" },
  "linked": false,
  "consent": null,
  "available": true,
  "provider": { "id": "claude", "name": "Claude (Anthropic)" },
  "errors": [],
  "consent_error": null,
  "sends": ["…"],
  "never_sends": ["…"]
}
```

- `agents`: 組み込みが先で、その後に利用者のもの（`~/.dagon/agents/*.toml`）が続きます。つなぎ方はそれぞれ `kind`（`command` | `messages-http`。種類は増えることがあるので、知らないものは中身を解釈しないでください）と `available`（この PC でいま使えるか。`command` のつなぎ方はプログラムが PATH にあること、`messages-http` のつなぎ方はキーが登録されていること）を持ちます。`key`: API キーを取るつなぎ方では、登録されているかどうか（`true` / `false`）。取らないつなぎ方では `null` です。この本文に、プログラム、引数、URL、キーが入ることはありません。
- `default`: つなぐときのダイアログがあらかじめ選ぶつなぎ方（最初のエージェントの最初のつなぎ方）。
- `linked`: 定義に残っているエージェントとつなぎ方に対して、同意が記録されていること。
- `consent`: つないだ後は `{ "agent": "claude", "way": "claude-code", "at": "2026-11-04T04:14:35Z" }`（`at` は UTC の RFC 3339）、それ以外は `null`。
- `available` / `provider`: つないだつなぎ方（つなぐ前は既定のつなぎ方）について、ここで使えるかどうかと、つなぐときの文に使うエージェントの名前（`"<name> (<provider>)"`）。DAGON が答えられるのは、`linked` と `available` が両方とも true のときだけです。
- `errors`: 読み込めなかった利用者の定義ファイル。`"<file name>: <why>"` の形です（フォルダは含みません）。
- `consent_error`: ~/.dagon/agent.json がこの dagon の読まないバージョンのとき、その理由（つなぎ直すと、読めるものを記録します）。それ以外は `null`。
- `sends` / `never_sends`: 「つなぐと送るもの」の一覧で、つなぐときのダイアログが表示します。ターミナル（`dagon ai link`）も同じ一覧を表示します。

## POST /v1/agent/link

本文: `{ "agent": "claude", "way": "claude-code", "consent": true }`。そのエージェントとつなぎ方への同意を記録します（新しくつなぐと古いものに代わります。つなぎ方を変えると、もう一度同意を求めます）。

- 200: GET /v1/agent と同じ本文（`linked: true` になっています）。
- 400 `agent and way must name a defined agent and way` / `consent must be true`（JSON の真偽値 `true` だけが有効です）。何も記録しません。
- 403: `dagon demo` の実行中（そこではつなげません。GET は動きます）。
- 500 `the consent could not be saved`。

つなぐときに、つなぎ方が使える必要はありません（利用者は後から Claude Code を入れたり、キーを登録したりできます）。

## DELETE /v1/agent/link

同意を取り消します（記録が無いときも成功します）。

- 200: GET /v1/agent と同じ本文（`linked: false` になっています）。
- 403: `dagon demo` の実行中。
- **切断は、実行中のエージェントの呼び出しを待ちます。** どの呼び出しも、確認から呼び出しの終わりまで同意を保持します（`~/.dagon/agent.lock` の共有ロック。呼び出しごとにハンドルが 1 つなので、スレッドでも、`dagon serve` と `dagon ai unlink` のようなプロセスでも、同じように排他されます）。DELETE はすぐに同意を消し（呼び出しはロックの下で読み直すので、その時点から新しい呼び出しは通りません）、その後でロックを排他的に取ります。そのため、200 を返すのは実行中の呼び出しが無いときだけです。待ちは最大で 1 回の呼び出しの分です。質問なら約 120 秒まで、構成の案の 1 ラウンドなら 600 秒までです。ページは、DELETE が答えるまで待ちの状態を表示してください。
- その後は、どの呼び出しも確認を通りません。実行中の `ai_topology` の作業は取り消します。答えている途中の質問は、それ以上呼び出しをせず、答えの代わりに 409 `AGENT_NOT_LINKED` を返します（`dagon ai ask` も答えを表示しません）。

## PUT /v1/agent/key/{agent}/{way}

本文: `{ "key": "..." }`。つなぎ方の API キーを OS の資格情報ストアに登録します（前のものに代わります）。キーは前後の空白を除いて受け取ります。表示できる ASCII 文字で 1 から 1024 文字でなければなりません（HTTP のヘッダに入るためです。Windows 資格情報マネージャーが持てるのは最大 2560 バイトで、UTF-16 で保存します）。キーを応答に含めることも、ログに残すことも、ファイルに書くこともありません。ページはキーをどこにも保存せず、送った後で入力欄を空にします。

- 200: GET /v1/agent と同じ本文（つなぎ方の `key` が `true` になっています）。
- 400 `this way takes no key`（ログインのつなぎ方）、`key must be a string`、またはキーを受け付けない理由。何も保存しません。
- 404 `no such agent and way`。
- 403: `dagon demo` の実行中、または Host/Origin の確認（ほかのオリジンは受け付けません）。
- 503 `AGENT_KEY_STORE`: ここでは OS の資格情報ストアを使えません（例: Secret Service が動いていない Linux）。`error` が理由と、入れ方や起動の仕方を示します。ほかの場所には何も保存しません。

## DELETE /v1/agent/key/{agent}/{way}

つなぎ方のキーをストアから消します（登録が無いときも成功します）。

- 200: GET /v1/agent と同じ本文（つなぎ方の `key` が `false` になっています）。
- 400 / 404 / 403 / 503 は PUT と同じです。

## API キーの置き場所

OS の資格情報ストアだけです。Windows 資格情報マネージャー、macOS のキーチェーン、Linux の Secret Service（GNOME Keyring、KeePassXC など）です。キーはそれぞれ、サービス `dagon`、アカウント `<agent id>/<way id>`（`claude/api`）の項目です。DAGON は環境変数からキーを読まず、ファイルにキーを置きません。ストアを使えない場所では、登録を受け付けません。キーがストアから出るのは、そのつなぎ方のリクエストのヘッダに入るときだけです。

## つなぐ前の AI のルート

`/v1/ai/...` のルートはどれも、つなぐまで次の答えを返します。

```json nocheck
HTTP 409
{ "error": "DAGON is not linked to an agent; link it first (consent is asked once)",
  "code": "AGENT_NOT_LINKED" }
```

文ではなく `code` で判定してください。同意のファイルがこの dagon の読まないバージョンのときは、上のどのルートでも代わりに 409 `AGENT_CONSENT_VERSION` を返します。つないだ後のルートは、いつもどおり動きます（`/v1/ai/topology` は作業を始めます。ここで使えないつなぎ方は、作業の中で理由を示して失敗します。例: `no API key is registered for claude/api`）。

`dagon demo` の実行中は、エージェントに届くルートはどれも 403 を返します（`/v1/agent/link` の POST/DELETE、`/v1/agent/ask`、`/v1/ai/...`）。デモが本物のエージェントを呼ぶことはありません。`GET /v1/agent` は答え、`ai_topology` はデモの `features` に入りません。

## POST /v1/agent/ask

DAGON への 1 つの質問です。同じリクエストで答えます（作業もイベントもありません）。

本文:

```json
{
  "question": "これは何が問題？",
  "kind": "problem",
  "selection": { "node": "seg" },
  "history": [
    { "role": "user", "text": "次は？" },
    { "role": "dagon", "text": "ビルドします。" }
  ]
}
```

- `question`（必須）: 1 から 2000 文字で、空白だけではないもの。4 つの決まった質問では、そのラベル（`これは何が問題？` など）を `kind` と一緒に送ります。
- `kind`（任意、既定は `free`）: `problem` | `next` | `propose` | `failure` | `free`。
- `selection`（任意。構成全体なら null）: `{ "node": "<id>" }` または `{ "link": { "from": "<id>", "to": "<id>" } }`。id は単純な名前です（`A-Z a-z 0-9 . _ -` と空白で 40 文字まで）。
- `history`（任意）: 10 ターンまで、古い順。`role` は `user` | `dagon`、`text` は 1 から 2000 文字（文字だけ）。
- `rollback`（任意。ページが、前のバージョンに戻った書き込みを表示しているあいだに送ります）: その書き込みの記録。次の欄だけを受け取ります。

  ```json
  {
    "node": "watch",
    "failed_version": "1.1.0",
    "returned_version": "1.0.0",
    "reason": "ROLLED BACK",
    "node_state": { "trial": false, "last_reason": 3 }
  }
  ```

  `node`、バージョン、`reason` は、id と同じく単純な名前です。`node_state` は、そのときノードの STATUS が報告したもの（`last_reason`: そのコード、0 から 255）で、ノードが答えなかったときは `null` です。

200:

```json
{
  "answer": "試す回数を 2 回に減らす案です。\n\n...",
  "proposal": { "yaml_diff": " dagon: 1\n-    max_attempts: 3\n+    max_attempts: 2\n...",
                "topology": "dagon: 1\nname: ...\n" }
}
```

- `answer`: 日本語の平文です。最初の文が質問に答えます（吹き出しのため 110 文字まで）。詳しいことがあれば、空行の後に続きます（会話のタブのため）。
- `proposal`: `null`、または `PUT /v1/topology` と同じ確認をすでに通った（エラー 0 件）新しい構成の全体です。`yaml_diff` は、いまのファイルと案のすべての行を 1 行ずつ挙げ、`' '`（そのまま）、`'-'`（消す）、`'+'`（足す）を先頭に付けます。適用するには、`PUT /v1/topology` に `topology` の文字列を渡します。それまで何も変わりません。
- 案はファイル全体です。いまのファイルのコメントは入りません。エージェントの案が 3 回の呼び出しの後でも確認を通らないときは、案なしで答え、「提案した構成は確認で誤りが残ったため、出していません。」で終わります。

エラー（400 以外はすべて `{ "error": "...", "code": "..." }`）:

| ステータス | コード | いつ |
|---|---|---|
| 400 | （なし） | 本文が JSON でない、または欄が上のルールに反する |
| 403 | （なし） | `dagon demo`、または Host/Origin の確認 |
| 401 | （なし） | 合言葉が無い |
| 409 | `AGENT_NOT_LINKED` | つないでいない、またはつないだエージェントやつなぎ方がもう定義されていない |
| 409 | `AGENT_NOT_LINKED` | ほかに: 質問に答えているあいだに切断された（それ以上呼び出しません） |
| 409 | `AGENT_CONSENT_VERSION` | ~/.dagon/agent.json がこの dagon の読まないバージョン。`error` がどれかと、つなぎ直すことを示します |
| 409 | `AGENT_UNAVAILABLE` | つないでいるが、ここではそのつなぎ方を使えない。プログラムが PATH に無い、またはキーが登録されていない（`error` がプログラム、またはつなぎ方とキーの登録の仕方を示します） |
| 429 | `AGENT_BUSY` | ほかの質問に答えている（serve ごとに 1 つずつ） |
| 413 | `AGENT_CONTEXT_TOO_LARGE` | 構成のファイルが 64 KiB を超える（読みません）、または 1 つのプロンプトが 192 KiB を超える（送りません） |
| 504 | `AGENT_TIMEOUT` | 1 回のエージェントの呼び出しが 120 秒を超えた（`command` のつなぎ方のプログラムは止めます） |
| 502 | `AGENT_FAILED` | エージェントが断った、プログラムがエラーで終わった、または予期しない形で答えた。`error` がどれかを示します（プログラムの出力は含みません。API からのエラーは 300 文字で切り、API がキーをそのまま返しても、答えにもエラーにもキーは現れません） |

エージェントに届くもの（これだけで、どのエージェントとつなぎ方でも同じ 2 つの文です）: システムの文（DAGON の指示、構成のスキーマと確認、使っている定義の許可された事実、返答の形式）と、ユーザーの文（質問を 1 つの JSON 文書にしたもの。コメントを除いたデータとしての構成で、単純な名前でない文字列とキーはすべて `project-text` にしたもの、そうした語を隠した確認の結果、選択、最後に終わった 5 つの作業の種類と結果、リクエストにあれば `rollback` の記録（`last_reason` は言葉で）、履歴、質問。利用者の言葉は、その中のデータです）。システムの文は、前のバージョンに戻った書き込みをその記録だけで説明すること、見えないことはそう言うこと、理由を作らないことをエージェントに指示します。決して送らないもの: キー、ファイルのパス、コマンド、ログ、作業のエラー。隠した値がある構成には、ファイルの案を出しません（その場合、答えは「この構成には DAGON に送らなかった値があるため、変更の案は出していません。」で終わり、修復の呼び出しもしません）。`project-text` を含む案も受け付けません。

192 KiB の上限は、どのつなぎ方でも、呼び出しのたびにシステムの文とユーザーの文を合わせて測ります。

## エージェントの定義

組み込み: `crates/dagon-cli/agents/claude.toml`。利用者は `~/.dagon/agents/*.toml` にファイルを足せます（プロジェクトのフォルダには置けません。つなぎ方はプログラムを指定できるからです）。ファイル 1 つがエージェント 1 つです。知らないキー、DAGON が実装していない種類、不正な id、すでにあるエージェントの id があるファイルは読み込みません（上の `errors`）。

**command のエージェントは、あなたの権限で動きます。信頼できるプログラムだけを足してください。** DAGON は、つなぎ方が指定するプログラムをサンドボックスに入れませんし、入れているとも言いません。DAGON が制御するのは、送るもの（下のプロンプト）と、プログラムが受け継ぐもの（環境変数の許可リスト、新しい作業フォルダ、時間と出力の上限）です。プログラムがそれ以外に何をするかは、プログラム次第です。組み込みの Claude Code のつなぎ方は、Claude Code 自身のフラグで、そのツール、MCP サーバ、カスタマイズ、保存したセッションを無効にしています。

```toml
dagon_agent = 1          # the definition format; another number is refused with the reason
id = "my-agent"          # a-z 0-9 -, up to 32
name = "My agent"
provider = "Me"

[[ways]]
id = "local"
name = "Local program"
[ways.transport]
kind = "command"         # or "messages-http"
program = "my-agent"     # on PATH, or a path
args = ["--answer-only", "--system", "{system_file}"]
system = "file"          # "stdin" (default): system text, a blank line, then the user text on stdin
pass_env = ["MY_AGENT_HOME"]  # optional: more variables the program needs
```

すべてのキー、その型、必須かどうかは[エージェント定義の形式](/reference/agent-definition/)にあります。dagon がファイルを読み込む型から生成しています。`kind` はそれぞれ、下の転送の種類のどれかです。

エージェントは 1 つの JSON オブジェクトで返答します（返答の形式はシステムの文にあります）。DAGON はその文字列からオブジェクトを読みます。

## 転送の種類

- `command`: `program` を `args` で実行します。ユーザーの文は stdin に渡します（`system = "stdin"` のときはシステムの文の後に。`system = "file"` のときは、システムの文はファイルにあり、そのパスが引数 `{system_file}` に置き換わります）。stdout が答えです（1 MiB まで、UTF-8）。stderr は読みません。
  - 環境変数: `PATH`、`PATHEXT`、`HOME`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、`TEMP`、`TMP`、`TMPDIR`、`SYSTEMROOT`、`WINDIR`、`COMSPEC`、`LANG`、`LC_*`、`TERM`（設定されているもの）と、`pass_env` の名前だけです。`DAGON_*`、署名の鍵の変数、`ANTHROPIC_API_KEY` は、挙げられていても渡しません。
  - 作業フォルダ: 新しい一時フォルダで、後で消します。プログラムがプロジェクトの中で始まらないようにするためです。これは衛生のためで、サンドボックスではありません。プログラムは、あなたのアカウントが届くものには届きます。
  - 寿命: プログラムは OS のグループの中で動きます。Windows のジョブオブジェクト（`command-group` の crate を通します。閉じると、dagon 自身が終わるときも含め、中のすべてのプロセスを終わらせます）か、プログラムを先頭とする Unix のプロセスグループです。グループは、呼び出しの時間が切れたときと、プログラム自身が終わったときにすぐ止めます。そのため、プログラムが始めて残したプロセスは（stdout を握っていてもいなくても）呼び出しより長く生きません。答えは、プログラムが終わる前に出力したものです。
- `messages-http`: `url`（https だけ）、`version`（`anthropic-version` のヘッダ）、`model`、任意の `model_env`（`model` を上書きする変数）。キーは、OS の資格情報ストアにあるそのつなぎ方の項目です（「API キーの置き場所」）。キーの変数（`key_env`）を指定する定義は受け付けません。答えは 1 MiB まで読みます。それより大きいものは、読んでいる途中で受け付けなくなります。

## 同意の置き場所

`~/.dagon/agent.json`（`{"version":1,"agent":"claude","way":"claude-code","at":"..."}`）。利用者ごとに 1 つで、`dagon serve` と `dagon ai link` / `dagon ai unlink` が共有します。プロジェクトごとではなく利用者ごとなのは、エージェントのログインやキーは利用者のものであること、プロジェクトごとの同意ではプロジェクトのたびに尋ねることになること、コミットされるかもしれないプロジェクトのフォルダに何も置かないことのためです。解析できない、別の `version` である、ほかの欄を持つ、単純でない id を持つ、または `at` が実在の日付の `YYYY-MM-DDTHH:MM:SSZ` でないファイルは、つないでいないものとして扱います。利用者はもう一度つなぎます。

## 組み込みのエージェント

dagon に入っているエージェント（`crates/dagon-cli/agents/*.toml`）とそのつなぎ方は、順に[エージェント定義の形式](/reference/agent-definition/#組み込みのエージェント)にあります。最初のエージェントの最初のつなぎ方が既定です。

## つなぐと送るもの

### 送るもの

- 構成と定義の事実（ノード・リンク・部品・設定値）
- 選んだノードやリンク
- 確かめた結果（問題・◇ 未対応・書き込みの記録）

### 送らないもの

- 鍵
- コマンドとその出力の全文
- ファイルのパス

## 形式番号

ファイルの形式にはそれぞれ番号があります。読む側は、知らない番号を受け付けず、どの番号を読むかを示します。

- `dagon_agent = 1`: エージェントの定義ファイル
- `"version": 1`: 同意のファイル、~/.dagon/agent.json

## CLI

`dagon ai` のコマンド（`link`、`unlink`、`key`、`ask`、`topology`）とそのオプションは、[コマンド](/reference/cli/#dagon-ai)にあります。つなぐときは同意の文（エージェントとつなぎ方、ログインやキーの置き場所、送るもの、送らないもの、切断の仕方）を表示し、`y` で同意を記録します。どの AI のコマンドも、DAGON をつなぐまで受け付けません。`dagon ai ask` が構成のファイルを変えることはありません。

## command のつなぎ方を、入っているプログラムで確かめる

組み込みのつなぎ方のフラグと、それぞれの理由は、その定義（`crates/dagon-cli/agents/*.toml`）にあります。ここでは繰り返しません。claude-code のつなぎ方は、DAGON 自身の起動の経路（環境変数の許可リスト、新しい作業フォルダ、システムのファイル）で、`cargo test -p dagon-cli claude_code_answers -- --ignored` で実行できます（既定では無視します。ネットワークと利用者のログインを使い、1 行を送り、プロジェクトのデータは送りません）。
