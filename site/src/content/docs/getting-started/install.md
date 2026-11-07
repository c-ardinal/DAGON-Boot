---
title: "入れる"
description: "dagonコマンドをリリースから入れる方法と、自分でビルドする方法です。"
---

## リリースから入れる

LinuxとmacOS:

```sh
curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.sh | sh
```

Windows（PowerShell）:

```powershell
irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.ps1 | iex
```

スクリプトは次のことをします。

1. 自分のシステム用のアーカイブを、最新のリリース（`cli-v<version>`）から取得します。
2. `.sha256`のファイルでアーカイブを確かめます。合わないときは止まります。
3. `dagon`を、管理者の権限の要らないフォルダに置きます。LinuxとmacOSは`~/.local/bin`、Windowsは`%LOCALAPPDATA%\Programs\dagon`です。
4. WindowsはそのフォルダをユーザーのPATHに足します。LinuxとmacOSは足し方を表示します。
5. したことを表示します。

スクリプトは次の環境変数を読みます。

| 環境変数 | 意味 |
|---|---|
| `DAGON_VERSION` | 入れるバージョン。`install.sh`は引数でも受け取ります（`sh -s -- <version>`）。プレリリース（`<version>-rc.N`）は、こうして指定したときだけ入ります |
| `DAGON_INSTALL_DIR` | 置くフォルダ |
| `DAGON_NO_MODIFY_PATH` | `install.ps1`で、PATHを変えず足し方を表示します |

`cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`でも同じアーカイブを取得します。`cargo-binstall`が要ります。

このリポジトリのクローンもgitのsubmoduleも要りません。

DAGON Consoleを開くブラウザはChromeかEdge 89以降です（Web Serial）。

入ったことを確かめます。

```sh
dagon --help
```

DAGON Consoleは`dagon`に組み込まれています。Node.jsは要りません。

## 自分でビルドする

`dagon`コマンドをソースからビルドする手順です。DAGONに手を加えるかどうかによらず同じです。

| 要るもの | 入れ方 |
|---|---|
| Rust 1.95.0以降 | <https://rustup.rs> |
| Cコンパイラ（`ring` crate） | WindowsはVisual Studio 2022のC++ビルドツール、Linuxはgcc 13以降、macOSはXcode Command Line Tools 16.4以降 |

```sh
cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli
```

クローンからビルドするときは`cargo install --path crates/dagon-cli`です。

## ファームウェアを開発するためのツール

ファームウェアを開発するには、[Rust](https://rustup.rs)と[マイコンごとのツール](/reference/tools/)が要ります。構成を描いたら、ビルドの前に足りないものを`dagon`に確かめさせます。

```sh
dagon doctor           # あるものと足りないもの
dagon doctor --fix     # 管理者権限の要らないものを入れる
```

プロジェクトのフォルダで実行すると、その構成に要るものを確かめます。それ以外の場所では、すべてのマイコンの分を確かめます。

STM32F401REのノードがあると、`dagon doctor`はST-LINKがつながっているかも確かめます（`probe-rs list`）。これは`--fix`では直せません。

## ネットワークが要るとき

`dagon new`で作るプロジェクトは、DAGONを手元の`dagon`と同じリリースから取得します（`dagon.toml`の`[dagon] git`と`tag = "cli-v<version>"`）。

- ビルドのときにcargoが取得します。
- `dagon flash`は、ブートローダ、stage0、factoryエージェントを、cargoが取得したそのタグのコピーからビルドします。

そのため、最初の1回はネットワークが要ります。

DAGONそのものを開発するときは[リポジトリ](/explanation/repository/)を見てください。
