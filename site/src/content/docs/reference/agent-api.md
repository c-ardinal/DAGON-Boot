---
title: "エージェントとの接続方法: serve API"
description: "DAGONがdagon serve経由でエージェントに到達する方法です。ルート、同意、キー、送信内容を説明します。"
---

DAGONは1つのインターフェースでエージェントと通信します。エージェントはデータです。エージェント（id、名称、提供元）と、そこに到達する接続方法があります。各接続方法はDAGONが実装する転送の種類のいずれかです（後述の「組込のエージェント」と「転送の種類」）。利用者は接続時に1度だけ、1つのエージェントと1つの接続方法に同意します。以後は再度の確認なしに質問を送信します。ログインはエージェント側で、APIキーはOSの資格情報ストアで管理します（「APIキーの格納場所」）。いずれもページには到達しません。

以下のルートは全部`dagon serve`の配下にあり、他の`/v1/`のルートと同一の確認があります。HostとOriginの確認（403）と、`Authorization: Bearer <passphrase>`（401）です。

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

- `agents`: 組込のエージェントが先頭で、その後が利用者のエージェント（`~/.dagon/agents/*.toml`）です。各接続方法には`kind`（`command` | `messages-http`。種類は増加する可能性があるため、未知の種類は内容を解釈しないでください）と`available`（このPCで現時点で使用可能か。`command`の接続方法はプログラムがPATHにあること、`messages-http`の接続方法はキーが登録済であること）があります。`key`: APIキーを使用する接続方法では、登録の有無（`true` / `false`）。使用しない接続方法では`null`です。この本文に、プログラム、引数、URL、キーを記載することはありません。
- `default`: 接続時のダイアログが事前に選択する接続方法（先頭のエージェントにある先頭の接続方法）。
- `linked`: 定義に残存するエージェントと接続方法に対する、同意の記録の有無。
- `consent`: 接続後は`{ "agent": "claude", "way": "claude-code", "at": "2026-11-04T04:14:35Z" }`（`at`はUTCのRFC 3339）、それ以外は`null`。
- `available` / `provider`: 接続済の接続方法（接続前は既定の接続方法）について、この環境での使用可否と、接続時の文に使用するエージェントの名称（`"<name> (<provider>)"`）。DAGONが応答可能なのは、`linked`と`available`が両方ともtrueの場合だけです。
- `errors`: 読込に失敗した利用者の定義ファイル。形式は`"<file name>: <why>"`です（フォルダは記載しません）。
- `consent_error`: ~/.dagon/agent.jsonがこのdagonが読込不可能なバージョンの場合は、その理由（再接続すると、読込可能な形式で記録します）。それ以外は`null`。
- `sends` / `never_sends`: 「接続時に送信するもの」の一覧で、接続時のダイアログが表示します。ターミナル（`dagon ai link`）も同一の一覧を表示します。

## POST /v1/agent/link

本文: `{ "agent": "claude", "way": "claude-code", "consent": true }`。そのエージェントと接続方法への同意を記録します（新規の接続は既存の同意を置換します。接続方法を変更すると、再度同意を要求します）。

- 200: GET /v1/agentと同一の本文（`linked: true`の状態です）。
- 400 `agent and way must name a defined agent and way` / `consent must be true`（JSONの真偽値`true`だけが有効です）。何も記録しません。
- 500 `the consent could not be saved`。

接続時に、その接続方法が使用可能である必要はありません（利用者は後からClaude Codeのインストールやキーの登録が可能です）。

## DELETE /v1/agent/link

同意を取消します（記録がない場合も成功します）。

- 200: GET /v1/agentと同一の本文（`linked: false`の状態です）。
- **切断は、実行中のエージェント呼出の完了を待機します。** 各呼出は確認から呼出の終了まで同意を保持します（`~/.dagon/agent.lock`の共有ロック。呼出ごとにハンドルが1つのため、スレッドでも、`dagon serve`と`dagon ai unlink`のようなプロセスでも、同様に排他します）。DELETEは即座に同意を削除し（呼出はロック下で再読込するため、その時点から新規の呼出は通過しません）、その後にロックを排他的に取得します。そのため、200を返却するのは実行中の呼出がない場合だけです。待機は最大で1回の呼出の分です。質問なら約120秒まで、構成の案の1ラウンドなら600秒までです。ページはDELETEの応答まで待機中の状態を表示してください。
- 以後は全呼出が確認を通過しません。実行中の`ai_topology`の作業は取消します。応答中の質問は、それ以上の呼出をせず、応答に替えて409 `AGENT_NOT_LINKED`を返却します（`dagon ai ask`も応答を表示しません）。

## PUT /v1/agent/key/{agent}/{way}

本文: `{ "key": "..." }`。接続方法のAPIキーをOSの資格情報ストアに登録します（既存のキーを置換します）。キーは前後の空白を除去して受理します。表示可能なASCII文字で1から1024文字である必要があります（HTTPのヘッダに格納するためです。Windows資格情報マネージャーの上限は2560バイトで、UTF-16で保存します）。キーを応答に記載することも、ログに記録することも、ファイルに出力することもありません。ページはキーをどこにも保存せず、送信後に入力欄を空白にします。

- 200: GET /v1/agentと同一の本文（接続方法の`key`が`true`の状態です）。
- 400 `this way takes no key`（ログインの接続方法）、`key must be a string`、またはキーの拒否理由。何も保存しません。
- 404 `no such agent and way`。
- 403: Host/Originの確認（他のオリジンは拒否します）。
- 503 `AGENT_KEY_STORE`: この環境ではOSの資格情報ストアを使用不可能です（例: Secret Serviceが停止中のLinux）。`error`が理由と、インストール方法や起動方法を提示します。他の保存先には何も保存しません。

## DELETE /v1/agent/key/{agent}/{way}

接続方法のキーをストアから削除します（登録がない場合も成功します）。

- 200: GET /v1/agentと同一の本文（接続方法の`key`が`false`の状態です）。
- 400 / 404 / 403 / 503はPUTと同一です。

## APIキーの格納場所

格納場所はOSの資格情報ストアだけです。Windows資格情報マネージャー、macOSのキーチェーン、LinuxのSecret Service（GNOME Keyring、KeePassXCなど）です。各キーはサービス`dagon`、アカウント`<agent id>/<way id>`（`claude/api`）の項目です。DAGONは環境変数からキーを読込せず、ファイルにキーを保存しません。ストアを使用不可能な環境では、登録を拒否します。キーをストアの外部で使用するのは、その接続方法のリクエストのヘッダに格納する場合だけです。

ストアを使用不可能な場合の`dagon ai key`の動作です（serveのPUTとDELETEは、前述のとおり503 `AGENT_KEY_STORE`です）。

- `dagon ai key set`と`dagon ai key delete`は、`error: <store> cannot be used (<reason>); <how to install or start one>. DAGON keeps API keys nowhere else`（storeはストアの名称、reasonは理由）を表示して、終了コード1で終了します。`set`はキーの入力の要求前に拒否します。
- `dagon ai key status`は終了コード0で終了し、キーを使用する各接続方法を`未登録 (<reason>)`と表示します。終了コードではストアの不在と登録の不在を区別しません。

## 接続前のAIのルート

`/v1/ai/...`の全ルートは接続するまで以下の応答を返却します。

```json nocheck
HTTP 409
{ "error": "DAGON is not linked to an agent; link it first (consent is asked once)",
  "code": "AGENT_NOT_LINKED" }
```

文ではなく`code`で判定してください。同意のファイルがこのdagonが読込不可能なバージョンの場合は、前述の全ルートで409 `AGENT_CONSENT_VERSION`を返却します。接続後のルートは、通常どおり動作します（`/v1/ai/topology`は作業を開始します。この環境で使用不可能な接続方法は、作業中に理由を表示して失敗します。例: `no API key is registered for claude/api`）。

## POST /v1/agent/ask

DAGONへの1つの質問です。同一のリクエストで応答します（作業もイベントもありません）。

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

- `question`（必須）: 1から2000文字で、空白だけではない文字列。4つの定型の質問では、そのラベル（`これは何が問題？`など）を`kind`と同時に送信します。
- `kind`（任意、既定は`free`）: `problem` | `next` | `propose` | `failure` | `free`。
- `selection`（任意。構成全体ならnull）: `{ "node": "<id>" }`または`{ "link": { "from": "<id>", "to": "<id>" } }`。idは単純な名称です（`A-Z a-z 0-9 . _ -`と空白で40文字まで）。
- `history`（任意）: 10ターンまで、時系列順。`role`は`user` | `dagon`、`text`は1から2000文字（テキストだけ）。
- `rollback`（任意。以前のバージョンに復帰した書込をページが表示中に送信します）: その書込の記録。以下の欄だけを受理します。

  ```json
  {
    "node": "watch",
    "failed_version": "1.1.0",
    "returned_version": "1.0.0",
    "reason": "ROLLED BACK",
    "node_state": { "trial": false, "last_reason": 3 }
  }
  ```

  `node`、バージョン、`reason`は、idと同様に単純な名称です。`node_state`は、その時点でノードのSTATUSが報告した内容（`last_reason`: そのコード、0から255）で、ノードが無応答の場合は`null`です。

200:

```json
{
  "answer": "試す回数を 2 回に減らす案です。\n\n...",
  "proposal": { "yaml_diff": " dagon: 1\n-    max_attempts: 3\n+    max_attempts: 2\n...",
                "topology": "dagon: 1\nname: ...\n" }
}
```

- `answer`: 日本語の平文です。最初の文が質問に回答します（吹出し用に110文字まで）。詳細があれば、空行の後に記載します（会話のタブ用）。
- `proposal`: `null`、または`PUT /v1/topology`と同一の確認を通過済の（エラー0件）新規の構成の全体です。`yaml_diff`は、既存のファイルと案の全行を1行ずつ列挙し、`' '`（無変更）、`'-'`（削除）、`'+'`（追加）を先頭に付与します。適用には`PUT /v1/topology`に`topology`の文字列を送信します。適用まで何も変化しません。
- 案はファイル全体です。既存のファイルのコメントは記載しません。エージェントの案が3回の呼出後も確認を通過しない場合は、案なしで回答し、回答の末尾は「提案した構成は確認で誤りが残ったため、出していません。」です。

エラー（400以外は全部`{ "error": "...", "code": "..." }`）:

| ステータス | コード | 条件 |
|---|---|---|
| 400 | （なし） | 本文がJSON以外、または欄が前述の規則に違反 |
| 403 | （なし） | Host/Originの確認 |
| 401 | （なし） | 合言葉がない |
| 409 | `AGENT_NOT_LINKED` | 未接続、または接続済のエージェントや接続方法が定義から削除済 |
| 409 | `AGENT_NOT_LINKED` | その他: 質問に応答中に切断された（以後の呼出はしません） |
| 409 | `AGENT_CONSENT_VERSION` | ~/.dagon/agent.jsonがこのdagonが読込不可能なバージョン。`error`がバージョンと、再接続が必要な旨を提示します |
| 409 | `AGENT_UNAVAILABLE` | 接続済だが、この環境ではその接続方法を使用不可能。プログラムがPATHにない、またはキーが未登録（`error`がプログラム、または接続方法とキーの登録方法を提示します） |
| 429 | `AGENT_BUSY` | 他の質問に応答中（serveごとに1件ずつ） |
| 413 | `AGENT_CONTEXT_TOO_LARGE` | 構成のファイルが64 KiBを超過（読込しません）、または1つのプロンプトが192 KiBを超過（送信しません） |
| 504 | `AGENT_TIMEOUT` | エージェントの呼出1回が120秒を超過（`command`の接続方法のプログラムは停止します） |
| 502 | `AGENT_FAILED` | エージェントの拒否、プログラムのエラー終了、または予期しない形式の応答。`error`がその種別を提示します（プログラムの出力は記載しません。APIからのエラーは300文字で切捨し、APIがキーを無変更で返却しても、応答とエラーのいずれにもキーは出現しません） |

エージェントへの送信内容は以下だけです。どのエージェントと接続方法でも同一の2つの文です。システムの文は、DAGONの指示、構成のスキーマと確認、使用中の定義のうち許可された事実、返答の形式です。ユーザーの文は質問を1つのJSON文書にしたものです。その内容は、コメントを除外したデータとしての構成（単純な名称以外の文字列とキーは全部`project-text`に置換）、それらの語を隠蔽した確認の結果、選択、最後に終了した5つの作業の種類と結果、リクエストにあれば`rollback`の記録（`last_reason`は文章で表現）、履歴、質問です。利用者の文章はその内部のデータです。システムの文は、以前のバージョンに復帰した書込をその記録だけで説明すること、不明な事項はその旨を明言すること、理由を捏造しないことをエージェントに指示します。決して送信しないものは、キー、ファイルのパス、コマンド、ログ、作業のエラーです。隠蔽した値がある構成には、ファイルの案を出力しません（その場合、回答の末尾は「この構成にはDAGONに送らなかった値があるため、変更の案は出していません。」で、修復の呼出もしません）。`project-text`を記載した案も拒否します。

192 KiBの上限は全接続方法で、呼出ごとにシステムの文とユーザーの文の合計で計測します。

## エージェントの定義

組込: `crates/dagon-cli/agents/claude.toml`。利用者は`~/.dagon/agents/*.toml`にファイルを追加可能です（プロジェクトのフォルダには配置不可です。接続方法でプログラムを指定可能なためです）。ファイル1つがエージェント1つです。未知のキー、DAGONが未実装の種類、不正なid、既存のエージェントのidのいずれかがあるファイルは読込しません（前述の`errors`）。

**commandのエージェントは、利用者の権限で動作します。信頼可能なプログラムだけを追加してください。** DAGONは接続方法が指定するプログラムをサンドボックスで隔離しません。隔離しているとも主張しません。DAGONが制御するのは、送信内容（後述のプロンプト）と、プログラムが継承するもの（環境変数の許可リスト、新規の作業フォルダ、時間と出力の上限）です。それ以外のプログラムの動作は、そのプログラムに依存します。組込のClaude Codeの接続方法は、Claude Code自身のフラグで、そのツール、MCPサーバ、カスタマイズ、保存済のセッションを無効にしています。

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

全キー、その型、必須か否かは[エージェント定義の形式](/reference/agent-definition/)にあります。dagonがファイルの読込に使用する型から生成しています。各`kind`は後述の転送の種類のいずれかです。

エージェントは1つのJSONオブジェクトで返答します（返答の形式はシステムの文にあります）。DAGONはその文字列からオブジェクトを読込します。

## 転送の種類

- `command`: `program`を`args`で実行します。ユーザーの文はstdinに入力します（`system = "stdin"`の場合はシステムの文の後に配置します。`system = "file"`の場合はシステムの文をファイルに格納し、そのパスが引数`{system_file}`を置換します）。stdoutが応答です（1 MiBまで、UTF-8）。stderrは読込しません。
  - 環境変数: `PATH`、`PATHEXT`、`HOME`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、`TEMP`、`TMP`、`TMPDIR`、`SYSTEMROOT`、`WINDIR`、`COMSPEC`、`LANG`、`LC_*`、`TERM`（設定済のもの）と、`pass_env`に列挙した変数だけです。`DAGON_*`、署名のキーの変数、`ANTHROPIC_API_KEY`は、列挙されていてもプログラムに付与しません。
  - 作業フォルダ: 新規の一時フォルダで、使用後に削除します。プログラムがプロジェクト内で開始しないようにするためです。これは衛生上の措置で、サンドボックスではありません。プログラムは利用者のアカウントがアクセス可能な対象には全部アクセス可能です。
  - 寿命: プログラムはOSのグループ内で動作します。Windowsのジョブオブジェクト（`command-group`のcrate経由です。ジョブオブジェクトのクローズ時は、dagon自身の終了時も対象として、内部の全プロセスを終了させます）か、プログラムを先頭とするUnixのプロセスグループです。グループは呼出の時間切れ時と、プログラム自身の終了時に即座に停止します。そのため、プログラムが起動して残存させたプロセスは（stdoutの保持に関係なく）呼出の終了後は存続しません。応答はプログラムの終了前の出力です。
- `messages-http`: `url`（httpsだけ）、`version`（`anthropic-version`のヘッダ）、`model`、任意の`model_env`（`model`を上書きする変数）。キーはOSの資格情報ストアにあるその接続方法の項目です（「APIキーの格納場所」）。キーの変数（`key_env`）を指定する定義は拒否します。応答は1 MiBまで読込します。それを超過する応答は、読込の途中で拒否します。

## 同意の格納場所

`~/.dagon/agent.json`（`{"version":1,"agent":"claude","way":"claude-code","at":"..."}`）。利用者ごとに1つで、`dagon serve`と`dagon ai link` / `dagon ai unlink`が共有します。プロジェクトごとではなく利用者ごとにする理由は以下のとおりです。エージェントのログインやキーは利用者に帰属します。プロジェクトごとの同意では、プロジェクトのたびに確認が必要になります。コミットの可能性があるプロジェクトのフォルダに何も保存しないためでもあります。解析不能なファイル、別の`version`のファイル、他の欄があるファイル、単純ではないidがあるファイル、`at`が実在の日付の`YYYY-MM-DDTHH:MM:SSZ`ではないファイルは、未接続として処理します。利用者は再度接続します。

## 組込のエージェント

dagonに内蔵のエージェント（`crates/dagon-cli/agents/*.toml`）とその接続方法は、順に[エージェント定義の形式](/reference/agent-definition/#組込のエージェント)にあります。先頭のエージェントにある先頭の接続方法が既定です。

## 接続時に送信するもの

### 送信するもの

- 構成と定義の事実（ノード・リンク・部品・設定値）
- 選んだノードやリンク
- 確認した結果（問題・◇ 制限・書き込みの記録）

### 送信しないもの

- 鍵
- コマンドとその出力の全文
- ファイルのパス

## 形式番号

各ファイルの形式には番号があります。読込側は未知の番号を拒否し、読込可能な番号を提示します。

- `dagon_agent = 1`: エージェントの定義ファイル
- `"version": 1`: 同意のファイル、~/.dagon/agent.json

## CLI

`dagon ai`のコマンド（`link`、`unlink`、`key`、`ask`、`topology`）とそのオプションは、[コマンド](/reference/cli/#dagon-ai)にあります。接続時は同意の文（エージェントと接続方法、ログインやキーの格納場所、送信するもの、送信しないもの、切断の方法）を表示し、`y`で同意を記録します。全AIコマンドは、DAGONの接続まで処理を拒否します。`dagon ai ask`が構成のファイルを変更することはありません。

## commandの接続方法の、インストール済のプログラムによる確認

組込の接続方法のフラグと、各フラグの理由は、その定義（`crates/dagon-cli/agents/*.toml`）にあります。このページでは記載しません。claude-codeの接続方法は、DAGON自身の起動経路（環境変数の許可リスト、新規の作業フォルダ、システムのファイル）で、`cargo test -p dagon-cli claude_code_answers -- --ignored`で実行可能です（既定では無視します。ネットワークと利用者のログインを使用し、1行を送信し、プロジェクトのデータは送信しません）。
