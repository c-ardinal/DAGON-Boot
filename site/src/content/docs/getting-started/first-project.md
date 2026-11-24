---
title: "導入から書込まで"
description: "プロジェクトの開始、構成の作図、ビルド、最初のバージョンの書込、新規バージョンの書込とロールバックの手順です。"
---

このページの例はサンプルの構成です。円形LCDのRP2040（PC接続ノード）、NUCLEO-F401RE（I2C）、M5StopWatch（UART）の3ノードです。手順2で独自の構成に置換可能です。

接続先のノードで動作するサンプルアプリの内容は`samples/f401-app`と`samples/esp32s3-app`にあります。

`dagon`コマンドとツールは、事前に[インストール](/getting-started/install/)の手順で用意します。

## 1. プロジェクトの開始

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new`は以下のものを作成します。

- `dagon.toml`
- `topology.yaml`: サンプルの構成です。`--from <yaml>`の指定で独自の構成になります。
- `.gitignore`
- Claude Codeのファイル（[プロジェクトでのClaudeの支援](/how-to/claude-help/)）
- 署名のキーペア（未作成の場合）: `~/.dagon/keys/my-fleet`に作成します。`--keys <dir>`で保存先を変更可能です。

秘密鍵はプロジェクトの外部、gitの作業ツリーの外部に配置します。`dagon.toml`には公開鍵の保存先だけを記述します。

署名のキーの保護方法は利用者が選択します。DAGONには選択した格納場所からキーを取得するポート（[`KeyStore`](/reference/ports/)）があります。既定はキーのファイルです。

署名時は秘密鍵のファイルの保存先を`dagon`に指定します。

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

キーをファイル以外に保管する場合は、`DAGON_SIGNING_KEY`の代替として`DAGON_SIGNING_KEY_COMMAND`を設定し、キーの16進数64桁（キーのファイルの内容）を出力するコマンドを指定します。macOSのキーチェーンと1Passwordの例です。

```sh
export DAGON_SIGNING_KEY_COMMAND='security find-generic-password -s dagon-signing -w'
export DAGON_SIGNING_KEY_COMMAND='op read op://Private/dagon-signing/seed'
```

`dagon`は署名のたびにこのコマンドをシェル経由で実行します。出力内容はメッセージに表示しません。環境変数でキーを指定する場合は、16進数64桁を`DAGON_SIGNING_KEY_SEED`に設定します。`dagon`が起動するビルドとエージェントからはこれらの環境変数を除去しますが、キーのコマンドが参照する他の環境変数は対象外です。3つの環境変数のうち設定可能なのは1つだけです。

## 2. 構成の作成

### DAGON Consoleでの作成

```sh
dagon serve
```

DAGON Consoleがブラウザに表示されます。127.0.0.1だけで動作し、一時的なパスフレーズが付与されます。プロジェクトが不在の場合は、プロジェクトの作成画面か選択画面から開始します。

1. ノードリストからノードを配置します。製品とマイコンのどちらでも選択可能です。
2. ノード間をリンクで接続します。
3. 検査で表示された問題を修正します。

編集内容は`topology.yaml`に保存します。

### コマンドでの確認

```sh
dagon topo check topology.yaml
```

### 文章による説明からの作成

文章で記述した説明から、Claude APIで構成の案を作成可能です。

```sh
dagon ai key set                    # APIキーを入力（非表示）。OSの資格情報ストアに保存
dagon ai link --way api             # 送信への同意（初回だけ）
dagon ai topology --in boards.txt --out topology.yaml
```

APIキーの保存先はOSの資格情報ストアだけです。環境変数やファイルには保存しません。

- Windows: 資格情報マネージャー
- macOS: キーチェーン
- Linux: GNOME KeyringなどのSecret Service

`dagon ai key status`で登録状態を確認し、`dagon ai key delete`で削除します。DAGON Consoleの「CLI連携」でも登録可能です。

資格情報ストアが不在の環境では、利用者自身のログインで使用するClaude Codeを選択してください（`dagon ai link`）。詳細は[エージェントとの接続方法: serve API](/reference/agent-api/)を参照してください。

### 定義への秘密情報の記述の禁止

定義内の名称（id、製品名、ピンのラベル、パーティション名）はDAGON Consoleに表示します。Claudeに依頼した場合はClaudeにも送信します。コマンド、ヒント、コメント、ファイルのパスは送信しません。詳細は[マイコンとボードの定義](/reference/definitions/)を参照してください。

## 3. ノードのcrateの作成

```sh
dagon gen
```

ノードごとに`nodes/<id>/`にcrateを生成します。DAGON Consoleでは「プロジェクト作成」が同一の処理をします。

アプリは`nodes/<id>/src/app.rs`に記述します。記述対象は2つです。

- 本来の処理を実行するtask
- 試行中のバージョンの確定可否を応答する`self_check`

構成の変更後は何度でも`dagon gen`を実行可能です。

- 再生成の対象は`src/wiring.rs`と、`Cargo.toml`にあるDAGONのcrateの行だけです。
- `app.rs`は変更しません。`main.rs`も`--force-main`の指定時以外は変更しません。

`dagon new`と`dagon gen`はシンボリックリンクやジャンクションを経由したファイルの保存と削除をしません。`nodes/<id>`（または`nodes`、`dagon gen`の出力ファイル）がそれに該当するノードは、理由を表示して処理を省略します。その後に`dagon gen`はエラーで終了します。

## 4. ビルド

構成の作図後は、ビルドの開始前に`dagon doctor`がその構成のマイコンに必要なツールを確認し、`dagon doctor --fix`が管理者権限の不要なものをインストールします。

```sh
dagon build            # または: dagon build seg
```

各ノードを`out/build/<id>/app-a.elf`にビルドします。マイコンの定義がアプリにスロットBを割当する場合は、スロットB用の`app-b.elf`もビルドします。ESP32-S3のイメージは両スロットで使用可能なため、1つです。`dagon.toml`の公開鍵を埋め込みます。

### バージョンの決定

ノードのバージョンは、そのcrateの`Cargo.toml`の`version`です。ブートローダのバージョンは、ブート用crateの`Cargo.toml`の`version`です。書込する変更ごとに、この値を増加してください。

- 初回の書込も同一のバージョンで署名します。
- `--version`は不要です。指定する場合は同一の値が必要です。不一致の場合はコマンドが停止し、相違箇所を表示します。
- ビルドはそのcrate自身の`Cargo.lock`の項目だけを、増加後のバージョンに統一します。他のcrateは固定のままです。

### ブートローダ

ノードのブートローダは、アプリと同時にパッケージに格納します。ノードで動作中のブートローダとバージョンが異なる場合だけ書込します。そのためブートローダの変更後はバージョンを増加してください。

`dagon gen`はブート用crateの`version`を上書きしません。このファイルで利用者が決定する項目はこの欄だけです。値の増加後に`dagon gen`の再実行は不要です。

### 不正なブートローダの拒否の確認

独自の検査に失敗するブートローダを作成して確認可能です。

1. `[build.<id>]`に`boot_features = ["no-confirm"]`を追加します。`boot_features`はブート用crateのフィーチャを指定します。`dagon deploy`とDAGON Consoleが、パッケージのブートローダをこのフィーチャでビルドします。初回の書込では使用しません。
2. ブート用crateのバージョンを増加して書込します。書込は停止し、ノードは確定済のブートローダと以前のアプリで動作を継続します。
3. 確認の完了後はこの行を削除してください。削除しない場合、次回の書込でも失敗するブートローダを送信します。

## 5. 最初のバージョンの書込（ノードごとに1回）

ブートローダと最初のアプリを、そのノード専用のケーブルで1回だけ書込します。`DAGON_SIGNING_KEY`を設定し（手順1）、ノードを接続して実行します。

```sh
dagon flash hub        # RP2040: BOOTを押した状態でUSB接続すると、rescue.uf2をRPI-RP2のドライブにコピー
dagon flash seg        # STM32F401RE: ST-LINK経由でprobe-rsが書込
dagon flash watch      # ESP32-S3: USB-C経由でespflashが書込
```

`dagon flash`は最初に手順を表示し、ボードの操作前に確認を要求します。`--yes`で確認を省略します。

### 書込先の選択方法

書込先は書込ツールが検出したデバイス（probe-rsの全プローブ、全シリアルポート、全UF2ドライブ）から選択します。USBのIDやUF2のBoard-IDは表示だけに使用します。候補の絞込はせず、一致した候補を先頭に表示します。

- 候補が1つ: その候補を選択し、確認時に表示します。
- 候補が2つ以上: 番号で選択します。一致した候補があれば、Enterでその候補を選択可能です。
- 明示的な指定: `--probe <vid:pid[:serial]>`、`--port <port>`、`--drive <path>`を使用します。Windowsでは大文字と小文字を区別しないため、`--drive e:`は`E:\`を指します。
- 書込前に停止する条件: 候補が皆無の場合。または候補が2つ以上で、端末が不在（または`--yes`）なのに指定もない場合。

### ビルド対象

`dagon flash`はブートローダ（PC接続ノードは`stage0`も）をプロジェクトのDAGONのソース（`dagon.toml`の`[dagon]`）からビルドします。ノードのアプリも同時にビルドします。いずれも署名のキーの公開鍵を使用します。

### ボードにアクセスしない試行

`--dry-run`は書込の直前までの全処理を実行します。部品とアプリをビルドし、初回の書込のファイルをprovisionのフォルダに作成します。ボードへのアクセスだけをしません。

provisionのフォルダは`dagon.toml`と同一の階層の`out/provision/<id>/`です。`dagon.toml`に`provision_dir`があればその配下です。`--out <dir>`で別の保存先を指定可能です。

### ESP32-S3のノード

`dagon flash`はボードのfactoryエージェントとアプリをビルドします。アプリをESPのイメージに変換して（`espflash save-image --chip esp32s3 --flash-size 16mb`）署名し、`espflash`で以下のものを書込します。

<!-- generated from the ESP32-S3 MCU definition and its partitions.csv: python site/scripts/gen.py -->
- 同梱の無改変のESP-IDF 2nd stage（0x0）: SHA-256が`targets/mcu/esp32s3/vendor/SHA256SUMS`と不一致の場合は書込しません。
- パーティション表（`targets/mcu/esp32s3/partitions.csv`、0x8000）
- factoryエージェント（factory、0x10000）
- アプリ（ota_0、0x110000）
- アプリの署名済ヘッダ（`dagon`パーティション、0x710000）
- 確定済として起動させるotadataの項目（0xD000）
<!-- end of generated table -->

`--dry-run`はファイルとそのオフセット、`espflash`のコマンドを表示します（同一のprovisionフォルダにある`commands.txt`）。

### 応答の確認

PC接続ノードはPCのUSBポートに直接接続します（[制限事項](/explanation/limitations/)）。全ノードの応答を確認します。

```sh
dagon status
```

各ノードの`versions:`の行に、以下の情報を表示します。

- stage0（リセット後に最初に動作する小規模なプログラム）
- ブートローダのスロット（マイコンの定義が割当する1つか2つ）と、動作中のスロット
- アプリのスロット
- 通信のバージョン
- MCUのID

`unknown`はその情報の通知に未対応のノード（通知の機能より以前に作成したエージェントやstage0）です。PC接続ノードが接続済であれば、`dagon doctor`もツールの確認後に同一の行を表示します。

状態を表示する各ノードの行で、`reset`は、起動に先行した直近のリセットの要因です。`power-on`は電源投入か電圧低下、`watchdog`はウォッチドッグ、`software`はファームウェアのリセット要求、`pin`はリセット端子、`unknown`はその他の要因です。アプリが起動時にマイコンのリセットのフラグを読出し、STM32F401ではRCC_CSRのフラグをその後に消去します。次回の起動時は、その後のリセットの要因だけを表示します。要因を通知しないノードの行には、`reset`を表示しません。

### `dagon flash`を使用しない場合

独自にビルドしたブート用のイメージを使用する場合は、`dagon provision <id>`がそのファイルを作成します。入力は`dagon.toml`の`[provision.<id>]`（PC接続ノードは`stage0`、他に`bootcore`、`app`、`version`）です。ブート用のイメージのビルド方法は、マイコンの定義のREADMEにあります。

## 6. 新規バージョンの書込と、必要に応じたロールバック

### コマンドでの書込

コードの変更後は、以下を実行します。

```sh
dagon deploy                       # または: dagon deploy seg watch
```

`dagon deploy`は以下の順序で処理します。

1. マイコンの定義が割当するスロットごとにビルドし、署名します。
2. パッケージを作成します。
3. ノードごとに既存のバージョンと新規バージョンを表示し、書込の開始前に確認を要求します。`--yes`で省略可能です。
4. 書込します。

バージョンの表示内容は以下のとおりです。

- 既存のバージョン: 前回のパッケージ（`out/release-latest.dgb`、または`dagon.toml`の`release`）をそのキーでの検証に成功した場合の、そのパッケージに記録したノードのバージョンです。ボードからの読込ではありません。
- 新規バージョン: 各ノードのcrateのバージョンです。

ノードのidの省略時は、`[build.<id>]`の全ノードが対象です。

ESP32-S3のノードは1回だけビルドします。イメージはどちらのOTAスロットでも動作します。`dagon deploy`がそのイメージをESPのイメージに変換して（`espflash save-image`）署名し、コーディネータが他のノードと同様に中継します。

個別の手順は`dagon build`、`dagon image`、`dagon release`、`dagon apply`です（`dagon --help`）。

### DAGON Consoleでの書込

1. 「準備」で「ビルド」をクリックします。全ノードをビルドして署名し、パッケージを作成します。
2. 「書込」で、カードのチェックにより書込対象のノードを選択します。
3. ヘッダーでバージョンを選択します。最新のビルドか、ロールバック時は保存済のバージョンです。
4. 「書込」をクリックします。Web Serial経由で書込します。

カードには旧→新のバージョンと、書込全体の進捗率を表示します。完了時に結果を表示します。

### 書込時の処理

1. 署名を検証します。
2. パッケージがこの構成用に作成したものであることを確認します。
3. 接続先のノードに書込し、その後にPC接続ノードに書込します。
4. ノードは新規バージョンを試行起動します。未確定の場合はウォッチドッグがリセットし、以前のバージョンで再起動します。アプリのスロットが1つのノードには以前のバージョンがありません。その場合の動作は[制限事項](/explanation/limitations/)にあります。

### 以前のバージョンへのロールバック

ノードへ書込したパッケージ（DAGON Console、`dagon apply`、`dagon deploy`）は、全部プロジェクトの`out/deploy/`に保存します。名称は`v<バージョン>.dgb`です。ノードごとにバージョンが異なる場合は`v<最新のバージョン>-<ダイジェスト>.dgb`です。

ロールバックには保存済の旧バージョンを再度書込します。DAGON Consoleはその一覧を表示します。コマンドでは`dagon apply out/deploy/v1.0.0.dgb`で書込可能です。

`dagon rollback`は以前に確定したバージョンへロールバックします。アプリのスロットが1つのノードには復帰先がなく、`dagon rollback`はその旨を応答します。

```sh
dagon rollback --node seg
```

### 無応答になったノード

`dagon deploy`は無応答になったノードにも使用可能です。

```sh
dagon deploy seg
```

無応答であることを通知し、確認の要求後に（`--yes`で省略）、ROMローダで書込します。ROMローダが不在のマイコンでは`dagon flash`を案内します。

全コマンドとオプションは`dagon --help`で確認可能です。
