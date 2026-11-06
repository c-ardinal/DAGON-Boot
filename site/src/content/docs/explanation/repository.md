---
title: "リポジトリ"
description: "リポジトリにあるもの、その場所、DAGON そのものを開発するときのことです。"
---

このリポジトリのコードとテストが示していること:

- 形式とプロトコル: `dagon-format` と `dagon-proto`。`crates/dagon-format/golden/` の黄金ベクタで照合しています。
- ノード側: `dagon-agent`（非稼働スロットへイメージを受け取り、署名と SHA-256 を検査し、切り替えと確定を行う）と `dagon-core`（起動の判断: 回数を限った試行起動と巻き戻し）。
- PC 側: `dagon-host`（状態、自動で見つける、構成の送信、書き込み、前のバージョンに戻す、確定、復旧）、`dagon-romlink`（STM32 の I2C ROM ブートローダの手順）、`dagon` CLI。これらは `dagon-sim`（F401 子機付きの模擬親機、電断点付きのフラッシュ、損失のあるリンク、ROM ブートローダの模擬）を相手にテストしています。
- プロジェクト: `dagon new`、`dagon gen`、`dagon build` が、MCU ごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の定義の上にノードの crate を作り、ビルドします。
- `dagon conform`: `dagon-conform` の 12 件の適合試験。模擬の F401 に対し、親機経由でも直結でも 12 件すべて合格します。
- 構成 YAML: 22 件の検証規則と対応ボードの表（`dagon-topology`）。`topology/sample.yaml` はこれを通ります。
- DAGON Console（`web/`。`dagon serve` が配る。できることは `web/README.md` を参照）。
- ファームウェア: RP2040 親機（`targets/rp2040-hub`）、F401 のデモ子機（`samples/f401-app`）、ESP32-S3 子機（`targets/boards/m5stopwatch`、`samples/esp32s3-app`）。どれを実機で確認したかは「制限事項」にあります。


## リポジトリの構成


| パス | 内容 |
|---|---|
| `crates/dagon-format` | イメージヘッダ、マニフェスト、リリースバンドル、状態ログ、構成バイナリ、黄金ベクタ |
| `crates/dagon-proto` | 通信プロトコル v1: フレーム、コマンド、ペイロード |
| `crates/dagon-port` | ボードの trait（フラッシュ、リンク、リセット、ウォッチドッグ、起動先の選択、I2C、子機の制御線） |
| `crates/dagon-core` | 起動の判断（試行、回数、巻き戻し） |
| `crates/dagon-agent` | ノード側で書き込みを受ける部分 |
| `crates/dagon-conform` | 12 件の適合試験と結果ファイルの形式 |
| `crates/dagon-sim` | シミュレータ: フラッシュ、リンク、ROM ブートローダ、ノード、親機 |
| `crates/dagon-topology` | 構成 YAML、検証規則、対応ボード |
| `crates/dagon-sign` | 鍵、イメージの署名、リリースの作成、初期導入用ファイル |
| `crates/dagon-host` | CLI と DAGON Console が共有する PC 側の手順 |
| `crates/dagon-romlink` | STM32 の I2C ROM ブートローダの手順 |
| `crates/dagon-cli` | `dagon` コマンド（DAGON Console を組み込み） |
| `crates/dagon-wasm` | DAGON Console 用の WebAssembly バインディング |
| `targets/` | MCU ごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の定義、RP2040 親機のファームウェア、立ち上げ用ツール |
| `samples/` | 定義を使うサンプルのアプリ（DAGON の一部ではない） |
| `web/` | DAGON Console（Svelte、Vite、TypeScript） |
| `topology/` | 構成の例 |
| `plugin/` | 新しいマイコンへの移植に使う Claude Code のプラグイン（エージェント、スキル、フック） |
| `scripts/licences.py` | 下の依存ライブラリのライセンス表を生成する |


## DAGON そのものを開発するとき


- [必要なもの](/getting-started/install/)のすべてと、`wasm32-unknown-unknown` のターゲット（ターゲットはすべて `rust-toolchain.toml` にあります）
- `crates/dagon-wasm` 用の `wasm-pack` 0.15.1（対応する `wasm-bindgen` と `wasm-opt` は自分で取得します）と、`web/` 用の Node.js と npm
- ESP32-S3 の 2nd stage を `targets/esp32s3-2ndstage` からビルドし直すときは ESP-IDF v6.1（ビルド済みのものを `targets/mcu/esp32s3/vendor/bootloader.bin` として同梱している。無改変の ESP-IDF v6.1。`vendor/PROVENANCE.md` を参照）
- RP2040 の立ち上げ用イメージに使う `elf2uf2-rs`
- `cargo install --path crates/dagon-cli` でクローンから CLI を入れます。プロジェクトは `dagon new --dagon-path <クローン>`（または `DAGON_HOME`）でそのクローンを使います
- リリースのアーカイブ: `python scripts/release.py [--target <triple>]` が、ページを組み込んだ `out/release/dagon-cli-<version>-<target>.*` を作ります。`.github/workflows/release.yml` は `cli-v*` のタグが push されたときに、コミットが求める上げ幅にバージョンが足りているかを `scripts/bump_check.py` で確かめてから各プラットフォームで同じことをし、ワークフローの成果物として残します（公開はしません）
