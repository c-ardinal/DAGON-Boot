---
title: ボードとマイコン
description: このリリースで定義しているボードとマイコンです。
editUrl: false
---

:::note
`targets/mcu/*/dagon-mcu.toml`, `targets/boards/*/dagon-board.toml` から生成しています。直接編集せず、`python site/scripts/gen.py` でもう一度生成してください。
:::

このリリースの `targets/mcu/` のマイコンのパッケージと、`targets/boards/` のボードのパッケージです。
パッケージのフォルダを足すと行が増えます。どちらのファイルの形式も
[マイコンとボードの定義](/reference/definitions/)にあります。

## マイコン

| Id | 名前 | ポート | フラッシュ | RAM | 上りのつながり | 下りのつながり | 親機にできるか | 初回の書き込み |
|---|---|---|---|---|---|---|---|---|
| `esp32s3` | ESP32-S3 | `vendor-stage2` | 16 MiB |  | uart, usb-cdc |  |  | `espflash` |
| `rp2040` | RP2040 | `self-boot` | 4 MiB | 264 KiB | usb-cdc | i2c, uart | 可 | `uf2-drive` |
| `stm32f401re` | STM32F401RE | `self-boot` | 512 KiB | 96 KiB | i2c |  |  | `probe-rs` |

## ボード

| Id | 製品 | マイコン | フラッシュ | 既定のつながり |
|---|---|---|---|---|
| `m5stopwatch` | M5StopWatch | ESP32-S3 (`esp32s3`) | 16 MiB | uart |
| `nucleo-f401re` | NUCLEO-F401RE | STM32F401RE (`stm32f401re`) | 512 KiB | i2c |
| `waveshare-rp2040-touch-lcd-1.28` | Waveshare RP2040-Touch-LCD-1.28 | RP2040 (`rp2040`) | 4 MiB | — |
