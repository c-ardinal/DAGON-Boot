---
title: Boards and MCUs
description: Every board and MCU this release defines, from targets/.
editUrl: false
---

:::note
Generated from `targets/mcu/*/dagon-mcu.toml`, `targets/boards/*/dagon-board.toml`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

Every MCU package in `targets/mcu/` and board package in `targets/boards/`
of this release. Adding a package folder adds a row; the format of both
files is in [MCU and board definitions](/reference/definitions/).

## MCUs

| Id | Name | Port | Flash | RAM | Up links | Down links | Can be 親機 | First write |
|---|---|---|---|---|---|---|---|---|
| `esp32s3` | ESP32-S3 | `vendor-stage2` | 16 MiB |  | uart, usb-cdc |  |  | `espflash` |
| `rp2040` | RP2040 | `self-boot` | 4 MiB | 264 KiB | usb-cdc | i2c, uart | yes | `uf2-drive` |
| `stm32f401re` | STM32F401RE | `self-boot` | 512 KiB | 96 KiB | i2c |  |  | `probe-rs` |

## Boards

| Id | Product | MCU | Flash | Default link |
|---|---|---|---|---|
| `m5stopwatch` | M5StopWatch | ESP32-S3 (`esp32s3`) | 16 MiB | uart |
| `nucleo-f401re` | NUCLEO-F401RE | STM32F401RE (`stm32f401re`) | 512 KiB | i2c |
| `waveshare-rp2040-touch-lcd-1.28` | Waveshare RP2040-Touch-LCD-1.28 | RP2040 (`rp2040`) | 4 MiB | — |
