---
title: Serve HTTP API
description: dagon serveのすべてのルートです。
editUrl: false
---

:::note
`crates/dagon-cli/src/serve/api.rs`から生成しています。直接編集せず、`python site/scripts/gen.py`で再度生成してください。
:::

どのルートも`dagon serve`が表示するループバックのアドレスの下にあります。
`/v1/`へのリクエストには、serveが表示したアドレスの合言葉を
`Authorization: Bearer <passphrase>`で付与します（不在の場合は401）。HostとOriginの確認も実施します（不合格の場合は403）。

| メソッド | パス | 内容 |
|---|---|---|
| `GET` | `/v1/hello` | バージョン、作業領域、可能な操作。キーの有無（キー自体は対象外）。 |
| `GET` | `/v1/agent` | エージェントとその接続方法（それぞれの使用可否。キー、パス、コマンドは対象外）、同意、接続時の送信内容。 |
| `POST` | `/v1/agent/link` | そのエージェントにその接続方法で送信することへの利用者の同意を記録します。定義済のエージェントと接続方法への明示的な同意以外は、何も記録しません。 |
| `DELETE` | `/v1/agent/link` | 同意を取消します（同意が不在の場合も完了します）。 |
| `PUT` | `/v1/agent/key/{agent}/{way}` | 接続方法のキーをOSの資格情報ストアに登録します。応答はGET /v1/agentの本文です（キーの登録の有無で、キー自体は対象外です）。 |
| `DELETE` | `/v1/agent/key/{agent}/{way}` | 接続方法のキーを削除します（未登録の場合も成功します）。 |
| `POST` | `/v1/agent/ask` | DAGONへの1つの質問。このリクエストで応答します。 |
| `GET` | `/v1/definitions` | 使用中の定義を`Registry::to_json`の形式で。ページのdagon-wasm用です（`load_definitions`）。 |
| `GET` | `/v1/topology` | ディスク上の、プロジェクトの構成のYAML（初回保存前は404）。 |
| `PUT` | `/v1/topology` | `dagon topo check`と同一の確認の後に、構成のYAMLを保存します。 |
| `GET` | `/v1/nodes/{id}/build` | dagon.tomlにあるノードの`[build.<id>]`テーブル（不在の場合は404）。 |
| `PUT` | `/v1/nodes/{id}/build` | dagon.tomlの`[build.<id>]`を置換します。ファイルの他の部分は変更しません。 |
| `POST` | `/v1/build` | `nodes`（省略時は全部）のアプリケーションを作業としてビルドします。応答にはビルドした各スロットとそのサイズを列挙します。 |
| `POST` | `/v1/sign` | `nodes`のビルド済のアプリケーションに、CLIのキーで署名します。 |
| `POST` | `/v1/release` | 全部の署名付きイメージからリリースを作成します。各イメージのバージョンは、ノードのcrateが付与したバージョンです（`version`を指定する場合、全イメージのバージョンと一致する必要があります）。 |
| `GET` | `/v1/release/latest` | プロジェクトのout/releaseで最新の.dgb。 |
| `GET` | `/v1/release/{id}` | プロジェクトのout/releaseにあるリリースを名称で返却します。プロジェクトのキーで検証に合格した場合だけです。 |
| `GET` | `/v1/kept` | プロジェクトに保存済のパッケージ（out/deploy）。最新バージョンから順番に、各ノードに付与するバージョンと同時に返却します。 |
| `GET` | `/v1/kept/{id}` | 保存済のパッケージのバイト列。検証に合格した場合だけ返却します。 |
| `POST` | `/v1/kept` | これから書込するパッケージを保存します。プロジェクトのキーで検証に合格した場合だけです。そのidを応答します。 |
| `GET` | `/v1/bootcore/{node}` | ノードの署名付きのブートローダ（ROMの経路）: [provision.&lt;node&gt;] bootcore、不在の場合は&lt;provision_dir&gt;/&lt;node&gt;/bootcore.img。 |
| `GET` | `/v1/stage0/{node}` | stage0のあるマイコンをROMの経路で全体再書込する際のstage0のバイト列。provisionが出力したもので、&lt;provision_dir&gt;/&lt;node&gt;/stage0.binです。stage0に署名はありません。ホストは書込前に、マイコンのstage0の領域に収容可能かを確認します。 |
| `GET` | `/v1/layout` | 表示名とエディタの位置（初回保存前は未設定の配置）。 |
| `PUT` | `/v1/layout` | この形式の配置ファイル。全体を一括で記述します。 |
| `POST` | `/v1/conform` | ページによるポートの受渡後に、PC接続ノード経由で`node`に12の適合試験を実施します。 |
| `GET` | `/v1/conform/{node}` | ノードの直近の適合試験結果（初回実行前は404）。 |
| `POST` | `/v1/ai/topology` | 作業。モデルからの質問はページへ送信します。 |
| `POST` | `/v1/ai/port` | DAGONのボード移植: 応答は501です。 |
| `GET` | `/v1/jobs/{id}` | 作業の状態、段階、進捗、ログの行、質問、結果。 |
| `POST` | `/v1/jobs/{id}/answer` | 作業の未回答の質問への`{answer}`（質問が不在の場合は409）。 |
| `POST` | `/v1/jobs/{id}/cancel` | 実行中の作業を停止します。 |
| `POST` | `/v1/device` | `{state: "released"}`: ページがPC接続ノードのポートをクローズしたので、そのポートを待機中のCLIの作業が使用可能です。 |
| `GET` | `/v1/project` | オープン中のプロジェクトと、最近のプロジェクト。 |
| `POST` | `/v1/project/open` | dagon.tomlのあるフォルダ。 |
| `POST` | `/v1/project/new` | その位置で`dagon new`を実行します（from_sampleが不在ならノードなしの構成）。キーの組が不在なら~/.dagon/keys/&lt;name&gt;に作成します。 |
| `POST` | `/v1/project/init` | 構成はあるがdagon.tomlが不在のフォルダに、そのYAML（`yaml`、またはフォルダ内の唯一のYAML）用のdagon.tomlを作成します。キーの組が不在なら~/.dagon/keys/&lt;folder&gt;に作成します。その後にオープンします。 |
| `GET` | `/v1/fs` | ホーム配下のフォルダ内のフォルダ（既定はホーム）。dagon.tomlがあるフォルダにマークを付与します。キーのフォルダは除外します。 |
| `POST` | `/v1/gen` | オープン中のプロジェクトで`dagon gen`を実行します。dagon.tomlはファイルのロック下で編集します。 |
| `POST` | `/v1/flash` | `device`は、書込の手段が報告する候補の1つを指定します。候補が複数で指定が不在の場合、作業は候補を提示して終了し（written: false）、何も書込しません。 |
| `POST` | `/v1/doctor` | 定義に準拠した、プロジェクトのマイコンに必要なツール。ここではコマンドの確認が不可能なので、プロジェクト自身が定義する確認は、ターミナルの`dagon doctor`で実施する確認として報告します。 |

`/v1/events`はWebSocketです。ページが最初のメッセージで合言葉を送信すると、serveのイベント
（作業の進捗、ログ、結果、質問、PC接続ノードの受渡）を受信します。
