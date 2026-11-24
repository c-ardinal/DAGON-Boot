---
title: 規約と部品
description: 規約ごとの形式の番号と、部品ごとのバージョンです。
editUrl: false
---

:::note
`compat.toml`, `crates/dagon-proto/src/lib.rs`から生成しています。直接編集せず、`python site/scripts/gen.py`で再度生成してください。
:::

DAGONの部品間の規約とその番号、部品ごとのバージョンと、読込と書込の対象とする形式番号です。
いずれも`compat.toml`の記載と同一です。変更時の規則は
[バージョンとリリース](/explanation/versioning/)にあります。

部品のバージョンは、その部品の最終リリースの値です（初回リリース前は0.1.0）。形式の番号は本ツリーの値です。
タグ`<部品>-v<バージョン>`以後の変更は、次回のリリースまで未リリースです。

## 規約

| 規約 | 形式の番号 | 番号の格納先 |
|---|---|---|
| `wire` | 2 | ホスト、コーディネータ、他のノード間のフレームとペイロード: 全フレームのバイト2、HELLOのproto_ver（dagon-proto PROTO_VERSION） |
| `image-header` | 1 | 署名付きの512バイトのイメージのヘッダ: オフセット0x004（dagon-format header::HDR_VERSION） |
| `state-log` | 1 | ブートローダとアプリの状態ログの面: オフセット0x004（dagon-format statelog::FORMAT_VERSION） |
| `manifest` | 1 | リリースのマニフェスト: オフセット4（dagon-format manifest::MF_VERSION） |
| `bundle` | 1 | リリースのファイル.dgb: オフセット4（dagon-format bundle::BN_VERSION） |
| `topology-bin` | 2 | コンパイル済の構成: オフセット4（dagon-format topology::TP_VERSION） |
| `topology-yaml` | 2 | 構成のソース: その`dagon:`キー（dagon-topology SCHEMA_VERSION） |
| `definitions` | 3 | dagon-mcu.toml / dagon-board.toml / [board.&lt;id&gt;]: `format`キー。省略時は1（dagon-topology defs::FORMAT） |
| `serve-api` | 1 | dagon serveのHTTP API: GET /v1/helloの`api`（dagon-cli versions::SERVE_API） |
| `layout` | 1 | 構成の隣の配置ファイル: その`format`（dagon-cli versions::LAYOUT_FORMAT） |
| `event` | 1 | ライフサイクルイベントの種類、コード、ペイロード、EventRecord（dagon-events EVENT_FORMAT） |
| `extension-manifest` | 1 | dagon-extension.toml: その`format`キー（dagon-extension MANIFEST_FORMAT） |
| `boot-program` | 1 | ボードの[boot]にあるブートのアクションプログラムの操作: 生成されるBOOT_PROGRAM_FORMAT（dagon-topology defs::board_boot::BOOT_PROGRAM_FORMAT） |
| `project` | 1 | dagon.toml: その`format`キー、必須（dagon-topology project::FORMAT） |
| `agent-def` | 1 | エージェントの定義（agents/*.toml、~/.dagon/agents/*.toml）: その`dagon_agent`キー（dagon-cli agents::FORMAT） |
| `agent-link` | 1 | エージェントへの同意の記録、~/.dagon/agent.json: その`version`（dagon-cli consent::FILE_VERSION） |
| `check-json` | 1 | JSON形式の確認の報告（dagon topo check --json、ページの確認）: その`format`（dagon-topology CHECK_JSON_FORMAT） |
| `conform-result` | 1 | 適合試験の結果ファイル: その`schema`の"dagon-conform/N"のN（dagon-conform report::SCHEMA_VERSION） |

## 部品

| 部品 | バージョン（最終リリース） | crate | 形式（本ツリー） |
|---|---|---|---|
| `cli` | 0.1.0 | `dagon-cli` | `wire` 2, `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 2, `topology-yaml` 2, `definitions` 3, `serve-api` 1, `layout` 1, `event` 1, `extension-manifest` 1, `boot-program` 1, `project` 1, `agent-def` 1, `agent-link` 1, `check-json` 1, `conform-result` 1 |
| `boot-rp2040` | 0.1.0 | `dagon-rp2040-stage0`, `dagon-rp2040-bootcore` | `image-header` 1, `state-log` 1, `event` 1 |
| `boot-stm32f401re` | 0.1.0 | `dagon-stm32f401re-stage0`, `dagon-stm32f401re-bootcore` | `image-header` 1, `state-log` 1, `event` 1 |
| `mcu-rp2040` | 0.1.0 | `dagon-mcu-rp2040` | `wire` 2, `image-header` 1, `state-log` 1, `definitions` 3 |
| `mcu-stm32f401re` | 0.1.0 | `dagon-mcu-stm32f401re` | `wire` 2, `image-header` 1, `state-log` 1, `definitions` 3 |
| `mcu-esp32s3` | 0.1.0 | `dagon-mcu-esp32s3` | `wire` 2, `image-header` 1, `state-log` 1, `definitions` 3 |
| `mcu-common` | 0.1.0 | `dagon-mcu-common` | `wire` 2 |
| `node` | 0.1.0 | `dagon-node` | `wire` 2, `image-header` 1, `state-log` 1 |
| `mcu-build` | 0.1.0 | `dagon-mcu-build` |  |
| `board-waveshare-rp2040-touch-lcd-1.28` | 0.1.0 | `dagon-board-waveshare-rp2040-touch-lcd-1-28` | `definitions` 3 |
| `board-nucleo-f401re` | 0.1.0 | `dagon-board-nucleo-f401re` | `definitions` 3 |
| `board-m5stopwatch` | 0.1.0 | `dagon-board-m5stopwatch` | `definitions` 3 |
| `format` | 0.1.0 | `dagon-format` | `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 2 |
| `ed25519` | 0.1.0 | `dagon-ed25519` |  |
| `proto` | 0.1.0 | `dagon-proto` | `wire` 2 |
| `core` | 0.1.0 | `dagon-core` | `image-header` 1, `state-log` 1 |
| `boot` | 0.1.0 | `dagon-boot` | `state-log` 1, `event` 1 |
| `events` | 0.1.0 | `dagon-events` | `event` 1 |
| `port` | 0.1.0 | `dagon-port` |  |
| `agent` | 0.1.0 | `dagon-agent` | `wire` 2, `image-header` 1 |
| `relay` | 0.1.0 | `dagon-relay` | `wire` 2 |
| `host` | 0.1.0 | `dagon-host` | `wire` 2, `image-header` 1, `state-log` 1, `bundle` 1, `topology-bin` 2 |
| `extension` | 0.1.0 | `dagon-extension` | `extension-manifest` 1, `definitions` 3, `boot-program` 1 |
| `topology` | 0.1.0 | `dagon-topology` | `topology-bin` 2, `topology-yaml` 2, `definitions` 3, `boot-program` 1, `project` 1, `check-json` 1 |
| `sign` | 0.1.0 | `dagon-sign` | `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 2 |
| `sim` | 0.1.0 | `dagon-sim` | `wire` 2, `image-header` 1, `state-log` 1 |
| `conform` | 0.1.0 | `dagon-conform` | `wire` 2, `image-header` 1, `state-log` 1, `conform-result` 1 |
| `romlink` | 0.1.0 | `dagon-romlink` |  |

## フレーム（`wire`）

フレームの`flags`バイトの各ビットです。`dagon-proto`の`flags`と同一です。

| フラグ | 値 | 意味 |
|---|---|---|
| `RESP` | `0x01` | 同一の`seq`と`cmd`の要求に対する応答のフレームです。 |
| `EVENT` | `0x02` | ノードが要求なしに送信するイベント用の予約ビットです。送信する部品はなく、受信側はこのビットのあるフレームを無視します。 |
| `RETRY` | `0x04` | 期限内に応答がなかったため再送した要求です。 |

リンクごとのペイロードの上限（`max_payload`）:

| リンク | バイト数 | 意味 |
|---|---|---|
| `USB` | 1088 | PCとハブの間のUSB CDC。 |
| `UART` | 1024 | UARTのリンク。 |
| `I2C` | 256 | I2Cのリンク。 |
| `SMALLEST` | 64 | 媒体に関係なくリンクが保証する最小値です。1回の転送量がこの値未満のリンクは、フレームより下の層で分割と結合を実施し、最低でもこの値のバイト数を転送します。全ノードのHELLOとCAPSの応答はこの値以下のため、要求元はどのリンク経由でもノードの機能を取得可能です。 |
