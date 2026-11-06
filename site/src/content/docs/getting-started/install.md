---
title: "入れる"
description: "dagon コマンドをリリースから入れる方法と、自分でビルドする方法です。"
---

## リリースから入れる

Linux と macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.sh | sh
```

Windows（PowerShell）:

```powershell
irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.ps1 | iex
```

スクリプトは次のことをします。

1. 自分のシステム用のアーカイブを、最新のリリース（`cli-v<version>`）から取ります。
2. `.sha256` のファイルで確かめます。合わないときは止まります。
3. `dagon` を、管理者の権限の要らないフォルダに置きます。Linux と macOS は `~/.local/bin`、Windows は `%LOCALAPPDATA%\Programs\dagon` です。
4. Windows はそのフォルダをユーザーの PATH に足します。Linux と macOS は足し方を表示します。
5. したことを表示します。

スクリプトは環境変数を読みます。

| 環境変数 | 意味 |
|---|---|
| `DAGON_VERSION` | 入れるバージョン。`install.sh` は引数でも受け取ります（`sh -s -- <version>`） |
| `DAGON_INSTALL_DIR` | 置くフォルダ |
| `DAGON_NO_MODIFY_PATH` | `install.ps1` で、PATH を変えず足し方を表示します |

`cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot` でも同じアーカイブを取ります。`cargo-binstall` が要ります。

このリポジトリのクローンも git の submodule も要りません。

DAGON Console を開くブラウザは Chrome か Edge 89 以降です（Web Serial）。

入ったことを確かめます。

```sh
dagon --help
```

DAGON Console は `dagon` に組み込まれています。Node.js は要りません。

## 自分でビルドする

`dagon` コマンドをソースからビルドする手順です。DAGON に手を加えるかどうかによらず同じです。

| 要るもの | 入れ方 |
|---|---|
| Rust 1.95.0 以降 | <https://rustup.rs> |
| C コンパイラ（`ring` crate） | Windows は Visual Studio 2022 の C++ ビルドツール、Linux は gcc 13 以降、macOS は Xcode Command Line Tools 16.4 以降 |

```sh
cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli
```

クローンからは `cargo install --path crates/dagon-cli` です。

## ファームウェアを開発するためのツール

ファームウェアを開発するには、[Rust](https://rustup.rs) と[マイコンごとのツール](/reference/tools/)が要ります。構成を描いたら、ビルドの前に足りないものを `dagon` に確かめさせます。

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
