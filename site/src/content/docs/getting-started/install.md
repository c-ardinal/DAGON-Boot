---
title: "インストール"
description: "dagonコマンドをリリースからインストールする方法と、独自にビルドする方法です。"
---

## リリースからのインストール

LinuxとmacOS:

```sh
curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/setup.sh | sh
```

Windows（PowerShell）:

```powershell
irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/setup.ps1 | iex
```

スクリプトの処理は以下のとおりです。

1. 利用者のシステム用のアーカイブを、最新のリリース（`cli-v<version>`）から取得します。
2. `.sha256`のファイルでアーカイブを検証します。不一致の場合は停止します。
3. `dagon`を、管理者権限が不要なフォルダに配置します。LinuxとmacOSは`~/.local/bin`、Windowsは`%LOCALAPPDATA%\Programs\dagon`です。
4. WindowsはそのフォルダをユーザーのPATHに追加します。LinuxとmacOSは追加方法を表示します。
5. 実行内容を表示します。

スクリプトは以下の環境変数を参照します。

| 環境変数 | 意味 |
|---|---|
| `DAGON_VERSION` | インストールするバージョン。`setup.sh`は引数でも指定可能です（`sh -s -- <version>`）。プレリリース（`<version>-rc.N`）は、この方法で指定した場合だけインストールします |
| `DAGON_INSTALL_DIR` | 配置先のフォルダ |
| `DAGON_NO_MODIFY_PATH` | `setup.ps1`で、PATHを変更せずに追加方法（アンインストールでは削除方法）を表示します |
| `DAGON_UNINSTALL` | 空以外の場合、インストールせずにアンインストールします。`setup.sh`は引数`--uninstall`でも指定可能です |

`cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`でも同一のアーカイブを取得します。`cargo-binstall`が必要です。

このリポジトリのクローンもgitのsubmoduleも不要です。

DAGON Consoleの表示に使用するブラウザはChromeかEdge 89以降です（Web Serial）。

インストール結果を確認します。

```sh
dagon --help
```

DAGON Consoleは`dagon`に内蔵しています。Node.jsは不要です。

## アンインストール

インストール時と同一のスクリプトでアンインストールします。ダウンロードはしません。

LinuxとmacOS:

```sh
curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/setup.sh | sh -s -- --uninstall
```

Windows（PowerShell）:

```powershell
$env:DAGON_UNINSTALL = "1"; irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/setup.ps1 | iex
```

PowerShellのスクリプトは実行後に`DAGON_UNINSTALL`を削除します。同一のウィンドウで再度実行すると、アンインストールではなくインストールします。

- 削除対象は`dagon`だけです。Windowsはそのフォルダにファイルがない場合はフォルダを削除し、ユーザーのPATHとこのプロセスのPATHからそのフォルダの項目も削除します。インストーラーによる項目の追加は記録がないため、項目が存在すれば削除します。
- `~/.dagon`は変更しません。署名のキー（`~/.dagon/keys`）は残存します。削除すると、そのキーで書込したノードには以後は書込不可能になります。
- 再度実行しても成功します。
- `cargo install`でインストールした場合は`cargo uninstall dagon-cli`です。

## 独自のビルド

`dagon`コマンドを、このリポジトリのクローンからビルドする手順です。DAGONを改変するか否かに関係なく同一です。使用だけが目的の場合は、上記のスクリプトでリリースからインストールします。

| 必要なもの | 入手方法 |
|---|---|
| Rust 1.95.0以降 | <https://rustup.rs> |
| Cコンパイラ（`ring` crate） | WindowsはVisual Studio 2022のC++ビルドツール、Linuxはgcc 13以降、macOSはXcode Command Line Tools 16.4以降 |
| Python 3、Node.jsとnpm、`wasm-pack` 0.15.1（DAGON Consoleのページ） | <https://www.python.org>、<https://nodejs.org>、`npm install --global wasm-pack@0.15.1` |

DAGON Consoleのページを最初にビルドします。`dagon`はビルド時にそのページを内蔵します。ページが不在の場合、`dagon serve`はページのビルド方法の表示だけをします。

```sh
python scripts/page.py build
cargo install --path crates/dagon-cli
```

## ファームウェア開発用のツール

ファームウェアの開発には、[Rust](https://rustup.rs)と[マイコンごとのツール](/reference/tools/)が必要です。構成の作図後は、ビルドの開始前に不足分を`dagon`で確認します。

```sh
dagon doctor           # 導入済のものと不足分
dagon doctor --fix     # 管理者権限が不要なものをインストール
```

プロジェクトのフォルダで実行すると、その構成に必要なものを確認します。それ以外のフォルダでは、全マイコンの分を確認します。

STM32F401REのノードがある場合、`dagon doctor`はST-LINKの接続状態も確認します（`probe-rs list`）。この項目は`--fix`では修正不可能です。

## ネットワークが必要な場合

`dagon new`で作成するプロジェクトは、DAGONを利用者の`dagon`と同一のリリースから取得します（`dagon.toml`の`[dagon] git`と`tag = "cli-v<version>"`）。

- ビルド時にcargoが取得します。
- `dagon flash`は、ブートローダ、stage0、factoryエージェントを、cargoが取得したそのタグのコピーからビルドします。

そのため初回はネットワークが必要です。

DAGON自体の開発については[リポジトリ](/explanation/repository/)を参照してください。
