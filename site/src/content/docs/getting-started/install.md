---
title: "入れる"
description: "DAGON に要るツールと dagon コマンドです。"
---

## 要るツール

| ツール | 要る場面 | 入れ方 |
|---|---|---|
| Rust（`rustup`） | いつも | <https://rustup.rs> |
| C コンパイラ | `dagon` コマンドをソースからビルドするとき（`ring` crate） | Windows は MSVC の Build Tools、ほかは `cc` |
| Chrome か Edge 89 以降 | DAGON Console（Web Serial） | — |

マイコンごとに要るツールは[マイコンごとのツール](/reference/tools/)にあります。

## `dagon` コマンドを入れる

次のどれかで入れます。

- ビルド済みのファイル: 使うバージョンの GitHub のリリース（`cli-v<version>`）から、自分のシステム用のアーカイブを取ります（Windows は `dagon-cli-<version>-<target>.zip`、それ以外は `dagon-cli-<version>-<target>.tar.gz`）。`.sha256` のファイルで確かめてから、`dagon` を `PATH` の通った場所に置きます。
- `cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`: 同じアーカイブを取ります。`cargo-binstall` が要ります。
- ソースからビルド: `cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli`

このリポジトリのクローンも git の submodule も要りません。

入ったことを確かめます。

```sh
dagon --help
```

DAGON Console は `dagon` に組み込まれています。Node.js が要るのは、DAGON Console そのものを開発するときだけです（`web/README.md`）。

## ツールを確かめる

足りないツールを `dagon` に確かめさせます。

```sh
dagon doctor           # あるものと足りないもの
dagon doctor --fix     # 管理者権限の要らないものを入れる
```

プロジェクトのフォルダで実行すると、その構成に要るものを確かめます。それ以外の場所では、すべてのマイコンの分を確かめます。

STM32F401RE のノードがあると、`dagon doctor` は ST-LINK がつながっているかも確かめます（`probe-rs list`）。これは `--fix` では直せません。

## ネットワークが要るとき

`dagon new` で作るプロジェクトは、DAGON を手元の `dagon` と同じリリースから取ります（`dagon.toml` の `[dagon] git` と `tag = "cli-v<version>"`）。

- ビルドのときに cargo が取得します。
- `dagon flash` は、ブートローダ、stage0、factory エージェントを、そのタグの cargo の写しからビルドします。

そのため、最初の 1 回はネットワークが要ります。

DAGON そのものを開発するときは[リポジトリ](/explanation/repository/)を見てください。
