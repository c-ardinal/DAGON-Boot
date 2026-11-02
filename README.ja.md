# DAGON Boot (Dual-bank Atomic Gateway for On-device Nodes)

[English: README.md](README.md)

小さなマイコンのネットワークに、署名付きの A/B ファームウェア更新と自動の巻き戻しを、1 台の親機（hub）を通して PC やブラウザから行います。

## 概要

親機（RP2040）は PC と USB でつながり、子機とは I2C（STM32F401）や UART（ESP32-S3）でつながります。PC は署名付きのリリースを検査し、各子機へ親機経由でイメージを送り、子機を新しいスロットで試行起動させ、子機が新しい版で応答したら確定させます。新しいイメージが確定されないままの子機は、ウォッチドッグでリセットされて前のスロットで起動し直します。まったく応答しなくなった STM32F401 は、PC が親機を通して操作する STM32 の ROM の I2C ブートローダで書き直せます。

すべての形式（イメージヘッダ、リリースのマニフェスト、`.dgb` リリースバンドル、状態ログ、構成バイナリ）と通信プロトコルはこのリポジトリで定義しています。ファームウェア、`dagon` コマンドラインツール、ブラウザのページ（WebAssembly 経由）が同じ Rust の crate を使います。

## 現在できること

このリポジトリのコードとテストが示していること:

- 形式とプロトコル: `dagon-format` と `dagon-proto`。`crates/dagon-format/golden/` の黄金ベクタで照合しています。
- 子機側: `dagon-agent`（非稼働スロットへイメージを受け取り、署名と SHA-256 を検査し、切り替えと確定を行う）と `dagon-core`（起動の判断: 回数を限った試行起動と巻き戻し）。
- PC 側: `dagon-host`（状態、自動で見つける、構成の送信、更新、前の版に戻す、確定、復旧）、`dagon-romlink`（STM32 の I2C ROM ブートローダの手順）、`dagon` CLI。これらは `dagon-sim`（F401 子機付きの模擬親機、電断点付きのフラッシュ、損失のあるリンク、ROM ブートローダの模擬）を相手にテストしています。
- `dagon sim run`: シミュレータ上の 4 つの通しシナリオ。転送中の電断を含む更新、確定しないイメージ、改ざんしたリリース（PC が拒否し、PC 側の検査を外すと子機が拒否）、全消去した F401 の ROM ブートローダ経由の復活。
- `dagon conform`: `dagon-conform` の 12 件の適合試験。模擬の F401 に対し、親機経由でも直結でも 12 件すべて合格します。
- 構成 YAML: 21 件の検証規則と対応ボードの表（`dagon-topology`）。`topology/desk-demo.yaml` はこれを通ります。
- ファームウェア: RP2040 親機（`targets/rp2040-stage0`、`rp2040-bootcore`、`rp2040-hub`）、F401 子機（`targets/f401-bootcore`、`f401-app`）、ESP32-S3 子機（`targets/esp32s3-port`、`esp32s3-agent`、ESP-IDF の 2nd stage と組み合わせ）。実機での通しの流れは確認中です（「制限事項」を参照）。
- `web/` のブラウザのページ（開発中。現在できることは `web/README.md` を参照）。

## はじめかた

CLI をビルドします（Rust stable。「開発に必要なもの」を参照）:

```sh
cargo build --release -p dagon-cli      # 実行ファイル: target/release/dagon
```

ハードウェアなしでシミュレータ上で一通り試す:

```sh
dagon sim run
```

鍵（秘密鍵は git の作業ツリーの外に置きます。中にあると CLI が使用を拒否します）:

```sh
dagon key gen --out ~/.dagon/keys
export DAGON_SIGNING_KEY=~/.dagon/keys/dagon-signing.key
export DAGON_PUBKEY=~/.dagon/keys/dagon-signing.pub   # ファームウェアのビルドが埋め込む
```

初期導入。`targets/` のファームウェアをビルドし（各ディレクトリの README にコマンドがあります）、`dagon.toml` の `[provision.<node>]` を ELF ファイルに向けてから:

```sh
dagon rescue hub        # out/provision/hub/rescue.uf2: BOOT を押しながら USB をつなぎ、RPI-RP2 にコピー
dagon provision seg     # out/provision/seg/flash.bin と ST-LINK 用の probe-rs のコマンド
```

更新。各子機の新しいアプリを両スロット分署名し、構成に対してリリースを作り、適用します:

```sh
dagon image sign app.elf --node seg --version 1.1.0 --slot a --out out/rel/seg-a.img
dagon image sign app-b.elf --node seg --version 1.1.0 --slot b --out out/rel/seg-b.img
# ... 親機も同様
dagon release build out/rel --name r1.1 --out out/release-1.1.dgb
dagon status
dagon apply out/release-1.1.dgb
dagon recover seg       # 子機が応答しなくなったとき
```

`dagon apply` は、`dagon.toml` の構成と違う構成で作られたリリースを拒否します。親機は USB ID `c0de:cafe` または製品名 "DAGON hub" で見つけます（`--port` でも指定できます）。

子機の適合試験（`out/conform/<node>` に `good-a.img`、`good-b.img`、`no-confirm-a.img`、`no-confirm-b.img` を置く）:

```sh
dagon conform --node seg            # result.json と report.html を書き出す
```

自然言語のボードの説明から Claude API で構成を作る（キーは環境変数から読み、表示しません）:

```sh
export ANTHROPIC_API_KEY=...
dagon ai topology --in boards.txt --out topology/mine.yaml
```

すべてのコマンドとオプションは `dagon --help` で確認できます。

## ブラウザの UI（開発）

`web/README.md` を参照してください。Node.js、npm、`wasm-pack` 0.15.1 があれば:

```sh
cd web
npm install
npm run dev       # http://127.0.0.1:5173（Chrome または Edge 89 以降、Web Serial）
```

## 自分のファームウェアに DAGON を組み込む

1. crate をリリースのタグに固定した git 依存として加えます。例:
   `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port`、ブートコアを作るなら `dagon-core` も同様です。
2. 既存のボードと同じ形にフラッシュを割り付けます: ブートコア、状態ログ 2 面、スロット A と B。各スロットの先頭に 512 バイトの署名付きイメージヘッダ、本体はスロット先頭 + 0x200。アプリはスロットの本体アドレスにリンクします（`targets/f401-app` と `targets/rp2040-hub` の README に割り付けがあります）。ボードを構成 YAML に加えます。
3. ボード用に `dagon-port` の trait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS` か `BootSelectV`）と `dagon_agent::SlotStore` を実装し、リンクとフックを渡してエージェントを起動します:

   ```rust
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, hooks); // hooks: dagon_agent::Hooks
   agent.run(&mut link, &mut reset).await;                    // 要求に応え続ける
   ```

   確定済みのあいだはウォッチドッグに給餌し、試行中は限られた時間だけ給餌して、確定されないイメージがリセットされ巻き戻るようにします。
4. `dagon conform` で移植を確かめ、初期導入は `dagon image sign` → `dagon release build` → `dagon provision`、以後は `dagon apply` を使います。

## リポジトリの構成

| パス | 内容 |
|---|---|
| `crates/dagon-format` | イメージヘッダ、マニフェスト、リリースバンドル、状態ログ、構成バイナリ、黄金ベクタ |
| `crates/dagon-proto` | 通信プロトコル v1: フレーム、コマンド、ペイロード |
| `crates/dagon-port` | ボードの trait（フラッシュ、リンク、リセット、ウォッチドッグ、起動先の選択、I2C、子機の制御線） |
| `crates/dagon-core` | 起動の判断（試行、回数、巻き戻し） |
| `crates/dagon-agent` | 子機側の更新の受け手 |
| `crates/dagon-conform` | 12 件の適合試験と結果ファイルの形式 |
| `crates/dagon-sim` | シミュレータ: フラッシュ、リンク、ROM ブートローダ、子機、親機 |
| `crates/dagon-topology` | 構成 YAML、検証規則、対応ボード |
| `crates/dagon-sign` | 鍵、イメージの署名、リリースの作成、初期導入用ファイル |
| `crates/dagon-host` | CLI とブラウザが共有する PC 側の手順 |
| `crates/dagon-romlink` | STM32 の I2C ROM ブートローダの手順 |
| `crates/dagon-cli` | `dagon` コマンド |
| `crates/dagon-wasm` | ブラウザのページ用の WebAssembly バインディング |
| `targets/` | ファームウェア: RP2040 親機、STM32F401 子機、ESP32-S3 子機、立ち上げ用ツール |
| `web/` | ブラウザのページ（Svelte、Vite、TypeScript） |
| `topology/` | 構成の例 |
| `plugin/` | 新しいボードへの移植に使う Claude Code のプラグイン（エージェント、スキル、フック） |
| `scripts/licences.py` | 下の依存ライブラリのライセンス表を生成する |

## 開発に必要なもの

- Rust stable と `rust-toolchain.toml` のターゲット（`thumbv6m-none-eabi`、`thumbv7em-none-eabihf`、`wasm32-unknown-unknown`）
- `crates/dagon-wasm` 用の `wasm-pack` 0.15.1（対応する `wasm-bindgen` と `wasm-opt` は自分で取得します）
- `web/` 用の Node.js と npm（`web/README.md` を参照）
- ESP32-S3 のファームウェア用: `espup` で入れる Xtensa 用 Rust ツールチェーン（`targets/esp32s3-*` は `esp` チャネルを使う）と、`targets/esp32s3-2ndstage` の 2nd stage をビルドする ESP-IDF v6.1
- F401 を ST-LINK で書く `probe-rs`、RP2040 の立ち上げ用イメージに使う `elf2uf2-rs`
- C コンパイラ: `ring` crate（`dagon ai topology` の TLS）が C のコードをビルドします

## ライセンス (MIT / Apache-2.0 デュアル)

このリポジトリで DAGON Boot のために書いたものは、[MIT ライセンス](LICENSE-MIT) と [Apache License, Version 2.0](LICENSE-APACHE) のどちらかを選んで利用できます。

## 依存ライブラリとそのライセンス

リポジトリ内のすべての `Cargo.lock` と `web/package-lock.json` から `python scripts/licences.py` で生成しています（表は英語のままです）。

注意点:

- `serialport`（MPL-2.0）は `dagon` CLI が改変せずに使っています。`dagon` の実行ファイルを配布する場合、MPL-2.0 により `serialport` のソースを入手できるようにする必要があります。公開されている crate <https://crates.io/crates/serialport>（リポジトリ <https://github.com/serialport/serialport-rs>）がそのソースです。
- `unescaper` は MIT OR GPL-3.0-only で提供されており、MIT で使っています。
- `ring` は Apache-2.0 AND ISC（両方が適用されます）。
- `webpki-roots` は CDLA-Permissive-2.0 です（`dagon ai topology` の TLS クライアントが使う外部データ、Mozilla のルート証明書のデータを含みます）。
- `subtle`、`rp-pac`、`rp2040-boot2`、`source-map-js` は BSD-3-Clause です。
- `lightningcss`（MPL-2.0）は web のビルドツールのビルド時の依存だけで、ページには何も含まれません。
- `web/public/fonts/` の書体は SIL Open Font License 1.1 です: Doto、DotGothic16、Zen Kaku Gothic New、JetBrains Mono（ライセンス文はファイルの隣にあります。`web/README.md` を参照）。

Rust crates: 307 packages, 340 name/version pairs. "Used by": `crates/` is the host workspace, other names are firmware packages under `targets/`.

| Crate | Versions | Licence | Used by |
|---|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| aligned | 0.4.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| ascii-canvas | 4.0.0 | Apache-2.0/MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| az | 1.2.1, 1.3.0 | MIT/Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bare-metal | 0.2.5 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| base64 | 0.23.1 | MIT OR Apache-2.0 | crates/ |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bit_field | 0.10.3 | Apache-2.0/MIT | f401-app, f401-board, f401-bootcore, f401-bringup |
| bitfield | 0.13.2, 0.19.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| bitflags | 1.3.2, 2.13.2 | MIT OR Apache-2.0 / MIT/Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| block-buffer | 0.10.4, 0.12.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| block-device-driver | 0.2.0 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates/ |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| byteorder | 1.5.0 | Unlicense OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bytes | 1.12.1 | MIT | crates/ |
| cc | 1.10.0 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| codespan-reporting | 0.11.1 | Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | crates/ |
| cordyceps | 0.3.5 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | crates/ |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | crates/ |
| cortex-m | 0.7.10 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cortex-m-macros | 0.7.8 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cortex-m-rt | 0.7.7 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cortex-m-rt-macros | 0.7.7 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cpufeatures | 0.2.17, 0.3.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| crc-any | 2.5.1 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| crunchy | 0.2.4 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| crypto-common | 0.1.7, 0.2.2 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| darling | 0.20.11, 0.21.3, 0.24.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| darling_core | 0.20.11, 0.21.3, 0.24.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| darling_macro | 0.20.11, 0.21.3, 0.24.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| debug-helper | 0.3.14 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| defmt | 0.3.101, 1.2.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| defmt-rtt | 1.3.1 | MIT OR Apache-2.0 | f401-app, f401-bringup |
| delegate | 0.13.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| digest | 0.10.7, 0.11.3 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| document-features | 0.2.12 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ed25519-compact | 2.6.0 | MIT | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-hub, rp2040-stage0 |
| either | 1.19.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-hal-internal | 0.4.0, 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-net-driver-channel | 0.4.0 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| embassy-rp | 0.10.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-stm32 | 0.6.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| embassy-sync | 0.6.2, 0.7.2, 0.8.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-usb | 0.6.0 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| embassy-usb-driver | 0.2.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-usb-synopsys-otg | 0.3.3, 0.4.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 | rp2040-hub |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 | rp2040-hub |
| embedded-hal | 0.2.7, 1.0.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-hal-bus | 0.3.0 | MIT OR Apache-2.0 | rp2040-hub |
| embedded-hal-nb | 1.0.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-io | 0.6.1, 0.7.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-io-async | 0.6.1, 0.7.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ena | 0.14.4 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| encode_unicode | 0.3.6 | MIT/Apache-2.0 | rp2040-bringup, rp2040-hub |
| enumset | 1.1.15 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port |
| enumset_derive | 0.15.1 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | crates/, esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| esp-bootloader-esp-idf | 0.6.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-config | 0.8.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-hal | 1.2.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-hal-procmacros | 0.23.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-metadata-generated | 0.5.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-riscv-rt | 0.15.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-rom-sys | 0.1.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-storage | 0.10.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-sync | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32 | 0.41.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c2 | 0.30.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c3 | 0.33.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c5 | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c6 | 0.24.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c61 | 0.4.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32h2 | 0.20.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32p4 | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32s2 | 0.32.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32s3 | 0.36.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32s31 | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| fixed | 1.29.0, 1.31.0 | MIT/Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| float-cmp | 0.9.0 | MIT | rp2040-hub |
| fnv | 1.0.7 | Apache-2.0 / MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| fugit | 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| gcd | 2.3.0 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port |
| generator | 0.8.11 | MIT/Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| generic-array | 0.14.7 | MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| getrandom | 0.2.17, 0.4.3 | MIT OR Apache-2.0 | crates/ |
| half | 2.7.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| heapless | 0.8.0, 0.9.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| heck | 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| http | 1.5.0 | MIT OR Apache-2.0 | crates/ |
| httparse | 1.10.1 | MIT OR Apache-2.0 | crates/ |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-hub, rp2040-stage0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | crates/, esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| indoc | 2.0.8 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| instability | 0.3.14 | MIT | esp32s3-agent, esp32s3-port |
| io-kit-sys | 0.4.1 | MIT / Apache-2.0 | crates/ |
| itertools | 0.14.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| itoa | 1.0.18 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port |
| jiff | 0.2.38 | Unlicense OR MIT | esp32s3-agent, esp32s3-port |
| jiff-core | 0.1.1 | Unlicense OR MIT | esp32s3-agent, esp32s3-port |
| jiff-static | 0.2.38 | Unlicense OR MIT | esp32s3-agent, esp32s3-port |
| js-sys | 0.3.107 | MIT OR Apache-2.0 | crates/ |
| keccak | 0.1.6 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lalrpop | 0.22.2 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lalrpop-util | 0.22.2 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| libc | 0.2.191 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| libyaml-rs | 0.3.0 | MIT | crates/ |
| litrs | 1.0.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| log | 0.4.35 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| loom | 0.7.2 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | crates/ |
| matchers | 0.2.0 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| memchr | 2.8.3 | Unlicense OR MIT | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| micromath | 2.1.0 | Apache-2.0 OR MIT | rp2040-hub |
| mipidsi | 0.10.0 | MIT | rp2040-hub |
| nb | 0.1.3, 1.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| new_debug_unreachable | 1.0.6 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| nix | 0.26.4 | MIT | crates/ |
| nu-ansi-term | 0.50.3 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| object | 0.37.3 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| panic-halt | 1.0.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| panic-probe | 1.0.0 | MIT OR Apache-2.0 | f401-app, f401-bringup |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| paste | 1.0.15 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | crates/ |
| petgraph | 0.7.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| phf_shared | 0.11.3 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pico-args | 0.5.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bringup, rp2040-hub |
| pio | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pio-core | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pio-parser | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pio-proc | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bringup, rp2040-hub |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port |
| precomputed-hash | 0.1.1 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| proc-macro-error-attr2 | 2.0.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| proc-macro-error2 | 2.0.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| quote | 1.0.47 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | crates/ |
| rand_core | 0.10.1, 0.6.4, 0.9.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| redox_syscall | 0.5.18 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| regex | 1.13.1 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| rgb | 0.8.53 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ring | 0.17.14 | Apache-2.0 AND ISC | crates/ |
| riscv | 0.15.0 | ISC | esp32s3-agent, esp32s3-port |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| riscv-pac | 0.2.0 | ISC | esp32s3-agent, esp32s3-port |
| riscv-rt | 0.16.0 | ISC | esp32s3-agent, esp32s3-port |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| riscv-target-parser | 0.1.3 | ISC | esp32s3-agent, esp32s3-port |
| rp-pac | 7.0.0 | BSD-3-Clause | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| rp2040-boot2 | 0.3.0 | BSD-3-Clause | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| rustc_version | 0.2.3 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | crates/ |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | crates/ |
| rustls-webpki | 0.103.16 | ISC | crates/ |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | crates/, esp32s3-agent, esp32s3-port |
| same-file | 1.0.6 | Unlicense/MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | crates/, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| sdio | 0.5.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| sdio-host | 0.9.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| semver | 0.9.0 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| semver-parser | 0.7.0 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| serde | 1.0.229 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | crates/ |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates/ |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| serialport | 4.10.1 | MPL-2.0 | crates/ |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-hub, rp2040-stage0 |
| sha2-const-stable | 0.1.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| sha3 | 0.10.9 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| sharded-slab | 0.1.7 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| shlex | 2.0.1 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| smart-leds | 0.4.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| smart-leds-trait | 0.3.2 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| somni-expr | 0.3.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| somni-parser | 0.3.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| somni-template | 0.3.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| ssmarshal | 1.0.0 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| static_cell | 2.1.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| stm32-fmc | 0.4.0 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| stm32-metapac | 21.0.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| strsim | 0.11.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| strum | 0.27.2 | MIT | esp32s3-agent, esp32s3-port |
| strum_macros | 0.27.2 | MIT | esp32s3-agent, esp32s3-port |
| subtle | 2.6.1 | BSD-3-Clause | crates/ |
| syn | 1.0.109, 2.0.119, 3.0.9 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| term | 1.2.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| termcolor | 1.4.1 | Unlicense OR MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates/ |
| toml_datetime | 0.6.11, 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port |
| toml_edit | 0.22.27, 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates/ |
| tracing | 0.1.44 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-attributes | 0.1.31 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-core | 0.1.36 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-log | 0.2.0 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-subscriber | 0.3.23 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| trait-set | 0.3.0 | MIT | f401-app, f401-board, f401-bootcore, f401-bringup |
| typenum | 1.20.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| unescaper | 0.1.10 | MIT OR GPL-3.0-only | crates/ |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| unicode-width | 0.1.14 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| unsafe-libyaml | 0.2.11 | MIT | esp32s3-agent, esp32s3-port |
| untrusted | 0.9.0 | ISC | crates/ |
| ureq | 3.5.0 | MIT OR Apache-2.0 | crates/ |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | crates/ |
| usb-device | 0.3.2 | MIT | rp2040-bringup, rp2040-hub |
| usbd-hid | 0.10.2 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| usbd-hid-descriptors | 0.10.2 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| usbd-hid-macros | 0.10.2 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | crates/ |
| valuable | 0.1.1 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| vcell | 0.1.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| version_check | 0.9.5 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| void | 1.0.2 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| volatile-register | 0.2.2 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| walkdir | 2.5.0 | Unlicense/MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | crates/ |
| winapi-util | 0.1.11 | Unlicense OR MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| windows-sys | 0.52.0, 0.61.2 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| winnow | 0.7.14, 1.0.4 | MIT | crates/, esp32s3-agent, esp32s3-port |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| xtensa-lx-rt | 0.23.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 | crates/ |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | crates/ |
| zmij | 1.0.23 | MIT | crates/ |

npm packages (`web/package-lock.json`): 86, all build and test time only except the Svelte runtime, which ships in the page.

| Package | Version | Licence |
|---|---|---|
| @jridgewell/gen-mapping | 0.3.13 | MIT |
| @jridgewell/remapping | 2.3.5 | MIT |
| @jridgewell/resolve-uri | 3.1.2 | MIT |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT |
| @jridgewell/trace-mapping | 0.3.31 | MIT |
| @oxc-project/types | 0.156.0 | MIT |
| @rolldown/binding-android-arm-eabi | 1.2.16 | MIT |
| @rolldown/binding-android-arm64 | 1.2.16 | MIT |
| @rolldown/binding-darwin-arm64 | 1.2.16 | MIT |
| @rolldown/binding-darwin-x64 | 1.2.16 | MIT |
| @rolldown/binding-freebsd-x64 | 1.2.16 | MIT |
| @rolldown/binding-linux-arm-gnueabihf | 1.2.16 | MIT |
| @rolldown/binding-linux-arm64-gnu | 1.2.16 | MIT |
| @rolldown/binding-linux-arm64-musl | 1.2.16 | MIT |
| @rolldown/binding-linux-ppc64-gnu | 1.2.16 | MIT |
| @rolldown/binding-linux-s390x-gnu | 1.2.16 | MIT |
| @rolldown/binding-linux-x64-gnu | 1.2.16 | MIT |
| @rolldown/binding-linux-x64-musl | 1.2.16 | MIT |
| @rolldown/binding-openharmony-arm64 | 1.2.16 | MIT |
| @rolldown/binding-win32-arm64-msvc | 1.2.16 | MIT |
| @rolldown/binding-win32-x64-msvc | 1.2.16 | MIT |
| @rolldown/pluginutils | 1.0.1 | MIT |
| @sveltejs/acorn-typescript | 1.0.13 | MIT |
| @sveltejs/load-config | 0.2.3 | MIT |
| @sveltejs/vite-plugin-svelte | 7.3.1 | MIT |
| @types/chai | 5.2.3 | MIT |
| @types/deep-eql | 4.0.2 | MIT |
| @types/estree | 1.0.10 | MIT |
| @vitest/mocker | 5.0.5 | MIT |
| @vitest/spy | 5.0.5 | MIT |
| acorn | 8.19.0 | MIT |
| aria-query | 5.3.1 | Apache-2.0 |
| assertion-error | 2.0.1 | MIT |
| axobject-query | 4.1.0 | Apache-2.0 |
| chai | 6.3.0 | MIT |
| chokidar | 4.0.3 | MIT |
| clsx | 2.1.1 | MIT |
| deepmerge | 4.3.1 | MIT |
| detect-libc | 2.1.2 | Apache-2.0 |
| devalue | 5.9.4 | MIT |
| es-module-lexer | 2.3.2 | MIT |
| esm-env | 1.2.2 | MIT |
| esrap | 2.4.0 | MIT |
| estree-walker | 3.0.3 | MIT |
| expect-type | 1.4.0 | Apache-2.0 |
| fdir | 6.5.0 | MIT |
| fsevents | 2.3.3 | MIT |
| is-reference | 3.0.3 | MIT |
| lightningcss | 1.33.0 | MPL-2.0 |
| lightningcss-android-arm64 | 1.33.0 | MPL-2.0 |
| lightningcss-darwin-arm64 | 1.33.0 | MPL-2.0 |
| lightningcss-darwin-x64 | 1.33.0 | MPL-2.0 |
| lightningcss-freebsd-x64 | 1.33.0 | MPL-2.0 |
| lightningcss-linux-arm-gnueabihf | 1.33.0 | MPL-2.0 |
| lightningcss-linux-arm64-gnu | 1.33.0 | MPL-2.0 |
| lightningcss-linux-arm64-musl | 1.33.0 | MPL-2.0 |
| lightningcss-linux-x64-gnu | 1.33.0 | MPL-2.0 |
| lightningcss-linux-x64-musl | 1.33.0 | MPL-2.0 |
| lightningcss-win32-arm64-msvc | 1.33.0 | MPL-2.0 |
| lightningcss-win32-x64-msvc | 1.33.0 | MPL-2.0 |
| locate-character | 3.0.0 | MIT |
| magic-string | 0.30.21 | MIT |
| magic-string | 1.4.5 | MIT |
| mri | 1.2.0 | MIT |
| nanoid | 3.3.20 | MIT |
| obug | 2.2.1 | MIT |
| obug | 3.0.0 | MIT |
| picocolors | 1.1.1 | ISC |
| picomatch | 4.0.7 | MIT |
| postcss | 8.5.32 | MIT |
| readdirp | 4.1.2 | MIT |
| rolldown | 1.2.16 | MIT |
| sade | 1.8.1 | MIT |
| source-map-js | 1.2.2 | BSD-3-Clause |
| std-env | 4.3.0 | MIT |
| svelte | 5.57.3 | MIT |
| svelte-check | 4.7.8 | MIT |
| tinybench | 6.2.3 | MIT |
| tinyexec | 1.3.1 | MIT |
| tinyglobby | 0.2.17 | MIT |
| typescript | 6.0.3 | Apache-2.0 |
| vite | 8.3.5 | MIT |
| vitefu | 1.1.3 | MIT |
| vitest | 5.0.5 | MIT |
| why-is-node-running | 3.2.1 | MIT |
| zimmerframe | 1.1.5 | MIT |

## 自作コードの範囲

- DAGON Boot のために書いたもの: `crates/`、`targets/`（下の無改変のベンダーコンポーネントを除く）、`web/src/`、`web/scripts/`、`plugin/`、`topology/`、`scripts/` のすべて。
- Cargo と npm を通して改変せずに使う依存: 上の表の crate とパッケージ（例: embassy、esp-hal、ed25519-compact、sha2、serde、Svelte、Vite）。
- 無改変のベンダーコンポーネント: 下を参照。

## 使用していないコンポーネント (公開されているブートローダ実装は使用していません)

公開されているブートローダ実装（例: embassy-boot、MCUboot、wolfBoot）のコードもロジックも使っていません。起動の判断、イメージとリリースの形式、状態ログ、更新のプロトコル、受け手はこのプロジェクトのために書いたものです。

## 無改変のベンダーコンポーネント

- ESP32-S3 の 2nd stage ブートローダ: 公式の ESP-IDF v6.1 のブートローダを、アプリのロールバックを有効にして公式のソースからビルドしたもの（`targets/esp32s3-2ndstage` で自作したのはプロジェクトのファイルと `sdkconfig.defaults` だけ）。ESP32-S3 のポートは otadata を通してスロットを選びます。
- RP2040 の boot2: `rp2040-boot2` crate（BSD-3-Clause）の generic 03h の second stage を、`targets/rp2040-stage0` がフラッシュの先頭に改変せずに置いています。
- STM32F401 の ROM にあるシステムメモリのブートローダ（文書化された I2C の手順で復旧に使います）。

## 制限事項

- ESP32-S3 は公式の ESP-IDF の 2nd stage を使います。その 2nd stage 自体が壊れた場合、DAGON では救えません。ベンダーのツールで USB から書き直す必要があります。
- 署名の鍵は PC 上のファイルで、公開鍵はファームウェアに組み込まれます。ハードウェアの信頼の起点（セキュアブートのヒューズやハードウェアの鍵保管）はありません。
- 実機でのすべてのフラッシュ操作に対する電断の網羅的な調査はまだ行っていません。電断はシミュレータでテストしています。
- USB の VID:PID `c0de:cafe` は試験用の ID で、割り当てを受けたものではありません。
- 親機のハードウェア（LCD 付き RP2040）と 3 台での通しの流れは、まだ実機で確認中です。
- `dagon conform` は同期の試験手順を動かすもので CLI 専用です。ブラウザでは適合試験を実行しません。
