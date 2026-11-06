---
title: マイコンごとのツール
description: dagon doctor がマイコンごとに確かめるものです。
editUrl: false
---

:::note
`targets/mcu/*/dagon-mcu.toml` から生成しています。直接編集せず、`python site/scripts/gen.py` でもう一度生成してください。
「ツール」の列と「補足」の列は元のソースのままで、英語です。
:::

Rust のほかに、マイコンごとに要るツールです。マイコンの定義の `[[doctor]]` にあるとおりです。
`dagon doctor` はプロジェクトの中ではそのマイコンの分を、外ではすべてのマイコンの分を確かめます。
`dagon doctor --fix` は、管理者の権限が要らないものを入れます。

| マイコン | ツール | 入れ方 | 補足 |
|---|---|---|---|
| ESP32-S3 | espup | `cargo install espup --locked` |  |
| ESP32-S3 | esp toolchain | `espup install` | （時間がかかります） |
| ESP32-S3 | espflash | `cargo install espflash --locked` |  |
| RP2040 | target thumbv6m-none-eabi | `rustup target add thumbv6m-none-eabi` |  |
| STM32F401RE | target thumbv7em-none-eabihf | `rustup target add thumbv7em-none-eabihf` |  |
| STM32F401RE | probe-rs | `cargo install probe-rs-tools --locked` |  |
| STM32F401RE | ST-LINK probe connected |  | plug in the ST-LINK; on Windows install the ST-LINK USB driver from st.com |
