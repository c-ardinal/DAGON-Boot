---
title: ボードとマイコン
description: このリリースで定義しているボードとマイコンです。
editUrl: false
---

:::note
`targets/mcu/*/dagon-mcu.toml`, `targets/boards/*/dagon-board.toml`から生成しています。直接編集せず、`python site/scripts/gen.py`で再度生成してください。
:::

このリリースにある`targets/mcu/`のマイコン定義と、`targets/boards/`のボードの定義です。
定義のフォルダを追加すると行が増加します。どちらのファイルの形式も
[マイコンとボードの定義](/reference/definitions/)にあります。

## マイコン

| Id | 名称 | ポート | フラッシュ | RAM | 上りリンク | 下りリンク | コーディネータ可否 | 初回の書込 |
|---|---|---|---|---|---|---|---|---|
| `esp32s3` | ESP32-S3 | `vendor-stage2` | 16 MiB |  | uart, usb-cdc |  |  | `espflash` |
| `rp2040` | RP2040 | `self-boot` | 4 MiB | 264 KiB | usb-cdc | i2c, uart | 可 | `uf2-drive` |
| `stm32f401re` | STM32F401RE | `self-boot` | 512 KiB | 96 KiB | i2c |  |  | `probe-rs` |

## ボード

| Id | 製品 | マイコン | フラッシュ | 既定のリンク |
|---|---|---|---|---|
| `m5stopwatch` | M5StopWatch | ESP32-S3 (`esp32s3`) | 16 MiB | uart |
| `nucleo-f401re` | NUCLEO-F401RE | STM32F401RE (`stm32f401re`) | 512 KiB | i2c |
| `waveshare-rp2040-touch-lcd-1.28` | Waveshare RP2040-Touch-LCD-1.28 | RP2040 (`rp2040`) | 4 MiB | — |

## フラッシュの配置

マイコンの定義の`[layout]`と`[[slot_layout]]`から作成した、領域ごとのアドレスと容量です。
ESP32-S3のアドレスはフラッシュ内のオフセットです。

### ESP32-S3 (`esp32s3`)

| 領域 | アドレス | 容量 | ヘッダ |
|---|---|---|---|
| `agent` factory（`factory`） | `0x0001_0000` | 1 MiB（`0x10_0000`） | `0x0071_2000`に別置 |
| `app` a（`ota_0`） | `0x0011_0000` | 3 MiB（`0x30_0000`） | `0x0071_0000`に別置 |
| `app` b（`ota_1`） | `0x0041_0000` | 3 MiB（`0x30_0000`） | `0x0071_1000`に別置 |

### RP2040 (`rp2040`)

| 領域 | アドレス | 容量 | ヘッダ |
|---|---|---|---|
| stage0 | `0x1000_0000` | 16 KiB（`0x4000`） |  |
| `bootcore_log`の面0 | `0x1000_4000` | 4 KiB（`0x1000`） |  |
| `bootcore_log`の面1 | `0x1000_5000` | 4 KiB（`0x1000`） |  |
| `app_log`の面0 | `0x1000_6000` | 4 KiB（`0x1000`） |  |
| `app_log`の面1 | `0x1000_7000` | 4 KiB（`0x1000`） |  |
| `bootcore` a | `0x1000_8000` | 127.5 KiB（`0x1_FE00`） | 領域の先頭 |
| `bootcore` b | `0x1002_8000` | 127.5 KiB（`0x1_FE00`） | 領域の先頭 |
| `app` a | `0x1004_8000` | 1023.5 KiB（`0xF_FE00`） | 領域の先頭 |
| `app` b | `0x1014_8000` | 1023.5 KiB（`0xF_FE00`） | 領域の先頭 |

### STM32F401RE (`stm32f401re`)

| 領域 | アドレス | 容量 | ヘッダ |
|---|---|---|---|
| stage0 | `0x0800_0000` | 16 KiB（`0x4000`） |  |
| `bootcore_log`の面0、`app_log`の面0 | `0x0800_4000` | 16 KiB（`0x4000`） |  |
| `bootcore_log`の面1、`app_log`の面1 | `0x0800_8000` | 16 KiB（`0x4000`） |  |
| `bootcore` a | `0x0801_0000` | 63.5 KiB（`0xFE00`） | 領域の先頭 |
| `app` a | `0x0802_0000` | 127.5 KiB（`0x1_FE00`） | 領域の先頭 |
| `app` b | `0x0804_0000` | 127.5 KiB（`0x1_FE00`） | 領域の先頭 |
| `bootcore` b | `0x0806_0000` | 127.5 KiB（`0x1_FE00`） | 領域の先頭 |
