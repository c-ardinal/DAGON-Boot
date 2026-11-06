---
title: "リポジトリ"
description: "リポジトリにあるもの、その場所、DAGONそのものを開発するときのことです。"
---

## リポジトリの構成

| パス | 内容 |
|---|---|
| `crates/` | DAGONのcrate。一覧は[Rust API](/reference/rust-api/)にあります |
| `targets/` | MCUごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の定義、RP2040親機のファームウェア、立ち上げ用ツール |
| `samples/` | 定義を使うサンプルのアプリ（DAGONの一部ではない） |
| `web/` | DAGON Console（Svelte、Vite、TypeScript）。できることは`web/README.md`にあります |
| `topology/` | 構成の例 |
| `plugin/` | 新しいマイコンへの移植に使うClaude Codeのプラグイン（エージェント、スキル、フック） |
| `scripts/licences.py` | [ライセンス](/reference/licenses/)の依存ライブラリの表を生成する |

## DAGONそのものを開発するとき

### 要るもの

- [入れる](/getting-started/install/)のツールのすべて
- `wasm32-unknown-unknown`のターゲット（ターゲットはすべて`rust-toolchain.toml`にあります）
- `crates/dagon-wasm`用の`wasm-pack` 0.15.1（対応する`wasm-bindgen`と`wasm-opt`はこのツールが自分で取得します）
- `web/`用のNode.jsとnpm
- RP2040の立ち上げ用イメージに使う`elf2uf2-rs`
- ESP32-S3の2nd stageを`targets/esp32s3-2ndstage`からビルドし直すときはESP-IDF v6.1。ビルド済みの無改変のものを`targets/mcu/esp32s3/vendor/bootloader.bin`として同梱しています（`vendor/PROVENANCE.md`）。

### クローンを使う

`cargo install --path crates/dagon-cli`で、クローンから`dagon`コマンドを入れます。

プロジェクトは`dagon new --dagon-path <クローン>`（または`DAGON_HOME`を設定）でそのクローンを使います。`dagon.toml`には`[dagon] path`が書かれます。

### リリースのアーカイブを作る

`python scripts/release.py [--target <triple>]`が、ページを組み込んだ`out/release/dagon-cli-<version>-<target>.*`を作ります。

`.github/workflows/release.yml`は、`cli-v*`のタグがpushされたときに同じことを各プラットフォームで行います。

1. コミットが求める上げ幅にバージョンが足りているかを`scripts/bump_check.py`で確かめます。
2. 各システムのアーカイブと`.sha256`を作ります。
3. すべてのビルドが通ると、GitHubのリリースを公開します。リリースは各システムのアーカイブと`.sha256`を持ちます。

ビルドが1つでも失敗すると、公開しません。手で動かしたときも公開しません。

バージョンの決まりは[バージョンとリリース](/explanation/versioning/)にあります。
