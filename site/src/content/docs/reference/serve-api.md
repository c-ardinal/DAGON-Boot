---
title: Serve HTTP API
description: dagon serve のすべてのルートです。
editUrl: false
---

:::note
`crates/dagon-cli/src/serve/api.rs` から生成しています。直接編集せず、`python site/scripts/gen.py` でもう一度生成してください。
:::

どのルートも、`dagon serve` が表示するループバックのアドレスの下にあります。
`/v1/` へのリクエストには、serve が表示したアドレスの合言葉を
`Authorization: Bearer <passphrase>` で付けます（無いと 401）。Host と Origin の確認も通ります（通らないと 403）。

| メソッド | パス | `dagon demo` | 内容 |
|---|---|---|---|
| `GET` | `/v1/hello` |  | バージョン、作業領域、できること。鍵があるかどうか（鍵そのものは含みません）。 |
| `GET` | `/v1/agent` |  | エージェントとそのつなぎ方（それぞれここで使えるか。キー、パス、コマンドは含みません）、同意、つなぐと送るもの。 |
| `POST` | `/v1/agent/link` | 拒否 | そのエージェントにそのつなぎ方で送ることへの利用者の同意を記録します。定義されたエージェントとつなぎ方へのはっきりした同意でなければ、何も記録しません。 |
| `DELETE` | `/v1/agent/link` | 拒否 | 同意を取り消します（無いときも完了します）。 |
| `PUT` | `/v1/agent/key/{agent}/{way}` | 拒否 | つなぎ方のキーを OS の資格情報ストアに登録します。答えは GET /v1/agent の本文です（キーが登録されているかどうかで、キーそのものは含みません）。 |
| `DELETE` | `/v1/agent/key/{agent}/{way}` | 拒否 | つなぎ方のキーを消します（登録が無いときも成功します）。 |
| `POST` | `/v1/agent/ask` | 拒否 | DAGON への 1 つの質問。このリクエストで答えます。 |
| `GET` | `/v1/definitions` |  | 使っている定義を `Registry::to_json` の形で。ページの dagon-wasm のためです（`load_definitions`）。 |
| `GET` | `/v1/topology` |  | ディスクにあるとおりの、プロジェクトの構成の YAML（最初の保存の前は 404）。 |
| `PUT` | `/v1/topology` |  | `dagon topo check` と同じ確認の後で、構成の YAML を書きます。 |
| `GET` | `/v1/nodes/{id}/build` |  | dagon.toml にあるノードの `[build.<id>]` テーブル（無ければ 404）。 |
| `PUT` | `/v1/nodes/{id}/build` |  | dagon.toml の `[build.<id>]` を置き換えます。ファイルのほかの部分はそのままです。 |
| `POST` | `/v1/build` |  | `nodes`（省くとすべて）のアプリケーションを作業としてビルドします。答えは、ビルドした各スロットとその大きさを挙げます。 |
| `POST` | `/v1/sign` |  | `nodes` のビルド済みのアプリケーションに、CLI の鍵で署名します。 |
| `POST` | `/v1/release` |  | すべての署名付きイメージからリリースを作ります。各イメージのバージョンは、ノードの crate が与えたものです（`version` を与えるなら、すべてのイメージのバージョンと同じでなければなりません）。 |
| `GET` | `/v1/release/latest` |  | プロジェクトの out/release でいちばん新しい .dgb。 |
| `GET` | `/v1/release/{id}` |  | プロジェクトの out/release にあるリリースを名前で返します。プロジェクトの鍵で検証が通ったときだけです。 |
| `GET` | `/v1/kept` |  | プロジェクトに残してあるパッケージ（out/deploy）。新しいバージョンから順に、各ノードに与えるバージョンとともに返します。 |
| `GET` | `/v1/kept/{id}` |  | 残してあるパッケージのバイト列。検証が通ったときだけ返します。 |
| `POST` | `/v1/kept` |  | 書き込もうとしているパッケージを残します。プロジェクトの鍵で検証が通ったときだけです。その id を答えます。 |
| `GET` | `/v1/bootcore/{node}` |  | ノードの署名付きのブートローダ（ROM の経路）: [provision.&lt;node&gt;] bootcore、無ければ &lt;provision_dir&gt;/&lt;node&gt;/bootcore.img。 |
| `GET` | `/v1/stage0/{node}` |  | stage0 のあるマイコンを ROM の経路ですべて書き直すときの stage0 のバイト列。provision が書いたもので、&lt;provision_dir&gt;/&lt;node&gt;/stage0.bin です。stage0 に署名はありません。ホストは書き込む前に、マイコンの stage0 の領域に収まることを確かめます。 |
| `GET` | `/v1/layout` |  | 呼び名とエディタの位置（最初の保存の前は空の配置）。 |
| `PUT` | `/v1/layout` |  | この形式の配置ファイル。全体をまとめて書きます。 |
| `POST` | `/v1/conform` | 拒否 | ページがポートを渡した後で、親機を通して `node` に 12 の適合試験を行います。 |
| `GET` | `/v1/conform/{node}` | 拒否 | ノードの最後の適合試験の結果（最初の実行の前は 404）。 |
| `POST` | `/v1/ai/topology` | 拒否 | 作業。モデルからの質問はページに届きます。 |
| `POST` | `/v1/ai/port` | 拒否 | DAGON をボードに移植する作業: 501 を返します。この dagon に移植の作業はありません。 |
| `GET` | `/v1/jobs/{id}` |  | 作業の状態、段階、進み具合、ログの行、質問、結果。 |
| `POST` | `/v1/jobs/{id}/answer` |  | 作業の開いている質問への `{answer}`（無ければ 409）。 |
| `POST` | `/v1/jobs/{id}/cancel` |  | 実行中の作業を止めます。 |
| `POST` | `/v1/device` | 拒否 | `{state: "released"}`: ページが親機のポートを閉じたので、それを待っている CLI の作業が使えます。 |
| `GET` | `/v1/project` |  | 開いているプロジェクトと、最近のもの。 |
| `POST` | `/v1/project/open` | 拒否 | dagon.toml のあるフォルダ。 |
| `POST` | `/v1/project/new` | 拒否 | そこで `dagon new` を実行します（from_sample が無ければ空の構成）。鍵の組が無ければ ~/.dagon/keys/&lt;name&gt; に作ります。 |
| `POST` | `/v1/project/init` | 拒否 | 構成はあるが dagon.toml が無いフォルダに、その YAML（`yaml`、またはフォルダの唯一のもの）のための dagon.toml を作ります。鍵の組が無ければ ~/.dagon/keys/&lt;folder&gt; に作ります。その後で開きます。 |
| `GET` | `/v1/fs` | 拒否 | ホームの下のフォルダの中のフォルダ（既定はホーム）。dagon.toml があるものに印を付けます。鍵のフォルダは除きます。 |
| `POST` | `/v1/gen` |  | 開いているプロジェクトで `dagon gen` を実行します。dagon.toml はファイルのロックの下で編集します。 |
| `POST` | `/v1/flash` | 拒否 | `device` は、書き込みの手段が報告する候補の 1 つを示します。候補が複数あり指定が無いと、作業は候補を示して終わり（written: false）、何も書きません。 |
| `POST` | `/v1/doctor` | 拒否 | 定義に基づく、プロジェクトのマイコンに要るツール。ここではだれもコマンドを確認できないので、プロジェクト自身が定義する確認は、ターミナルの `dagon doctor` で行うものとして報告します。 |
| `POST` | `/v1/sim/write` |  | 模擬の親機の USB ポートに渡すバイト列。 |
| `GET` | `/v1/sim/read` |  | 模擬の親機の答え（待ち時間のあいだに何も来なければ空）。 |

`/v1/events` は WebSocket です。ページが最初のメッセージで合言葉を送ると、serve のイベント
（作業の進み具合、ログ、結果、質問、親機の受け渡し）を受け取ります。

「拒否」のルートは、`dagon demo` の実行中は 403 を返します。デモは自分の模擬の親機と子機、
自分のプロジェクトだけを使い、実機、ほかのプロジェクト、エージェントには触れません。
