---
title: Tools by MCU
description: What dagon doctor checks for each MCU, from the MCU definitions.
editUrl: false
---

:::note
Generated from `targets/mcu/*/dagon-mcu.toml`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

The tools each MCU needs besides Rust, as its MCU definition's `[[doctor]]`
entries name them. `dagon doctor` checks them in a project (every MCU
outside one), and `dagon doctor --fix` runs the install commands that need
no administrator rights.

| MCU | Tool | Install | Note |
|---|---|---|---|
| ESP32-S3 | espup | `cargo install espup --locked` |  |
| ESP32-S3 | esp toolchain | `espup install` | (takes a while) |
| ESP32-S3 | espflash | `cargo install espflash --locked` |  |
| RP2040 | target thumbv6m-none-eabi | `rustup target add thumbv6m-none-eabi` |  |
| STM32F401RE | target thumbv7em-none-eabihf | `rustup target add thumbv7em-none-eabihf` |  |
| STM32F401RE | probe-rs | `cargo install probe-rs-tools --locked` |  |
| STM32F401RE | ST-LINK probe connected |  | plug in the ST-LINK; on Windows install the ST-LINK USB driver from st.com |
