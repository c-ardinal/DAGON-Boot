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

### 形式検証

2種類の検証が、テストより広い範囲を網羅します。

- **stage0の判定のKaniによる証明**（`crates/dagon-core/src/proofs.rs`）。stage0は現場で更新しないので、パニックはROMローダ以外で復旧不可能な状態になります。証明はフラッシュと保持メモリの全内容、署名検証の全応答に対して、状態ログの読込と畳込、そしてstage0の判定（`bootcore::start`）の組立がパニックしないこと、試行のセルと起動の選択の規則が成立することを検査します。前提: フラッシュの読出は毎回任意の値、状態ログのCRC-32は任意の値（スタブ）、状態ログの面は記録2個です。イメージのヘッダと本体の検査は、512バイトの記号的なヘッダでソルバーの所要時間が過大なので、判定の証明では任意の結果に置換し、テストと網羅探索が担当します。ループの本体は反復ごとに同一なので、この上限は経路を隠蔽しません。署名検証自体の正当性は、Ed25519のベクタと差分のテストが担当します。
- **ノード全体の網羅探索**（`targets/mcu/stm32f401re/tests/explore.rs`）。F401の本番のコード（stage0の判定、ブートローダのアプリの判定と決着、エージェント、ストア）で、アプリとブートローダの書込、CONFIRM、起動、電断の全系列を上限まで実行し、全フラッシュ操作の後のリセットか電断を挿入します。ブートローダのスロット2個と1個の両構成が対象です。状態はフラッシュと保持メモリの内容で比較し、同一の状態は1回だけ展開します。性質: stage0は必ず何かを起動する（N1）、破損したイメージは確定しない（N2）、確定済のアプリが存続する（N3）、追加の故障がなければ有限回の起動で正常なアプリが動作する（N4）。違反は最短の系列で表示します。

```
cargo kani -Z stubbing -p dagon-core          # Kani 0.68以降
cd targets/mcu/stm32f401re
cargo test --no-default-features --target <host triple> --release --test explore
DAGON_EXPLORE_STEPS=8 DAGON_EXPLORE_FAULTS=2 ...  # 上限の拡大（既定: 6、書込2、故障1）
DAGON_EXPLORE_KNOWN=1 ...                          # 既知の不具合の経路も探索
```

CIは両方を実行します（`.github/workflows/ci.yml`の`kani`と、F401のパッケージのホストテスト）。

### リリースのアーカイブを作成する

`python scripts/release.py [--target <triple>]`はそのツリーからページをビルドし、ページを組込んだ`out/release/dagon-cli-<version>-<target>.*`を作成します。組込対象のページがない場合はビルドが停止します。

`.github/workflows/release.yml`は、`cli-v*`のタグのpush時に同一の処理を各プラットフォームで実行します。

1. コミットが要求する上げ幅をバージョンが満たすかを`scripts/bump_check.py`で確認します。
2. 各システムのアーカイブと`.sha256`を作成します。
3. 全ビルドの成功後、GitHubのリリースを公開します。リリースには各システムのアーカイブと`.sha256`を添付します。

ビルドが1件でも失敗した場合は公開しません。手動実行の場合も公開しません。

バージョンの規則は[バージョンとリリース](/explanation/versioning/)に記載しています。
