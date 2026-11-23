---
title: "リポジトリ"
description: "リポジトリの内容と配置、DAGON自体の開発手順です。"
---

対象読者はDAGON貢献者です。

## リポジトリの構成

| パス | 内容 |
|---|---|
| `crates/` | DAGONのcrate。一覧は[Rust API](/reference/rust-api/)にあります |
| `targets/` | MCUごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の定義。リレー本体はマイコン非依存の`crates/dagon-relay`にあり、リレー可能なマイコンは定義の`[coordinator]`で決定します。ノードは定義の`[[relay_link]]`が列挙するリンクで中継し、中継先は構成が決定します |
| `samples/` | 定義を使用するサンプルアプリ（DAGONの一部ではない） |
| `web/` | DAGON Console（Svelte、Vite、TypeScript）。機能の一覧は`web/README.md`に記載しています |
| `topology/` | 構成の例 |
| `plugin/` | 新規マイコンへの移植に使用するClaude Codeのプラグイン（エージェント、スキル、フック） |
| `scripts/licences.py` | [ライセンス](/reference/licenses/)に記載した依存ライブラリ表の生成 |

## DAGON自体の開発

### 必要な環境

- [インストール](/getting-started/install/)に記載した全ツール
- `wasm32-unknown-unknown`のターゲット（全ターゲットは`rust-toolchain.toml`に記載しています）
- `crates/dagon-wasm`用の`wasm-pack` 0.15.1（対応する`wasm-bindgen`と`wasm-opt`はこのツールが自動で取得します）
- `web/`用のNode.jsとnpm
- ESP32-S3の2nd stageを`targets/esp32s3-2ndstage`から再ビルドする場合はESP-IDF v6.1。ビルド済で無改変のバイナリを`targets/mcu/esp32s3/vendor/bootloader.bin`として同梱しています（`vendor/PROVENANCE.md`）。

### クローンの使用

最初に`python scripts/page.py build`でDAGON Consoleのページを`crates/dagon-cli/page`にビルドします。その後に`cargo install --path crates/dagon-cli`でクローンから`dagon`コマンドをインストールします。`dagon`はビルド時にそのページを組込みます。ページはビルドの成果物なので、gitには格納しません。`web/`の変更後はページを再ビルドします。

プロジェクトは`dagon new --dagon-path <クローン>`（または`DAGON_HOME`を設定）でそのクローンを使用します。`dagon.toml`には`[dagon] path`を記録します。

### リリースのアーカイブを作成する

`python scripts/release.py [--target <triple>]`はそのツリーからページをビルドし、ページを組込んだ`out/release/dagon-cli-<version>-<target>.*`を作成します。組込対象のページがない場合はビルドが停止します。

`.github/workflows/release.yml`は、`cli-v*`のタグのpush時に同一の処理を各プラットフォームで実行します。

1. コミットが要求する上げ幅をバージョンが満たすかを`scripts/bump_check.py`で確認します。
2. 各システムのアーカイブと`.sha256`を作成します。
3. 全ビルドの成功後、GitHubのリリースを公開します。リリースには各システムのアーカイブと`.sha256`を添付します。

ビルドが1件でも失敗した場合は公開しません。手動実行の場合も公開しません。

バージョンの規則は[バージョンとリリース](/explanation/versioning/)に記載しています。
