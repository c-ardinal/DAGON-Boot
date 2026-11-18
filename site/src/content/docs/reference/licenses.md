---
title: "ライセンス"
description: "DAGONのライセンス、依存ライブラリのライセンス、改変せずに使うコンポーネントです。"
---

このリポジトリでDAGON Bootのために書いたものは、[MITライセンス](https://github.com/c-ardinal/DAGON-Boot/blob/main/LICENSE-MIT)と[Apache License, Version 2.0](https://github.com/c-ardinal/DAGON-Boot/blob/main/LICENSE-APACHE)のどちらかを選んで利用できます。


## 依存ライブラリとそのライセンス


リポジトリ内のすべての`Cargo.lock`と`web/package-lock.json`から`python scripts/licences.py`で生成しています（表は英語のままです）。

注意点:

- `serialport`（MPL-2.0）は`dagon` CLIが改変せずに使っています。`dagon`の実行ファイルを配布する場合、MPL-2.0により`serialport`のソースを入手できるようにする必要があります。公開されているcrate<https://crates.io/crates/serialport>（リポジトリ<https://github.com/serialport/serialport-rs>）がそのソースです。
- `unescaper`はMIT OR GPL-3.0-onlyで提供されており、MITで使っています。
- `ring`はApache-2.0 AND ISC（両方が適用されます）。
- `webpki-roots`はCDLA-Permissive-2.0です（`dagon ai topology`のTLSクライアントが使う外部データ、Mozillaのルート証明書のデータを含みます）。
- `subtle`、`rp-pac`、`rp2040-boot2`、`source-map-js`、`curve25519-dalek`、`ed25519-dalek`はBSD-3-Clauseです。
- `ed25519-dalek`、`curve25519-dalek`とその依存（`fiat-crypto`、`curve25519-dalek-derive`、`ed25519`、`signature`ほか）は、`dagon-ed25519`のテストだけが使う開発時の依存で、DAGONのプログラムには含まれません。
- `lightningcss`（MPL-2.0）はwebのビルドツールのビルド時の依存だけで、DAGON Consoleには何も含まれません。
- DAGON Consoleに含まれるnpmのコードは、Svelteのクライアント用ランタイムと、それが読み込む`clsx`と`esm-env`（いずれもMIT）だけです。他のnpmのライブラリはビルドとテストの道具です（表の"Ships in"列）。
- `web/public/fonts/`の書体はSIL Open Font License 1.1です: Doto、DotGothic16、Zen Kaku Gothic New、JetBrains Mono（ライセンス文はファイルの隣にあります。`web/README.md`を参照）。
- サンプル`samples/rp2040-round-lcd`の画面の日本語の状態語は、Noto Sans JP（SIL Open Font License 1.1）から作成したビットマップです（ライセンス文は`samples/rp2040-round-lcd/screen/fonts/`）。

Rust crates: 408 packages, 449 name/version pairs. "Used by": `crates/` is the host workspace, `samples/...` the sample apps, other names are firmware packages under `targets/`.

| Crate | Versions | Licence | Used by |
|---|---|---|---|
| aes | 0.9.4 | MIT OR Apache-2.0 | crates/ |
| aho-corasick | 1.1.5 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| aligned | 0.4.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| allocator-api2 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| anstream | 1.0.0 | MIT OR Apache-2.0 | crates/ |
| anstyle | 1.0.14 | MIT OR Apache-2.0 | crates/ |
| anstyle-parse | 1.0.0 | MIT OR Apache-2.0 | crates/ |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 | crates/ |
| anstyle-wincon | 3.0.11 | MIT OR Apache-2.0 | crates/ |
| apple-native-keyring-store | 1.0.2 | MIT OR Apache-2.0 | crates/ |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| ascii-canvas | 4.0.0 | Apache-2.0/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 | crates/ |
| async-channel | 2.5.0 | Apache-2.0 OR MIT | crates/ |
| async-executor | 1.14.1 | Apache-2.0 OR MIT | crates/ |
| async-io | 2.6.0 | Apache-2.0 OR MIT | crates/ |
| async-lock | 3.5.0 | Apache-2.0 OR MIT | crates/ |
| async-process | 2.5.0 | Apache-2.0 OR MIT | crates/ |
| async-recursion | 1.2.0 | MIT OR Apache-2.0 | crates/ |
| async-signal | 0.2.14 | Apache-2.0 OR MIT | crates/ |
| async-task | 4.7.1 | Apache-2.0 OR MIT | crates/ |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | crates/ |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | crates/ |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| az | 1.2.1, 1.3.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| bare-metal | 0.2.5 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| base64 | 0.23.1 | MIT OR Apache-2.0 | crates/ |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| bit_field | 0.10.3 | Apache-2.0/MIT | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| bitfield | 0.13.2, 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, samples/esp32s3-app, samples/rp2040-round-lcd/app |
| bitflags | 1.3.2, 2.13.2 | MIT OR Apache-2.0 / MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| block-buffer | 0.10.4, 0.12.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| block-device-driver | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| block-padding | 0.4.2 | MIT OR Apache-2.0 | crates/ |
| blocking | 1.7.0 | Apache-2.0 OR MIT | crates/ |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates/ |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, samples/esp32s3-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| byteorder | 1.5.0 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| bytes | 1.12.1 | MIT | crates/ |
| cbc | 0.2.1 | MIT OR Apache-2.0 | crates/ |
| cc | 1.10.0, 1.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| cipher | 0.5.2 | MIT OR Apache-2.0 | crates/ |
| clap | 4.6.7 | MIT OR Apache-2.0 | crates/ |
| clap_builder | 4.6.7 | MIT OR Apache-2.0 | crates/ |
| clap_derive | 4.6.7 | MIT OR Apache-2.0 | crates/ |
| clap_lex | 1.1.1 | MIT OR Apache-2.0 | crates/ |
| cmov | 0.5.4 | Apache-2.0 OR MIT | crates/ |
| codespan-reporting | 0.11.1 | Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 | crates/ |
| command-group | 5.0.1 | Apache-2.0 OR MIT | crates/ |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | crates/ |
| const-default | 1.0.0 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | crates/ |
| cordyceps | 0.3.5 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | crates/ |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | crates/ |
| cortex-m | 0.7.10 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| cortex-m-macros | 0.7.8 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| cortex-m-rt | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| cortex-m-rt-macros | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| cpubits | 0.1.1 | MIT OR Apache-2.0 | crates/ |
| cpufeatures | 0.2.17, 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| crc-any | 2.5.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | crates/ |
| crunchy | 0.2.4 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| crypto-common | 0.1.7, 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| ctutils | 0.4.8 | Apache-2.0 OR MIT | crates/ |
| curve25519-dalek | 5.0.0 | BSD-3-Clause | crates/ |
| curve25519-dalek-derive | 0.1.1 | MIT/Apache-2.0 | crates/ |
| darling | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| darling_core | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| darling_macro | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| debug-helper | 0.3.14 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| defmt | 0.3.101, 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-rtt | 1.3.1 | MIT OR Apache-2.0 | samples/f401-app |
| delegate | 0.13.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| digest | 0.10.7, 0.11.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| document-features | 0.2.12 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| ed25519 | 3.0.0 | Apache-2.0 OR MIT | crates/ |
| ed25519-compact | 2.6.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| ed25519-dalek | 3.0.0 | BSD-3-Clause | crates/ |
| either | 1.19.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| embassy-hal-internal | 0.4.0, 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app, samples/rp2040-round-lcd/app |
| embassy-net-driver-channel | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| embassy-rp | 0.10.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-stm32 | 0.6.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| embassy-sync | 0.6.2, 0.7.2, 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| embassy-usb | 0.6.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| embassy-usb-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embassy-usb-synopsys-otg | 0.3.3, 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 | samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 | samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embedded-hal | 0.2.7, 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embedded-hal-nb | 1.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embedded-io | 0.6.1, 0.7.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| embedded-io-async | 0.6.1, 0.7.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| ena | 0.14.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| encode_unicode | 0.3.6 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| endi | 1.1.1 | MIT | crates/ |
| enumflags2 | 0.7.12 | MIT OR Apache-2.0 | crates/ |
| enumflags2_derive | 0.7.12 | MIT OR Apache-2.0 | crates/ |
| enumset | 1.1.15 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| enumset_derive | 0.15.1 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| errno | 0.3.14 | MIT OR Apache-2.0 | crates/ |
| esp-alloc | 0.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-bootloader-esp-idf | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-config | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal | 1.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
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
| event-listener | 5.4.2 | Apache-2.0 OR MIT | crates/ |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT | crates/ |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | crates/ |
| fiat-crypto | 0.3.0 | MIT OR Apache-2.0 OR BSD-1-Clause | crates/ |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| fixed | 1.29.0, 1.31.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| float-cmp | 0.9.0 | MIT | samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| fnv | 1.0.7 | Apache-2.0 / MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| fugit | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | crates/ |
| futures-lite | 2.6.1 | Apache-2.0 OR MIT | crates/ |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 | crates/ |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| gcd | 2.3.0 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| generator | 0.8.11 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| generic-array | 0.14.7 | MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, samples/esp32s3-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| getrandom | 0.2.17, 0.4.3 | MIT OR Apache-2.0 | crates/ |
| half | 2.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| heapless | 0.8.0, 0.9.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| heck | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| hermit-abi | 0.5.3 | MIT OR Apache-2.0 | crates/ |
| hex | 0.4.3 | MIT OR Apache-2.0 | crates/ |
| hkdf | 0.13.0 | MIT OR Apache-2.0 | crates/ |
| hmac | 0.13.0 | MIT OR Apache-2.0 | crates/ |
| http | 1.5.0 | MIT OR Apache-2.0 | crates/ |
| httparse | 1.10.1 | MIT OR Apache-2.0 | crates/ |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| ident_case | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| indoc | 2.0.8 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| inout | 0.2.5 | MIT OR Apache-2.0 | crates/ |
| instability | 0.3.14 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| io-kit-sys | 0.4.1 | MIT / Apache-2.0 | crates/ |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 | crates/ |
| itertools | 0.14.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| itoa | 1.0.18, 1.0.19 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| jiff | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-core | 0.1.1 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-static | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| js-sys | 0.3.107 | MIT OR Apache-2.0 | crates/ |
| keccak | 0.1.6 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| keyring-core | 1.0.1 | MIT OR Apache-2.0 | crates/ |
| lalrpop | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| lalrpop-util | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| libc | 0.2.191 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| libyaml-rs | 0.3.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| linked_list_allocator | 0.10.6 | Apache-2.0/MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| litrs | 1.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| log | 0.4.35 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| loom | 0.7.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | crates/ |
| matchers | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| memchr | 2.8.3 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| memoffset | 0.9.1 | MIT | crates/ |
| micromath | 2.1.0 | Apache-2.0 OR MIT | samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| nb | 0.1.3, 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| new_debug_unreachable | 1.0.6 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| nix | 0.26.4, 0.27.1 | MIT | crates/ |
| nu-ansi-term | 0.50.3 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| num | 0.4.3 | MIT OR Apache-2.0 | crates/ |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 | crates/ |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | crates/ |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | crates/ |
| num-iter | 0.1.46 | MIT OR Apache-2.0 | crates/ |
| num-rational | 0.4.2 | MIT OR Apache-2.0 | crates/ |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| object | 0.37.3 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| once_cell_polyfill | 1.70.2 | MIT OR Apache-2.0 | crates/ |
| ordered-stream | 0.2.0 | MIT OR Apache-2.0 | crates/ |
| panic-halt | 1.0.0 | MIT OR Apache-2.0 | mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/stage0 |
| panic-probe | 1.0.0 | MIT OR Apache-2.0 | samples/f401-app |
| parking | 2.2.1 | Apache-2.0 OR MIT | crates/ |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| paste | 1.0.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, samples/esp32s3-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | crates/ |
| petgraph | 0.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| phf_shared | 0.11.3 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| pico-args | 0.5.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| pio | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| pio-core | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| pio-parser | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| pio-proc | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| piper | 0.2.5 | MIT OR Apache-2.0 | crates/ |
| polling | 3.11.0 | Apache-2.0 OR MIT | crates/ |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| precomputed-hash | 0.1.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| proc-macro-error-attr2 | 2.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| proc-macro-error2 | 2.0.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| quote | 1.0.47 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | crates/ |
| rand_core | 0.10.1, 0.6.4, 0.9.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| redox_syscall | 0.5.18 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| regex | 1.13.1 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| rgb | 0.8.53 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| ring | 0.17.14 | Apache-2.0 AND ISC | crates/ |
| riscv | 0.15.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-pac | 0.2.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt | 0.16.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-target-parser | 0.1.3 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rlsf | 0.2.3 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rp-pac | 7.0.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| rp2040-boot2 | 0.3.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| rpassword | 7.5.4 | Apache-2.0 | crates/ |
| rtoolbox | 0.0.6 | Apache-2.0 | crates/ |
| rustc_version | 0.2.3, 0.4.1 | MIT OR Apache-2.0 / MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | crates/ |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | crates/ |
| rustls-webpki | 0.103.16 | ISC | crates/ |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| same-file | 1.0.6 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| sdio | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| sdio-host | 0.9.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| secret-service | 5.2.0 | MIT OR Apache-2.0 | crates/ |
| security-framework | 3.7.0 | MIT OR Apache-2.0 | crates/ |
| security-framework-sys | 2.17.0 | MIT OR Apache-2.0 | crates/ |
| semver | 0.9.0, 1.0.28 | MIT OR Apache-2.0 / MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| semver-parser | 0.7.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| serde | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 | crates/ |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| serialport | 4.10.1 | MPL-2.0 | crates/ |
| sha1 | 0.11.0 | MIT OR Apache-2.0 | crates/ |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| sha2-const-stable | 0.1.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| sha3 | 0.10.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| sharded-slab | 0.1.7 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| shlex | 2.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | crates/ |
| signature | 3.0.0 | Apache-2.0 OR MIT | crates/ |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| slab | 0.4.12 | MIT | crates/ |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| smart-leds | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| smart-leds-trait | 0.3.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| somni-expr | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-parser | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-template | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| ssmarshal | 1.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| static_cell | 2.1.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| stm32-fmc | 0.4.0 | MIT/Apache-2.0 | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| stm32-metapac | 21.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| strsim | 0.11.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| strum | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| strum_macros | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| subtle | 2.6.1 | BSD-3-Clause | crates/ |
| syn | 1.0.109, 2.0.119, 3.0.9 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | crates/ |
| term | 1.2.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| termcolor | 1.4.1 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, samples/esp32s3-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| tokio | 1.54.1 | MIT | crates/ |
| toml | 0.8.23 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| toml_datetime | 0.6.11, 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| toml_edit | 0.22.27, 0.25.17+spec-1.1.0, 0.25.18+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| tracing | 0.1.44 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| tracing-attributes | 0.1.31 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| tracing-core | 0.1.36 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| tracing-log | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| tracing-subscriber | 0.3.23 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| trait-set | 0.3.0 | MIT | boards/nucleo-f401re, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| typenum | 1.20.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, boot-extension-template, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| uds_windows | 1.2.1 | MIT | crates/ |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| unescaper | 0.1.10 | MIT OR GPL-3.0-only | crates/ |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| unicode-width | 0.1.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| unsafe-libyaml | 0.2.11 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| untrusted | 0.9.0 | ISC | crates/ |
| ureq | 3.5.0 | MIT OR Apache-2.0 | crates/ |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | crates/ |
| usb-device | 0.3.2 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| usbd-hid | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| usbd-hid-descriptors | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| usbd-hid-macros | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, samples/rp2040-round-lcd/app |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | crates/ |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT | crates/ |
| uuid | 1.29.0 | Apache-2.0 OR MIT | crates/ |
| valuable | 0.1.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| vcell | 0.1.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| version_check | 0.9.5 | MIT/Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, samples/esp32s3-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| void | 1.0.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| volatile-register | 0.2.2 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| walkdir | 2.5.0 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-futures | 0.4.80 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | crates/ |
| winapi | 0.3.9 | MIT/Apache-2.0 | crates/ |
| winapi-i686-pc-windows-gnu | 0.4.0 | MIT/Apache-2.0 | crates/ |
| winapi-util | 0.1.11 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, samples/esp32s3-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| winapi-x86_64-pc-windows-gnu | 0.4.0 | MIT/Apache-2.0 | crates/ |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| windows-native-keyring-store | 1.1.0 | MIT OR Apache-2.0 | crates/ |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| windows-sys | 0.52.0, 0.61.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| winnow | 0.7.14, 0.7.15, 1.0.4, 1.0.5 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/build, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen, samples/rp2040-round-lcd/uf2check |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| zbus | 5.19.0 | MIT | crates/ |
| zbus-secret-service-keyring-store | 1.0.1 | MIT OR Apache-2.0 | crates/ |
| zbus_macros | 5.19.0 | MIT | crates/ |
| zbus_names | 4.3.5 | MIT | crates/ |
| zcheapstr | 1.2.0 | MIT | crates/ |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, samples/rp2040-round-lcd/app, samples/rp2040-round-lcd/screen |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | crates/ |
| zmij | 1.0.23 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/common, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re/bootcore, mcu/stm32f401re/stage0, samples/esp32s3-app, samples/f401-app, samples/rp2040-round-lcd/app |
| zvariant | 5.15.1 | MIT | crates/ |
| zvariant_derive | 5.15.1 | MIT | crates/ |
| zvariant_utils | 4.2.0 | MIT | crates/ |

npm packages (`web/package-lock.json`): 89. Only those marked "page" ship in the built page (the Svelte client runtime and what it imports); the rest are build and test tools.

| Package | Version | Licence | Ships in |
|---|---|---|---|
| @jridgewell/gen-mapping | 0.3.13 | MIT | build/test |
| @jridgewell/remapping | 2.3.5 | MIT | build/test |
| @jridgewell/resolve-uri | 3.1.2 | MIT | build/test |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT | build/test |
| @jridgewell/trace-mapping | 0.3.31 | MIT | build/test |
| @oxc-project/types | 0.156.0 | MIT | build/test |
| @playwright/test | 1.65.0 | Apache-2.0 | build/test |
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
| playwright | 1.65.0 | Apache-2.0 | build/test |
| playwright-core | 1.65.0 | Apache-2.0 | build/test |
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


- DAGON Bootのために書いたもの: `crates/`（下のテストデータを除く）、`targets/`（下の無改変のベンダーコンポーネントを除く）、`web/src/`、`site/`、`samples/`、`web/scripts/`、`plugin/`、`topology/`、`scripts/`のすべて。
- Cargoとnpmを通して改変せずに使う依存: 上の表のcrateとパッケージ（例: embassy、esp-hal、ed25519-compact、sha2、serde、Svelte、Vite）。
- 無改変のベンダーコンポーネント: 下を参照。


## 使用していないコンポーネント(公開されているブートローダ実装は使用していません)


公開されているブートローダ実装（例: embassy-boot、MCUboot、wolfBoot）のコードもロジックも使っていません。起動の判断、イメージとリリースの形式、状態ログ、書込のプロトコル、受け手はこのプロジェクトのために書いたものです。


## 無改変のベンダーコンポーネント


- ESP32-S3の2nd stageブートローダ: 公式のESP-IDF v6.1のブートローダを、アプリのロールバックを有効にして公式のソースからビルドしたもの（`targets/esp32s3-2ndstage`で自作したのはプロジェクトのファイルと`vendor/sdkconfig.defaults`のsdkconfigオプションだけ）。無改変のまま`targets/mcu/esp32s3/vendor/bootloader.bin`として、出どころとライセンスを添えて同梱している（`vendor/PROVENANCE.md`、`vendor/LICENSE-ESP-IDF`）。
- `esp-bootloader-esp-idf`（crate、MIT OR Apache-2.0）: ESP32-S3のポートがotadataの読み書きに改変せずに使っています。この2nd stageでのスロットの選択はotadataで行います。
- RP2040のboot2: `rp2040-boot2` crate（BSD-3-Clause）のgeneric 03hのsecond stageを、`targets/mcu/rp2040/stage0`がフラッシュの先頭に改変せずに置いています。
- STM32F401のROMにあるシステムメモリのブートローダ（文書化されたI2Cの手順で復旧に使います）。
- Ed25519のテストデータ（`crates/dagon-ed25519/tests/vectors/`、テストだけが読み、プログラムには含まれません）:
  - Project Wycheproofの`testvectors_v1/ed25519_test.json`（<https://github.com/C2SP/wycheproof>のコミット`65ca494727a52944c204441c860b039e6786b99b`）。Apache License 2.0で、無改変のまま同梱し、ライセンス文（`LICENSE-WYCHEPROOF`）と出どころ（`PROVENANCE.md`）を添えています。
  - RFC 8032の7.1節のEd25519のテストベクタ（`rfc8032.txt`、RFCの値を改行だけ除いて写したもの。IETF Trustの著作権表示とTrust Legal Provisionsに従います）。
