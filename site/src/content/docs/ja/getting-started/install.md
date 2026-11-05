---
title: "入れる"
description: "DAGON に要るツールと dagon コマンドです。"
---

| ツール | 要る場面 | 入れ方 |
|---|---|---|
| Rust（`rustup`） | いつも | <https://rustup.rs> |
| C コンパイラ | `dagon` CLI をソースからビルドするとき（`ring` crate） | Windows は MSVC の Build Tools、ほかは `cc` |
| Chrome か Edge 89 以降 | DAGON Console（Web Serial） | — |

マイコンごとに要るツールは [Tools by MCU](/ja/reference/tools/) にあります。`dagon doctor` が確かめます。


`dagon` CLI は、次のどれかで入れます:

- 使うバージョンの GitHub のリリース（`cli-v<version>`）から、自分のシステム用のアーカイブ（Windows は `dagon-cli-<version>-<target>.zip`、それ以外は `dagon-cli-<version>-<target>.tar.gz`）を取り、`.sha256` のファイルで確かめてから、`dagon` を `PATH` の通った場所に置く。
- `cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`（同じアーカイブを取ります。`cargo-binstall` が要ります）。
- ビルドする: `cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli`。

このリポジトリのクローンも git の submodule も要りません:

```sh
dagon --help
```

`dagon new` で作るプロジェクトは、DAGON を手元の `dagon` と同じリリースから取ります（`dagon.toml` の `[dagon] git` と `tag = "cli-v<version>"`）。ビルドのときに cargo が取得し、`dagon flash` もブートコア・stage0・factory エージェントをそのタグの cargo の写しからビルドするので、最初の 1 回はネットワークが要ります。DAGON そのものを開発するときはクローンを使います: `dagon new my-fleet --dagon-path <クローン>`（または `DAGON_HOME` を設定）で `[dagon] path` を書きます。

続けて、構成に要るツールを `dagon` に確かめさせます（プロジェクトのフォルダで実行します。それ以外では全部のマイコンの家族を確かめます）:

```sh
dagon doctor           # あるものと足りないもの
dagon doctor --fix     # 管理者権限の要らないものを入れる
```

STM32F401RE のノードがあると、`dagon doctor` は ST-LINK がつながっているかも確かめます（`probe-rs list`）。これは `--fix` では直せません。

DAGON Console は `dagon` に組み込まれています。Node.js が要るのは、DAGON Console そのものを開発するときだけです（`web/README.md`）。
