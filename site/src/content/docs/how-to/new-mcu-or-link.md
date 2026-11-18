---
title: "マイコンとリンクの追加"
description: "DAGONに新規のマイコンやリンクを追加する場合の、記述対象と変更対象外の範囲です。"
---

層の図は[DAGONの構造](/explanation/)にあります。

## マイコンの追加

`crates/`は変更しません。マイコン固有の情報は`targets/mcu/<id>/`だけに配置します。

1. `targets/mcu/<id>/`にcrate `dagon-mcu-<id>`を作成します。このcrateには独自の`[workspace]`があります。ルートの`Cargo.toml`は変更しません。
2. `dagon-mcu.toml`を記述します。キーは[マイコンとボードの定義](/reference/definitions/)にあります。フォルダの追加で定義が増加します。
3. `crates/dagon-port/src/lib.rs`のトレイトを実装します: `Flash`、`Link`、`Reset`、`Watchdog`、ブートの種類に応じて`BootSelectS`か`BootSelectV`。コーディネータにするマイコンは`StreamPort`、`I2cBus`、`SubControl`も実装します。マイクロ秒単位のクロックはポートではなくDAGON自身の機能です。`crates/dagon-events/src/compose.rs`の`Clock`をマイコンのcrateが実装します。
4. `crates/dagon-agent/src/lib.rs`の`SlotStore`を実装します（例: `targets/mcu/stm32f401re/src/store.rs`）。
5. DAGONのブートを使用するマイコンは`stage0/`と`bootcore/`を用意します（例: `targets/mcu/stm32f401re/`）。
6. 全マイコンで`template/`を用意します。`dagon gen`はこのフォルダからノードのcrateを作成します。
7. `compat.toml`に部品を追加します（[バージョンとリリース](/explanation/versioning/)）。
8. `dagon sim`で確認し、`dagon conform`をPC接続ノード経由で実行します。

`plugin/skills/dagon-port/SKILL.md`は同一の手順をClaude Codeで実行します。

## リンクの追加

リンクの種類は固定の一覧で、コード内の1か所にあります。そのため`crates/`も変更します。

1. `crates/dagon-format/src/link.rs`の`with_link_kinds!`に1行を追加します。構成で使用する名称とイベントのコードは、この行が起点です。
2. そのリンクに対応するマイコンで、`dagon_port::Link`（コーディネータから接続先のノードへのバイト列の場合は`StreamPort`）を実装します。例は`targets/mcu/esp32s3/src/links.rs`です。
3. マイコンの`LINK_KINDS`（`targets/mcu/rp2040/src/kinds.rs`）と、`dagon-mcu.toml`の`[links]`に追加します。
4. DAGON Consoleでの表示用の文言を、`web/src/app/copy.ts`の`LINK_KIND_WORDS`に追加します（`label`はキャンバスと表、`choice`はインスペクタの一覧）。追加するまでDAGON Consoleのテストは失敗します。
5. PC側で新規の媒体を使用する場合は、`crates/dagon-host/src/lib.rs`の`Transport`を実装します（例: `crates/dagon-cli/src/serial.rs`）。

DAGONのソースを変更せずに転送とプロトコルを置換する場合は、拡張で`UpdateTransport`と`UpdateProtocol`のポートを提供します（[ライフサイクルイベントと拡張](/explanation/lifecycle-events/)の8.4）。
