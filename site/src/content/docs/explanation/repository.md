---
title: "リポジトリ"
description: "リポジトリにあるもの、その場所、DAGON そのものを開発するときのことです。"
---

## リポジトリの構成

| パス | 内容 |
|---|---|
| `crates/` | DAGON の crate。一覧は [Rust API](/reference/rust-api/) にあります |
| `targets/` | MCU ごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の定義、RP2040 親機のファームウェア、立ち上げ用ツール |
| `samples/` | 定義を使うサンプルのアプリ（DAGON の一部ではない） |
| `web/` | DAGON Console（Svelte、Vite、TypeScript）。できることは `web/README.md` にあります |
| `topology/` | 構成の例 |
| `plugin/` | 新しいマイコンへの移植に使う Claude Code のプラグイン（エージェント、スキル、フック） |
| `scripts/licences.py` | [ライセンス](/reference/licenses/)の依存ライブラリの表を生成する |

## DAGON そのものを開発するとき

### 要るもの

- [入れる](/getting-started/install/)のツールのすべて
- `wasm32-unknown-unknown` のターゲット（ターゲットはすべて `rust-toolchain.toml` にあります）
- `crates/dagon-wasm` 用の `wasm-pack` 0.15.1（対応する `wasm-bindgen` と `wasm-opt` は自分で取得します）
- `web/` 用の Node.js と npm
- RP2040 の立ち上げ用イメージに使う `elf2uf2-rs`
- ESP32-S3 の 2nd stage を `targets/esp32s3-2ndstage` からビルドし直すときは ESP-IDF v6.1。ビルド済みの無改変のものを `targets/mcu/esp32s3/vendor/bootloader.bin` として同梱しています（`vendor/PROVENANCE.md`）。

### クローンを使う

`cargo install --path crates/dagon-cli` で、クローンから `dagon` コマンドを入れます。

プロジェクトは `dagon new --dagon-path <クローン>`（または `DAGON_HOME` を設定）でそのクローンを使います。`dagon.toml` には `[dagon] path` が書かれます。

### リリースのアーカイブを作る

`python scripts/release.py [--target <triple>]` が、ページを組み込んだ `out/release/dagon-cli-<version>-<target>.*` を作ります。

`.github/workflows/release.yml` は、`cli-v*` のタグが push されたときに同じことを各プラットフォームで行います。

1. コミットが求める上げ幅にバージョンが足りているかを `scripts/bump_check.py` で確かめます。
2. 各システムのアーカイブと `.sha256` を作ります。
3. すべてのビルドが通ると、GitHub のリリースを公開します。リリースは各システムのアーカイブと `.sha256` を持ちます。

ビルドが 1 つでも失敗すると、公開しません。手で動かしたときも公開しません。

バージョンの決まりは[バージョンとリリース](/explanation/versioning/)にあります。
