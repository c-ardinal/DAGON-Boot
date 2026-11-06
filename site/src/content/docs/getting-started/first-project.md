---
title: "はじめから書き込みまで"
description: "プロジェクトを始め、構成を描き、ビルドし、最初のバージョンを書き込み、新しいバージョンを書き込んで戻します。"
---

このページの例は見本の構成です。丸い LCD の RP2040 親機、NUCLEO-F401RE（I2C）、M5StopWatch（UART）の 3 つのノードです。手順 2 で自分の構成に置き換えられます。

子機のサンプルのアプリの動きは `samples/f401-app` と `samples/esp32s3-app` にあります。

`dagon` コマンドとツールは、先に[入れる](/getting-started/install/)の手順で用意します。

## 1. プロジェクトを始める

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new` は次のものを作ります。

- `dagon.toml`
- `topology.yaml`: 見本の構成です。`--from <yaml>` を付けると自分の構成になります。
- `.gitignore`
- Claude Code のファイル（[プロジェクトでの Claude の手助け](/how-to/claude-help/)）
- 署名の鍵の組（まだ無いとき）: `~/.dagon/keys/my-fleet` に作ります。`--keys <dir>` で場所を変えられます。

秘密鍵はプロジェクトの外、git の作業ツリーの外に置かれます。`dagon.toml` には公開鍵の場所だけが書かれます。

署名するときは、秘密鍵の場所を `dagon` に教えます。

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

## 2. 構成を作る

### DAGON Console で作る

```sh
dagon serve
```

DAGON Console がブラウザに開きます。127.0.0.1 だけで動き、使い捨てのパスフレーズが付きます。プロジェクトが無いときは、作るか開く画面から始まります。

1. ノードリストからノードを置きます。製品でもマイコンでも選べます。
2. ノードをリンクでつなぎます。
3. 検査が示す問題を直します。

編集は `topology.yaml` に保存されます。

### コマンドで確かめる

```sh
dagon topo check topology.yaml
```

### 言葉の説明から作る

言葉で書いた説明から、Claude API で構成の案を作れます。

```sh
dagon ai key set                    # API キーを入力（表示しない）。OS の資格情報ストアに保存する
dagon ai link --way api             # 送ることへの同意（一度だけ）
dagon ai topology --in boards.txt --out topology.yaml
```

API キーは OS の資格情報ストアにだけ置きます。環境変数やファイルには置きません。

- Windows: 資格情報マネージャー
- macOS: キーチェーン
- Linux: GNOME Keyring などの Secret Service

`dagon ai key status` で登録済みかを確かめ、`dagon ai key delete` で削除します。DAGON Console の「CLI 連携」でも登録できます。

資格情報ストアが無い環境では、自分のログインで使う Claude Code を選んでください（`dagon ai link`）。詳しくは[エージェントとのつなぎ方: serve API](/reference/agent-api/)を見てください。

### 定義に秘密を書かない

定義の中の名前（id、製品名、ピンのラベル、パーティション名）は DAGON Console に表示されます。Claude に頼んだときは Claude にも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。詳しくは[マイコンとボードの定義](/reference/definitions/)を見てください。

## 3. ノードの crate を作る

```sh
dagon gen
```

ノードごとに `nodes/<id>/` に crate ができます。DAGON Console では「プロジェクトを作る」が同じことをします。

アプリは `nodes/<id>/src/app.rs` に書きます。書くものは 2 つです。

- 仕事をする task
- 試行中のバージョンを確定してよいかを答える `self_check`

構成を変えたら、何度でも `dagon gen` を実行できます。

- 書き直すのは `src/wiring.rs` と、`Cargo.toml` の DAGON の crate の行だけです。
- `app.rs` には触れません。`main.rs` にも、`--force-main` のとき以外は触れません。

`dagon new` と `dagon gen` は、シンボリックリンクやジャンクションを通して書き込みも削除もしません。`nodes/<id>`（または `nodes`、`dagon gen` が書くファイル）がそれにあたるノードは、理由を示して飛ばします。そのあと `dagon gen` はエラーで終わります。

## 4. ビルドする

```sh
dagon build            # または: dagon build seg
```

各ノードを `out/build/<id>/app-a.elf` と、スロット B 用の `app-b.elf` にビルドします。ESP32-S3 のイメージは両方のスロットで使えるので 1 つです。`dagon.toml` の公開鍵が埋め込まれます。

### バージョンを決める

ノードのバージョンは、その crate の `Cargo.toml` の `version` です。ブートローダのバージョンは、ブート用 crate の `Cargo.toml` の `version` です。書き込む変更ごとに、そこで上げてください。

- 最初の書き込みも同じバージョンで署名します。
- `--version` は要りません。書いた場合は同じ値でなければなりません。違うとコマンドは止まり、どこが違うかを示します。
- ビルドは、その crate 自身の `Cargo.lock` の項目だけを上げたバージョンにそろえます。ほかの crate は固定したままです。

### ブートローダ

ノードのブートローダは、アプリと一緒にパッケージに入ります。ノードが動かしているブートローダとバージョンが違うときだけ書き込まれます。そのため、ブートローダを変えたらバージョンを上げてください。

`dagon gen` はブート用 crate の `version` を書き換えません。このファイルのうち、あなたが決めるのはこの欄だけです。上げても `dagon gen` をやり直す必要はありません。

### 悪いブートローダを受け入れないことを確かめる

自分の検査に失敗するブートローダを作って確かめられます。

1. `[build.<id>]` に `boot_features = ["no-confirm"]` を足します。`boot_features` はブート用 crate のフィーチャを指定します。`dagon deploy` と DAGON Console が、パッケージのブートローダをこのフィーチャでビルドします。最初の書き込みでは使いません。
2. ブート用 crate のバージョンを上げて書き込みます。書き込みは止まり、ノードは確定したブートローダと元のアプリのまま動きます。
3. 終わったらこの行を消してください。消さないと、次の書き込みでまた失敗するブートローダが送られます。

## 5. 最初のバージョンを書き込む（ノードごとに 1 回）

ブートローダと最初のアプリを、そのノード自身のケーブルで 1 回だけ書き込みます。`DAGON_SIGNING_KEY` を設定し（手順 1）、ノードをつないで実行します。

```sh
dagon flash hub        # RP2040: BOOT を押しながら USB をつなぐと、rescue.uf2 を RPI-RP2 のドライブにコピー
dagon flash seg        # STM32F401RE: ST-LINK 経由で probe-rs が書き込む
dagon flash watch      # ESP32-S3: USB-C 経由で espflash が書き込む
```

`dagon flash` は先に手順を表示し、ボードに触れる前に確認を求めます。`--yes` で確認を省きます。

### 書き込み先の選び方

書き込み先は、書き込みツールが見つけたデバイス（probe-rs のすべてのプローブ、すべてのシリアルポート、すべての UF2 ドライブ）から選びます。USB の ID や UF2 の Board-ID は表示に使うだけです。候補を絞らず、一致した候補を先頭に並べます。

- 候補が 1 つ: それを選び、確認のときに示します。
- 候補が 2 つ以上: 番号で選びます。一致した候補があれば、Enter でそれを選べます。
- 指定する: `--probe <vid:pid[:serial]>`、`--port <port>`、`--drive <path>` を使います。Windows では大文字と小文字を区別しないので、`--drive e:` は `E:\` を指します。
- 書き込む前に止まるとき: 候補が 1 つも無いとき。または候補が 2 つ以上で、端末がない（または `--yes`）のに指定もないとき。

### ビルドされるもの

`dagon flash` は、ブートローダ（親機は `stage0` も）をプロジェクトの DAGON のソース（`dagon.toml` の `[dagon]`）からビルドします。ノードのアプリも一緒にビルドします。どれも署名の鍵の公開側を使います。

ブートローダは見本の構成の tag（`hub-rp2040-lcd`、`seg-f401-7seg`）を受け付けます。これらのボードでは `topology.yaml` の tag をそのままにします。

### ボードに触れずに試す

`--dry-run` は、書き込みの手前まですべて行います。部品とアプリをビルドし、最初の書き込みのファイルを provision のフォルダに作ります。ボードにだけ触れません。

provision のフォルダは `dagon.toml` の横の `out/provision/<id>/` です。`dagon.toml` に `provision_dir` があればその下です。`--out <dir>` で別の場所にできます。

### ESP32-S3 のノード

`dagon flash` は、ボードの factory エージェントとアプリをビルドします。アプリを ESP のイメージにして（`espflash save-image --chip esp32s3 --flash-size 16mb`）署名し、`espflash` で次のものを書き込みます。

- 同梱の無改変の ESP-IDF 2nd stage（0x0）: SHA-256 が `targets/mcu/esp32s3/vendor/SHA256SUMS` と一致しなければ書き込みません。
- パーティション表（`targets/mcu/esp32s3/partitions.csv`、0x8000）
- factory エージェント
- アプリ（ota_0、0x110000）
- アプリの署名済みヘッダ（`dagon` パーティション、0x710000）
- 確定済みとして起動させる otadata の項目（0xD000）

`--dry-run` は、ファイルとそのオフセット、`espflash` のコマンドを示します（同じ provision のフォルダの `commands.txt`）。

### 応答を確かめる

親機をつなぎ、すべてのノードが応答するか確かめます。

```sh
dagon status
```

各ノードの `versions:` の行に、次のものが出ます。

- stage0（リセットの後で最初に動く小さなプログラム）
- ブートローダのスロット A/B と、動いている方
- アプリのスロット
- 通信のバージョン
- MCU の ID

`unknown` は、それを伝えられないノード（伝える前に作られたエージェントや stage0）です。親機をつないでいれば、`dagon doctor` もツールの確認のあとに同じ行を出します。

### `dagon flash` を使わないとき

自分でビルドしたブート用のイメージを使うときは、`dagon rescue hub` と `dagon provision <id>` が同じファイルを作ります。元になるのは `dagon.toml` の `[provision.<id>]`（親機は `stage0`、ほかに `bootcore`、`app`、`version`）です。ブート用のイメージのビルドは、マイコンの定義の README にあります。

## 6. 新しいバージョンを書き込み、必要なら戻す

### コマンドで書き込む

コードを変えたら、次を実行します。

```sh
dagon deploy                       # または: dagon deploy seg watch
```

`dagon deploy` は次の順に進みます。

1. スロット A と B をビルドし、署名します。
2. パッケージを作ります。
3. ノードごとに今のバージョンと新しいバージョンを並べ、書き込みの前に確認を求めます。`--yes` で省けます。
4. 書き込みます。

バージョンの表示はこう決まります。

- 今のバージョン: 前回のパッケージ（`out/release-latest.dgb`、または `dagon.toml` の `release`）がその鍵で検証できたときの、そのパッケージにあるノードのバージョンです。ボードから読むのではありません。
- 新しいバージョン: 各ノードの crate のバージョンです。

ノードの id を省くと、`[build.<id>]` のすべてのノードが対象です。

ESP32-S3 のノードは 1 回だけビルドします。イメージはどちらの OTA スロットでも動きます。`dagon deploy` がそれを ESP のイメージにして（`espflash save-image`）署名し、親機がほかのノードと同じように中継します。

1 つずつの手順は `dagon build`、`dagon image sign`、`dagon release build`、`dagon apply` です（`dagon --help`）。

### DAGON Console で書き込む

1. 「準備」で「ビルドする」を押します。全ノードをビルドして署名し、パッケージを作ります。
2. 「書込」で、カードのチェックで書き込むノードを選びます。
3. ヘッダーでバージョンを選びます。最新のビルドか、前に戻すときは保存済みのものです。
4. 「書き込む」を押します。Web Serial から書き込みます。

カードには今 → 新のバージョンと、書き込み全体の % が出ます。終わると結果が出ます。

### 書き込みで起きること

1. 署名を確かめます。
2. パッケージがこの構成に対して作られたことを確かめます。
3. 構成を親機へ送ります。
4. 子機に書き込み、そのあと親機に書き込みます。
5. ノードは新しいバージョンを試行起動します。確定されなければ、ウォッチドッグでリセットされ、前のバージョンで起動し直します。

### 前のバージョンに戻す

ノードへ書き込んだパッケージ（DAGON Console、`dagon apply`、`dagon deploy`）は、すべてプロジェクトの `out/deploy/` に保存されます。名前は `v<バージョン>.dgb` です。ノードごとにバージョンが違うときは `v<いちばん新しいバージョン>-<ダイジェスト>.dgb` です。

戻すには、保存された古いバージョンをもう一度書き込みます。DAGON Console はその一覧を出します。コマンドでは `dagon apply out/deploy/v1.0.0.dgb` で書き込めます。

`dagon rollback` は、前に確定したバージョンへ戻します。

```sh
dagon rollback --node seg
```

### 応答しなくなったノード

`dagon deploy` は、応答しなくなったノードにも使えます。

```sh
dagon deploy seg
```

そのことを伝えて確かめてから（`--yes` で省略）、ROM ローダで書き込みます。ROM ローダが無いマイコンでは `dagon flash` を案内します。

すべてのコマンドとオプションは `dagon --help` で確認できます。
