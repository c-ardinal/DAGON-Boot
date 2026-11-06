---
title: マイコンごとのツール
description: dagon doctor がマイコンごとに確かめるものです。
editUrl: false
---

:::note
`targets/mcu/*/dagon-mcu.toml` から生成しています。直接編集せず、`python site/scripts/gen.py` でもう一度生成してください。
:::

Rust のほかに、マイコンごとに要るツールです。マイコンの定義の `[[doctor]]` にあるとおりです。
`dagon doctor` はプロジェクトの中ではそのマイコンの分を、外ではすべてのマイコンの分を確かめます。
`dagon doctor --fix` は、管理者の権限が要らないものを入れます。

| マイコン | 種類 | ツール | 入れ方 | 補足 |
|---|---|---|---|---|
| ESP32-S3 | コンパイラ | espup | `cargo install espup --locked` |  |
| ESP32-S3 | コンパイラ | esp ツールチェーン | `espup install` | （時間がかかります） |
| ESP32-S3 | 書き込みツール | espflash | `cargo install espflash --locked` |  |
| RP2040 | Rust ターゲット | ターゲット thumbv6m-none-eabi | `rustup target add thumbv6m-none-eabi` |  |
| STM32F401RE | Rust ターゲット | ターゲット thumbv7em-none-eabihf | `rustup target add thumbv7em-none-eabihf` |  |
| STM32F401RE | 書き込みツール | probe-rs | `cargo install probe-rs-tools --locked` |  |
| STM32F401RE | ドライバ | ST-LINK のプローブがつながっている |  | ST-LINK を差してください。Windows では st.com から ST-LINK の USB ドライバを入れてください |
