---
title: "マイコンとリンクを足す"
description: "DAGONに新しいマイコンやリンクを足すときに、書くものと触らないものです。"
---

層の図は[しくみ](/explanation/)にあります。

## マイコンを足す

`crates/`は変えません。マイコンの知識は`targets/mcu/<id>/`にだけ置きます。

1. `targets/mcu/<id>/`にcrate `dagon-mcu-<id>`を作ります。このcrateは自分の`[workspace]`を持ちます。ルートの`Cargo.toml`は変えません。
2. `dagon-mcu.toml`を書きます。キーは[マイコンとボードの定義](/reference/definitions/)にあります。フォルダを足すと定義が増えます。
3. `crates/dagon-port/src/lib.rs`のトレイトを実装します: `Flash`、`Link`、`Reset`、`Clock`、`Watchdog`、そしてブートの種類で`BootSelectS`か`BootSelectV`。親機にするマイコンは`StreamPort`、`I2cBus`、`SubControl`も実装します。
4. `crates/dagon-agent/src/lib.rs`の`SlotStore`を実装します（`targets/mcu/stm32f401re/src/store.rs`が例です）。
5. DAGONのブートを持つマイコンは`stage0/`と`bootcore/`を用意します（`targets/mcu/stm32f401re/`が例です）。
6. どのマイコンも`template/`を用意します。`dagon gen`はここからノードのcrateを作ります。
7. `compat.toml`に部品を足します（[バージョンとリリース](/explanation/versioning/)）。
8. `dagon sim`で確かめ、`dagon conform`を親機を通して実行します。

`plugin/skills/dagon-port/SKILL.md`は、同じ手順をClaude Codeで進めます。

## リンクを足す

リンクの種類は決まった一覧で、コードの中の1か所にあります。そのため`crates/`も変えます。

1. `crates/dagon-format/src/link.rs`の`with_link_kinds!`に1行足します。構成の名前とイベントのコードはここから来ます。
2. そのリンクを持つマイコンで、`dagon_port::Link`（親機から子機へのバイト列なら`StreamPort`）を実装します。`targets/mcu/esp32s3/src/links.rs`が例です。
3. マイコンの`LINK_KINDS`（`targets/mcu/rp2040/src/kinds.rs`）と、`dagon-mcu.toml`の`[links]`に足します。
4. PC側で新しい媒体を使うときは、`crates/dagon-host/src/lib.rs`の`Transport`を実装します（`crates/dagon-cli/src/serial.rs`が例です）。

DAGONのソースを変えずに転送とプロトコルを置き換えるには、拡張で`UpdateTransport`と`UpdateProtocol`のポートを提供します（[ライフサイクルイベントと拡張](/explanation/lifecycle-events/)の8.4）。
