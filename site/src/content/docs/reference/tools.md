---
title: マイコンごとのツール
description: dagon doctorがマイコンごとに確認する項目です。
editUrl: false
---

:::note
`targets/mcu/*/dagon-mcu.toml`から生成しています。直接編集せず、`python site/scripts/gen.py`で再度生成してください。
:::

Rust以外に、マイコンごとに必要なツールです。マイコン定義にある`[[doctor]]`の記載と同一です。
`dagon doctor`はプロジェクト内ではそのマイコンの分を、プロジェクト外では全マイコンの分を確認します。
`dagon doctor --fix`は、管理者の権限が不要なツールをインストールします。

| マイコン | 種類 | ツール | インストール方法 | 補足 |
|---|---|---|---|---|
| ESP32-S3 | コンパイラ | espup | `cargo install espup --locked` |  |
| ESP32-S3 | コンパイラ | espツールチェーン | `espup install` | （時間がかかります） |
| ESP32-S3 | 書込ツール | espflash | `cargo install espflash --locked` |  |
| RP2040 | Rustターゲット | ターゲットthumbv6m-none-eabi | `rustup target add thumbv6m-none-eabi` |  |
| STM32F401RE | Rustターゲット | ターゲットthumbv7em-none-eabihf | `rustup target add thumbv7em-none-eabihf` |  |
| STM32F401RE | 書込ツール | probe-rs | `cargo install probe-rs-tools --locked` |  |
| STM32F401RE | ドライバ | ST-LINKのプローブを接続済 |  | ST-LINKを接続してください。Windowsではst.comからST-LINKのUSBドライバをインストールしてください |
