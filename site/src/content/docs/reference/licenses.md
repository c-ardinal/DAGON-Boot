---
title: "ライセンス"
description: "DAGON のライセンス、依存ライブラリのライセンス、改変せずに使うコンポーネントです。"
---

このリポジトリで DAGON Boot のために書いたものは、[MIT ライセンス](https://github.com/c-ardinal/DAGON-Boot/blob/main/LICENSE-MIT) と [Apache License, Version 2.0](https://github.com/c-ardinal/DAGON-Boot/blob/main/LICENSE-APACHE) のどちらかを選んで利用できます。


## 依存ライブラリとそのライセンス


リポジトリ内のすべての `Cargo.lock` と `web/package-lock.json` から `python scripts/licences.py` で生成しています（表は英語のままです）。

注意点:

- `serialport`（MPL-2.0）は `dagon` CLI が改変せずに使っています。`dagon` の実行ファイルを配布する場合、MPL-2.0 により `serialport` のソースを入手できるようにする必要があります。公開されている crate <https://crates.io/crates/serialport>（リポジトリ <https://github.com/serialport/serialport-rs>）がそのソースです。
- `unescaper` は MIT OR GPL-3.0-only で提供されており、MIT で使っています。
- `ring` は Apache-2.0 AND ISC（両方が適用されます）。
- `webpki-roots` は CDLA-Permissive-2.0 です（`dagon ai topology` の TLS クライアントが使う外部データ、Mozilla のルート証明書のデータを含みます）。
- `subtle`、`rp-pac`、`rp2040-boot2`、`source-map-js` は BSD-3-Clause です。
- `lightningcss`（MPL-2.0）は web のビルドツールのビルド時の依存だけで、DAGON Console には何も含まれません。
- DAGON Console に含まれる npm のコードは、Svelte のクライアント用ランタイムと、それが読み込む `clsx` と `esm-env`（いずれも MIT）だけです。他の npm のライブラリはビルドとテストの道具です（表の "Ships in" 列）。
- `web/public/fonts/` の書体は SIL Open Font License 1.1 です: Doto、DotGothic16、Zen Kaku Gothic New、JetBrains Mono（ライセンス文はファイルの隣にあります。`web/README.md` を参照）。
- 親機の表示の日本語の状態語は、Noto Sans JP（SIL Open Font License 1.1）から作ったビットマップです（ライセンス文は `targets/boards/waveshare-rp2040-touch-lcd-1.28/fonts/`）。

Rust crates: 316 packages, 350 name/version pairs. "Used by": `crates/` is the host workspace, `samples/...` the sample apps, other names are firmware packages under `targets/`.

| Crate | Versions | Licence | Used by |
|---|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| aligned | 0.4.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| allocator-api2 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| ascii-canvas | 4.0.0 | Apache-2.0/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| az | 1.2.1, 1.3.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bare-metal | 0.2.5 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| base64 | 0.23.1 | MIT OR Apache-2.0 | crates/ |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bit_field | 0.10.3 | Apache-2.0/MIT | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| bitfield | 0.13.2, 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| bitflags | 1.3.2, 2.13.2 | MIT OR Apache-2.0 / MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| block-buffer | 0.10.4, 0.12.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| block-device-driver | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates/ |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| byteorder | 1.5.0 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| bytes | 1.12.1 | MIT | crates/ |
| cc | 1.10.0, 1.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| codespan-reporting | 0.11.1 | Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| const-default | 1.0.0 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | crates/ |
| cordyceps | 0.3.5 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | crates/ |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | crates/ |
| cortex-m | 0.7.10 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| cortex-m-macros | 0.7.8 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| cortex-m-rt | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| cortex-m-rt-macros | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| cpufeatures | 0.2.17, 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| crc-any | 2.5.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| crunchy | 0.2.4 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| crypto-common | 0.1.7, 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| darling | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| darling_core | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| darling_macro | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| debug-helper | 0.3.14 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| defmt | 0.3.101, 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-rtt | 1.3.1 | MIT OR Apache-2.0 | f401-bringup, samples/f401-app |
| delegate | 0.13.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| digest | 0.10.7, 0.11.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| document-features | 0.2.12 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ed25519-compact | 2.6.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| either | 1.19.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-hal-internal | 0.4.0, 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| embassy-net-driver-channel | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-rp | 0.10.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-stm32 | 0.6.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| embassy-sync | 0.6.2, 0.7.2, 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, f401-bringup, mcu/esp32s3, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-usb | 0.6.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-usb-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-usb-synopsys-otg | 0.3.3, 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| embedded-hal | 0.2.7, 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-hal-nb | 1.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| embedded-io | 0.6.1, 0.7.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-io-async | 0.6.1, 0.7.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ena | 0.14.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| encode_unicode | 0.3.6 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| enumset | 1.1.15 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| enumset_derive | 0.15.1 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| esp-alloc | 0.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-bootloader-esp-idf | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-config | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal | 1.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal-co5300 | 0.1.0 | MIT OR Apache-2.0 | samples/esp32s3-app |
| esp-hal-procmacros | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-metadata-generated | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-riscv-rt | 0.15.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-rom-sys | 0.1.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-rtos | 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-storage | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-sync | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32 | 0.41.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c2 | 0.30.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c3 | 0.33.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c5 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c6 | 0.24.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c61 | 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32h2 | 0.20.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32p4 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s2 | 0.32.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s3 | 0.36.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s31 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| fixed | 1.29.0, 1.31.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| float-cmp | 0.9.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| fnv | 1.0.7 | Apache-2.0 / MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| fugit | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| gcd | 2.3.0 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| generator | 0.8.11 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| generic-array | 0.14.7 | MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| getrandom | 0.2.17, 0.4.3 | MIT OR Apache-2.0 | crates/ |
| half | 2.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| heapless | 0.8.0, 0.9.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| heck | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| http | 1.5.0 | MIT OR Apache-2.0 | crates/ |
| httparse | 1.10.1 | MIT OR Apache-2.0 | crates/ |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| ident_case | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| indoc | 2.0.8 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| instability | 0.3.14 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| io-kit-sys | 0.4.1 | MIT / Apache-2.0 | crates/ |
| itertools | 0.14.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| itoa | 1.0.18 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| jiff | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-core | 0.1.1 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-static | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| js-sys | 0.3.107 | MIT OR Apache-2.0 | crates/ |
| keccak | 0.1.6 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lalrpop | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lalrpop-util | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| libc | 0.2.191 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| libyaml-rs | 0.3.0 | MIT | crates/ |
| linked_list_allocator | 0.10.6 | Apache-2.0/MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| litrs | 1.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| log | 0.4.35 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| loom | 0.7.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | crates/ |
| matchers | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| memchr | 2.8.3 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| micromath | 2.1.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| nb | 0.1.3, 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| new_debug_unreachable | 1.0.6 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| nix | 0.26.4 | MIT | crates/ |
| nu-ansi-term | 0.50.3 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| object | 0.37.3 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| panic-halt | 1.0.0 | MIT OR Apache-2.0 | mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump |
| panic-probe | 1.0.0 | MIT OR Apache-2.0 | f401-bringup, samples/f401-app |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| paste | 1.0.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | crates/ |
| petgraph | 0.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| phf_shared | 0.11.3 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pico-args | 0.5.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| pio | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-core | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-parser | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-proc | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| precomputed-hash | 0.1.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| proc-macro-error-attr2 | 2.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro-error2 | 2.0.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| quote | 1.0.47 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | crates/ |
| rand_core | 0.10.1, 0.6.4, 0.9.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| redox_syscall | 0.5.18 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| regex | 1.13.1 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| rgb | 0.8.53 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| ring | 0.17.14 | Apache-2.0 AND ISC | crates/ |
| riscv | 0.15.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-pac | 0.2.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt | 0.16.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-target-parser | 0.1.3 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rlsf | 0.2.3 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rp-pac | 7.0.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| rp2040-boot2 | 0.3.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| rustc_version | 0.2.3 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | crates/ |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | crates/ |
| rustls-webpki | 0.103.16 | ISC | crates/ |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| same-file | 1.0.6 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sdio | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| sdio-host | 0.9.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| semver | 0.9.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| semver-parser | 0.7.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| serde | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | crates/ |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates/ |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| serialport | 4.10.1 | MPL-2.0 | crates/ |
| sha1 | 0.11.0 | MIT OR Apache-2.0 | crates/ |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| sha2-const-stable | 0.1.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sha3 | 0.10.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sharded-slab | 0.1.7 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| shlex | 2.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| slab | 0.4.12 | MIT | crates/ |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| smart-leds | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| smart-leds-trait | 0.3.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| somni-expr | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-parser | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-template | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| ssmarshal | 1.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| static_cell | 2.1.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| stm32-fmc | 0.4.0 | MIT/Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| stm32-metapac | 21.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| strsim | 0.11.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| strum | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| strum_macros | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| subtle | 2.6.1 | BSD-3-Clause | crates/ |
| syn | 1.0.109, 2.0.119, 3.0.9 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| term | 1.2.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| termcolor | 1.4.1 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tokio | 1.54.1 | MIT | crates/ |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates/ |
| toml_datetime | 0.6.11, 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_edit | 0.22.27, 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates/ |
| tracing | 0.1.44 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-attributes | 0.1.31 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-core | 0.1.36 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-log | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-subscriber | 0.3.23 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| trait-set | 0.3.0 | MIT | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| typenum | 1.20.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| unescaper | 0.1.10 | MIT OR GPL-3.0-only | crates/ |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| unicode-width | 0.1.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| unsafe-libyaml | 0.2.11 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| untrusted | 0.9.0 | ISC | crates/ |
| ureq | 3.5.0 | MIT OR Apache-2.0 | crates/ |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | crates/ |
| usb-device | 0.3.2 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid-descriptors | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid-macros | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | crates/ |
| valuable | 0.1.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| vcell | 0.1.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| version_check | 0.9.5 | MIT/Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| void | 1.0.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| volatile-register | 0.2.2 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| walkdir | 2.5.0 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-futures | 0.4.80 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | crates/ |
| winapi-util | 0.1.11 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-sys | 0.52.0, 0.61.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| winnow | 0.7.14, 1.0.4 | MIT | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 | crates/ |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | crates/ |
| zmij | 1.0.23 | MIT | crates/ |

npm packages (`web/package-lock.json`): 86. Only those marked "page" ship in the built page (the Svelte client runtime and what it imports); the rest are build and test tools.

| Package | Version | Licence | Ships in |
|---|---|---|---|
| @jridgewell/gen-mapping | 0.3.13 | MIT | build/test |
| @jridgewell/remapping | 2.3.5 | MIT | build/test |
| @jridgewell/resolve-uri | 3.1.2 | MIT | build/test |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT | build/test |
| @jridgewell/trace-mapping | 0.3.31 | MIT | build/test |
| @oxc-project/types | 0.156.0 | MIT | build/test |
| @rolldown/binding-android-arm-eabi | 1.2.16 | MIT | build/test |
| @rolldown/binding-android-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-darwin-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-darwin-x64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-freebsd-x64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm-gnueabihf | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm64-musl | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-ppc64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-s390x-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-x64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-x64-musl | 1.2.16 | MIT | build/test |
| @rolldown/binding-openharmony-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-win32-arm64-msvc | 1.2.16 | MIT | build/test |
| @rolldown/binding-win32-x64-msvc | 1.2.16 | MIT | build/test |
| @rolldown/pluginutils | 1.0.1 | MIT | build/test |
| @sveltejs/acorn-typescript | 1.0.13 | MIT | build/test |
| @sveltejs/load-config | 0.2.3 | MIT | build/test |
| @sveltejs/vite-plugin-svelte | 7.3.1 | MIT | build/test |
| @types/chai | 5.2.3 | MIT | build/test |
| @types/deep-eql | 4.0.2 | MIT | build/test |
| @types/estree | 1.0.10 | MIT | build/test |
| @vitest/mocker | 5.0.5 | MIT | build/test |
| @vitest/spy | 5.0.5 | MIT | build/test |
| acorn | 8.19.0 | MIT | build/test |
| aria-query | 5.3.1 | Apache-2.0 | build/test |
| assertion-error | 2.0.1 | MIT | build/test |
| axobject-query | 4.1.0 | Apache-2.0 | build/test |
| chai | 6.3.0 | MIT | build/test |
| chokidar | 4.0.3 | MIT | build/test |
| clsx | 2.1.1 | MIT | page |
| deepmerge | 4.3.1 | MIT | build/test |
| detect-libc | 2.1.2 | Apache-2.0 | build/test |
| devalue | 5.9.4 | MIT | build/test |
| es-module-lexer | 2.3.2 | MIT | build/test |
| esm-env | 1.2.2 | MIT | page |
| esrap | 2.4.0 | MIT | build/test |
| estree-walker | 3.0.3 | MIT | build/test |
| expect-type | 1.4.0 | Apache-2.0 | build/test |
| fdir | 6.5.0 | MIT | build/test |
| fsevents | 2.3.3 | MIT | build/test |
| is-reference | 3.0.3 | MIT | build/test |
| lightningcss | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-android-arm64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-darwin-arm64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-darwin-x64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-freebsd-x64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm-gnueabihf | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm64-gnu | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm64-musl | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-x64-gnu | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-x64-musl | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-win32-arm64-msvc | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-win32-x64-msvc | 1.33.0 | MPL-2.0 | build/test |
| locate-character | 3.0.0 | MIT | build/test |
| magic-string | 0.30.21 | MIT | build/test |
| magic-string | 1.4.5 | MIT | build/test |
| mri | 1.2.0 | MIT | build/test |
| nanoid | 3.3.20 | MIT | build/test |
| obug | 2.2.1 | MIT | build/test |
| obug | 3.0.0 | MIT | build/test |
| picocolors | 1.1.1 | ISC | build/test |
| picomatch | 4.0.7 | MIT | build/test |
| postcss | 8.5.32 | MIT | build/test |
| readdirp | 4.1.2 | MIT | build/test |
| rolldown | 1.2.16 | MIT | build/test |
| sade | 1.8.1 | MIT | build/test |
| source-map-js | 1.2.2 | BSD-3-Clause | build/test |
| std-env | 4.3.0 | MIT | build/test |
| svelte | 5.57.3 | MIT | page |
| svelte-check | 4.7.8 | MIT | build/test |
| tinybench | 6.2.3 | MIT | build/test |
| tinyexec | 1.3.1 | MIT | build/test |
| tinyglobby | 0.2.17 | MIT | build/test |
| typescript | 6.0.3 | Apache-2.0 | build/test |
| vite | 8.3.5 | MIT | build/test |
| vitefu | 1.1.3 | MIT | build/test |
| vitest | 5.0.5 | MIT | build/test |
| why-is-node-running | 3.2.1 | MIT | build/test |
| zimmerframe | 1.1.5 | MIT | build/test |


## 自作コードの範囲


- DAGON Boot のために書いたもの: `crates/`、`targets/`（下の無改変のベンダーコンポーネントを除く）、`web/src/`、`site/`、`samples/`、`web/scripts/`、`plugin/`、`topology/`、`scripts/` のすべて。
- Cargo と npm を通して改変せずに使う依存: 上の表の crate とパッケージ（例: embassy、esp-hal、ed25519-compact、sha2、serde、Svelte、Vite）。
- 無改変のベンダーコンポーネント: 下を参照。


## 使用していないコンポーネント (公開されているブートローダ実装は使用していません)


公開されているブートローダ実装（例: embassy-boot、MCUboot、wolfBoot）のコードもロジックも使っていません。起動の判断、イメージとリリースの形式、状態ログ、書き込みのプロトコル、受け手はこのプロジェクトのために書いたものです。


## 無改変のベンダーコンポーネント


- ESP32-S3 の 2nd stage ブートローダ: 公式の ESP-IDF v6.1 のブートローダを、アプリのロールバックを有効にして公式のソースからビルドしたもの（`targets/esp32s3-2ndstage` で自作したのはプロジェクトのファイルと `vendor/sdkconfig.defaults` の sdkconfig オプションだけ）。無改変のまま `targets/mcu/esp32s3/vendor/bootloader.bin` として、出どころとライセンスを添えて同梱している（`vendor/PROVENANCE.md`、`vendor/LICENSE-ESP-IDF`）。
- `esp-bootloader-esp-idf`（crate、MIT OR Apache-2.0）: ESP32-S3 のポートが otadata の読み書きに改変せずに使っています。この 2nd stage でのスロットの選択は otadata で行います。
- RP2040 の boot2: `rp2040-boot2` crate（BSD-3-Clause）の generic 03h の second stage を、`targets/mcu/rp2040/stage0` がフラッシュの先頭に改変せずに置いています。
- STM32F401 の ROM にあるシステムメモリのブートローダ（文書化された I2C の手順で復旧に使います）。
