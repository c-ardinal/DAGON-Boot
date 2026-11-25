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
files is in [MCU and board definitions](/en/reference/definitions/).

## MCUs

| Id | Name | Port | Flash | RAM | Up links | Down links | Can be coordinator | First write |
|---|---|---|---|---|---|---|---|---|
| `esp32s3` | ESP32-S3 | `vendor-stage2` | 16 MiB |  | uart, usb-cdc |  |  | `espflash` |
| `rp2040` | RP2040 | `self-boot` | 4 MiB | 256 KiB | usb-cdc | i2c, uart | yes | `uf2-drive` |
| `stm32f401re` | STM32F401RE | `self-boot` | 512 KiB | 96 KiB | i2c |  |  | `probe-rs` |

## Boards

| Id | Product | MCU | Flash | Default link |
|---|---|---|---|---|
| `m5stopwatch` | M5StopWatch | ESP32-S3 (`esp32s3`) | 16 MiB | uart |
| `nucleo-f401re` | NUCLEO-F401RE | STM32F401RE (`stm32f401re`) | 512 KiB | i2c |
| `waveshare-rp2040-touch-lcd-1.28` | Waveshare RP2040-Touch-LCD-1.28 | RP2040 (`rp2040`) | 4 MiB | — |

## Flash layout

Where each region of an MCU is and how much it holds, from the MCU
definition's `[layout]` and `[[slot_layout]]`. ESP32-S3 addresses are
offsets in the flash.

### ESP32-S3 (`esp32s3`)

| Region | Address | Size | Header |
|---|---|---|---|
| `agent` factory (`factory`) | `0x0001_0000` | 1 MiB (`0x10_0000`) | detached at `0x0071_2000` |
| `app` a (`ota_0`) | `0x0011_0000` | 3 MiB (`0x30_0000`) | detached at `0x0071_0000` |
| `app` b (`ota_1`) | `0x0041_0000` | 3 MiB (`0x30_0000`) | detached at `0x0071_1000` |

### RP2040 (`rp2040`)

| Region | Address | Size | Header |
|---|---|---|---|
| stage0 | `0x1000_0000` | 16 KiB (`0x4000`) |  |
| `bootcore_log` face 0 | `0x1000_4000` | 4 KiB (`0x1000`) |  |
| `bootcore_log` face 1 | `0x1000_5000` | 4 KiB (`0x1000`) |  |
| `app_log` face 0 | `0x1000_6000` | 4 KiB (`0x1000`) |  |
| `app_log` face 1 | `0x1000_7000` | 4 KiB (`0x1000`) |  |
| `bootcore` a | `0x1000_8000` | 127.5 KiB (`0x1_FE00`) | at the start |
| `bootcore` b | `0x1002_8000` | 127.5 KiB (`0x1_FE00`) | at the start |
| `app` a | `0x1004_8000` | 1023.5 KiB (`0xF_FE00`) | at the start |
| `app` b | `0x1014_8000` | 1023.5 KiB (`0xF_FE00`) | at the start |

### STM32F401RE (`stm32f401re`)

| Region | Address | Size | Header |
|---|---|---|---|
| stage0 | `0x0800_0000` | 16 KiB (`0x4000`) |  |
| `bootcore_log` face 0, `app_log` face 0 | `0x0800_4000` | 16 KiB (`0x4000`) |  |
| `bootcore_log` face 1, `app_log` face 1 | `0x0800_8000` | 16 KiB (`0x4000`) |  |
| `bootcore` a | `0x0801_0000` | 63.5 KiB (`0xFE00`) | at the start |
| `app` a | `0x0802_0000` | 127.5 KiB (`0x1_FE00`) | at the start |
| `app` b | `0x0804_0000` | 127.5 KiB (`0x1_FE00`) | at the start |
| `bootcore` b | `0x0806_0000` | 127.5 KiB (`0x1_FE00`) | at the start |
