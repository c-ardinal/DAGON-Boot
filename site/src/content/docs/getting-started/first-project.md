---
title: "導入から書込まで"
description: "プロジェクトを始め、構成を描き、ビルドし、最初のバージョンを書込し、新しいバージョンを書込後にロールバックします。"
---

このページの例は見本の構成です。丸いLCDのRP2040（PC接続ノード）、NUCLEO-F401RE（I2C）、M5StopWatch（UART）の3つのノードです。手順2で自分の構成に置き換えられます。

接続先のノードで動作するサンプルアプリの内容は`samples/f401-app`と`samples/esp32s3-app`にあります。

`dagon`コマンドとツールは、先に[入れる](/getting-started/install/)の手順で用意します。

## 1. プロジェクトを始める

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new`は次のものを作成します。

- `dagon.toml`
- `topology.yaml`: 見本の構成です。`--from <yaml>`を付けると自分の構成になります。
- `.gitignore`
- Claude Codeのファイル（[プロジェクトでのClaudeの手助け](/how-to/claude-help/)）
- 署名の鍵の組（無いとき）: `~/.dagon/keys/my-fleet`に作成します。`--keys <dir>`で場所を変えられます。

秘密鍵はプロジェクトの外、gitの作業ツリーの外に置かれます。`dagon.toml`には公開鍵の場所だけが書かれます。

署名の鍵をどう守るかは利用者が選びます。DAGONは選んだ置き場所から鍵を受け取るポート（[`KeyStore`](/reference/ports/)）を持ちます。既定は鍵のファイルです。

署名するときは、秘密鍵のファイルの場所を`dagon`に教えます。

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

## 2. 構成を作成する

### DAGON Consoleで作成する

```sh
dagon serve
```

DAGON Consoleがブラウザに開きます。127.0.0.1だけで動き、使い捨てのパスフレーズが付きます。プロジェクトが無いときは、プロジェクトを作成するか開く画面から始まります。

1. ノードリストからノードを置きます。製品でもマイコンでも選べます。
2. ノードをリンクでつなぎます。
3. 検査が示す問題を直します。

編集は`topology.yaml`に保存されます。

### コマンドで確かめる

```sh
dagon topo check topology.yaml
```

### 言葉の説明から作成する

言葉で書いた説明から、Claude APIで構成の案を作成可能です。

```sh
dagon ai key set                    # API キーを入力（表示しない）。OS の資格情報ストアに保存する
dagon ai link --way api             # 送ることへの同意（一度だけ）
dagon ai topology --in boards.txt --out topology.yaml
```

APIキーはOSの資格情報ストアにだけ置きます。環境変数やファイルには置きません。

- Windows: 資格情報マネージャー
- macOS: キーチェーン
- Linux: GNOME KeyringなどのSecret Service

`dagon ai key status`で登録済みかを確かめ、`dagon ai key delete`で削除します。DAGON Consoleの「CLI連携」でも登録できます。

資格情報ストアが無い環境では、自分のログインで使うClaude Codeを選んでください（`dagon ai link`）。詳しくは[エージェントとのつなぎ方: serve API](/reference/agent-api/)を見てください。

### 定義に秘密を書かない

定義の中の名前（id、製品名、ピンのラベル、パーティション名）はDAGON Consoleに表示されます。Claudeに頼んだときはClaudeにも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。詳しくは[マイコンとボードの定義](/reference/definitions/)を見てください。

## 3. ノードのcrateを作成する

```sh
dagon gen
```

ノードごとに`nodes/<id>/`にcrateができます。DAGON Consoleでは「プロジェクト作成」が同じことをします。

アプリは`nodes/<id>/src/app.rs`に書きます。書くものは2つです。

- 仕事をするtask
- 試行中のバージョンを確定してよいかを答える`self_check`

構成を変えたら、何度でも`dagon gen`を実行できます。

- 書き直すのは`src/wiring.rs`と、`Cargo.toml`のDAGONのcrateの行だけです。
- `app.rs`には触れません。`main.rs`にも、`--force-main`のとき以外は触れません。

`dagon new`と`dagon gen`は、シンボリックリンクやジャンクションを通してファイルを保存も削除もしません。`nodes/<id>`（または`nodes`、`dagon gen`が書くファイル）がそれにあたるノードは、理由を示して飛ばします。そのあと`dagon gen`はエラーで終わります。

## 4. ビルドする

構成を描いたら、ビルドの前に`dagon doctor`がその構成のマイコンに要るツールを確かめ、`dagon doctor --fix`が管理者の権限の要らないものを入れます。

```sh
dagon build            # または: dagon build seg
```

各ノードを`out/build/<id>/app-a.elf`にビルドし、マイコンの定義がアプリにスロットBを与えるときは、スロットB用の`app-b.elf`もビルドします。ESP32-S3のイメージは両方のスロットで使えるので1つです。`dagon.toml`の公開鍵が埋め込まれます。

### バージョンを決める

ノードのバージョンは、そのcrateの`Cargo.toml`の`version`です。ブートローダのバージョンは、ブート用crateの`Cargo.toml`の`version`です。書込する変更ごとに、そこで上げてください。

- 最初の書込も同じバージョンで署名します。
- `--version`は要りません。付けた場合は同じ値でなければなりません。違うとコマンドは止まり、どこが違うかを示します。
- ビルドはそのcrate自身の`Cargo.lock`の項目だけを上げたバージョンにそろえます。ほかのcrateは固定したままです。

### ブートローダ

ノードのブートローダは、アプリと一緒にパッケージに入ります。ノードが動かしているブートローダとバージョンが違うときだけ書込されます。そのため、ブートローダを変えたらバージョンを上げてください。

`dagon gen`はブート用crateの`version`を書き換えません。このファイルのうち、あなたが決めるのはこの欄だけです。上げても`dagon gen`をやり直す必要はありません。

### 悪いブートローダを受け入れないことを確かめる

自分の検査に失敗するブートローダを作成して確かめられます。

1. `[build.<id>]`に`boot_features = ["no-confirm"]`を足します。`boot_features`はブート用crateのフィーチャを指定します。`dagon deploy`とDAGON Consoleが、パッケージのブートローダをこのフィーチャでビルドします。最初の書込では使いません。
2. ブート用crateのバージョンを上げて書込します。書込は止まり、ノードは確定したブートローダと元のアプリのまま動きます。
3. 終わったらこの行を消してください。消さないと、次の書込でまた失敗するブートローダが送られます。

## 5. 最初のバージョンを書込する（ノードごとに1回）

ブートローダと最初のアプリを、そのノード自身のケーブルで1回だけ書込します。`DAGON_SIGNING_KEY`を設定し（手順1）、ノードをつないで実行します。

```sh
dagon flash hub        # RP2040: BOOT を押しながら USB をつなぐと、rescue.uf2 を RPI-RP2 のドライブにコピー
dagon flash seg        # STM32F401RE: ST-LINK 経由で probe-rs が書込する
dagon flash watch      # ESP32-S3: USB-C 経由で espflash が書込する
```

`dagon flash`は先に手順を表示し、ボードに触れる前に確認を求めます。`--yes`で確認を省きます。

### 書込先の選び方

書込先は、書込ツールが見つけたデバイス（probe-rsのすべてのプローブ、すべてのシリアルポート、すべてのUF2ドライブ）から選びます。USBのIDやUF2のBoard-IDは表示に使うだけです。候補を絞らず、一致した候補を先頭に並べます。

- 候補が1つ: それを選び、確認のときに示します。
- 候補が2つ以上: 番号で選びます。一致した候補があれば、Enterでそれを選べます。
- 指定する: `--probe <vid:pid[:serial]>`、`--port <port>`、`--drive <path>`を使います。Windowsでは大文字と小文字を区別しないので、`--drive e:`は`E:\`を指します。
- 書込前に止まるとき: 候補が1つも無いとき。または候補が2つ以上で、端末がない（または`--yes`）のに指定もないとき。

### ビルドされるもの

`dagon flash`は、ブートローダ（PC接続ノードは`stage0`も）をプロジェクトのDAGONのソース（`dagon.toml`の`[dagon]`）からビルドします。ノードのアプリも一緒にビルドします。どれも署名の鍵の公開側を使います。

### ボードに触れずに試す

`--dry-run`は、書込の手前まですべて行います。部品とアプリをビルドし、最初の書込のファイルをprovisionのフォルダに作成します。ボードにだけ触れません。

provisionのフォルダは`dagon.toml`の横の`out/provision/<id>/`です。`dagon.toml`に`provision_dir`があればその下です。`--out <dir>`で別の場所にできます。

### ESP32-S3のノード

`dagon flash`は、ボードのfactoryエージェントとアプリをビルドします。アプリをESPのイメージにして（`espflash save-image --chip esp32s3 --flash-size 16mb`）署名し、`espflash`で次のものを書込します。

- 同梱の無改変のESP-IDF 2nd stage（0x0）: SHA-256が`targets/mcu/esp32s3/vendor/SHA256SUMS`と一致しなければ書込しません。
- パーティション表（`targets/mcu/esp32s3/partitions.csv`、0x8000）
- factoryエージェント
- アプリ（ota_0、0x110000）
- アプリの署名済みヘッダ（`dagon`パーティション、0x710000）
- 確定済みとして起動させるotadataの項目（0xD000）

`--dry-run`は、ファイルとそのオフセット、`espflash`のコマンドを示します（同じprovisionのフォルダの`commands.txt`）。

### 応答を確かめる

PC接続ノードはPCのUSBポートに直接つなぎます（[制限事項](/explanation/limitations/)）。すべてのノードが応答するか確かめます。

```sh
dagon status
```

各ノードの`versions:`の行に、次のものが出ます。

- stage0（リセットの後で最初に動く小さなプログラム）
- ブートローダのスロット（マイコンの定義が与える1つか2つ）と、動いている方
- アプリのスロット
- 通信のバージョン
- MCUのID

`unknown`は、それを伝えられないノード（伝えるようになる前に作成されたエージェントやstage0）です。PC接続ノードをつないでいれば、`dagon doctor`もツールの確認のあとに同じ行を出します。

### `dagon flash`を使わないとき

自分でビルドしたブート用のイメージを使うときは、`dagon provision <id>`がそのファイルを作成します。元になるのは`dagon.toml`の`[provision.<id>]`（PC接続ノードは`stage0`、ほかに`bootcore`、`app`、`version`）です。ブート用のイメージのビルドは、マイコンの定義のREADMEにあります。

## 6. 新しいバージョンを書込し、必要なら戻す

### コマンドで書込する

コードを変えたら、次を実行します。

```sh
dagon deploy                       # または: dagon deploy seg watch
```

`dagon deploy`は次の順に進みます。

1. マイコンの定義が与えるスロットごとにビルドし、署名します。
2. パッケージを作成します。
3. ノードごとに今のバージョンと新しいバージョンを並べ、書込の前に確認を求めます。`--yes`で省けます。
4. 書込します。

バージョンの表示はこう決まります。

- 今のバージョン: 前回のパッケージ（`out/release-latest.dgb`、または`dagon.toml`の`release`）がその鍵で検証できたときの、そのパッケージにあるノードのバージョンです。ボードから読むのではありません。
- 新しいバージョン: 各ノードのcrateのバージョンです。

ノードのidを省くと、`[build.<id>]`のすべてのノードが対象です。

ESP32-S3のノードは1回だけビルドします。イメージはどちらのOTAスロットでも動きます。`dagon deploy`がそれをESPのイメージにして（`espflash save-image`）署名し、コーディネータがほかのノードと同じように中継します。

1つずつの手順は`dagon build`、`dagon image`、`dagon release`、`dagon apply`です（`dagon --help`）。

### DAGON Consoleで書込する

1. 「準備」で「ビルドする」を押します。全ノードをビルドして署名し、パッケージを作成します。
2. 「書込」で、カードのチェックで書込するノードを選びます。
3. ヘッダーでバージョンを選びます。最新のビルドか、前に戻すときは保存済みのものです。
4. 「書込」を押します。Web Serialから書込します。

カードには今 → 新のバージョンと、書込全体の%が出ます。終わると結果が出ます。

### 書込で起きること

1. 署名を確かめます。
2. パッケージがこの構成に対して作成されたことを確かめます。
3. 接続先のノードに書込し、その後にPC接続ノードに書込します。
4. ノードは新しいバージョンを試行起動します。確定されなければ、ウォッチドッグでリセットされ、前のバージョンで起動し直します。アプリのスロットが1つのノードには前のバージョンが無く、どうなるかは[制限事項](/explanation/limitations/)にあります。

### 前のバージョンに戻す

ノードへ書込したパッケージ（DAGON Console、`dagon apply`、`dagon deploy`）は、すべてプロジェクトの`out/deploy/`に保存されます。名前は`v<バージョン>.dgb`です。ノードごとにバージョンが違うときは`v<いちばん新しいバージョン>-<ダイジェスト>.dgb`です。

戻すには、保存された古いバージョンをもう一度書込します。DAGON Consoleはその一覧を出します。コマンドでは`dagon apply out/deploy/v1.0.0.dgb`で書込可能です。

`dagon rollback`は、前に確定したバージョンへ戻します。アプリのスロットが1つのノードには戻る先が無く、`dagon rollback`はそう答えます。

```sh
dagon rollback --node seg
```

### 応答しなくなったノード

`dagon deploy`は、応答しなくなったノードにも使えます。

```sh
dagon deploy seg
```

応答しないことを伝え、確認を求めてから（`--yes`で省略）、ROMローダで書込します。ROMローダが無いマイコンでは`dagon flash`を案内します。

すべてのコマンドとオプションは`dagon --help`で確認できます。
