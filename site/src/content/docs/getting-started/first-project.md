---
title: "はじめから書き込みまで"
description: "プロジェクトを始め、構成を描き、ビルドし、最初のバージョンを書き込み、新しいバージョンを書き込んで戻します。"
---

例は見本の構成です: 丸い LCD の RP2040 親機、NUCLEO-F401RE（I2C）、M5StopWatch（UART）。子機のサンプルのアプリの動きは `samples/f401-app` と `samples/esp32s3-app` にあります。手順 2 で自分の構成に置き換えます。

## 1. プロジェクトを始める

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new` は `dagon.toml`、`topology.yaml`（見本の構成。`--from <yaml>` で自分の構成）、`.gitignore`、Claude Code のファイル（下）を作り、まだ無ければ署名の鍵の組を `~/.dagon/keys/my-fleet`（または `--keys <dir>`）に作ります。秘密鍵はプロジェクトの外、git の作業ツリーの外に置かれ、`dagon.toml` には公開鍵の場所だけが書かれます。署名するときは秘密鍵の場所を CLI に教えます:

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

## 2. DAGON Console で構成を作る

```sh
dagon serve
```

DAGON Console が 127.0.0.1 だけで、使い捨てのパスフレーズ付きでブラウザに開きます。ノードリストからノードを置き（製品でもマイコンでも選べます）、リンクでつなぎ、検査が示す問題を直します。編集は `topology.yaml` に保存されます。プロジェクトが無いときは、作るか開く画面から始まります。

コマンドでは `dagon topo check topology.yaml` で検査します。言葉で書いた説明から Claude API で構成の案を作ることもできます:

```sh
dagon ai key set                    # API キーを入力（表示しない）。OS の資格情報ストアに保存する
dagon ai link --way api             # 送ることへの同意（一度だけ）
dagon ai topology --in boards.txt --out topology.yaml
```

API キーは OS の資格情報ストア（Windows の資格情報マネージャー、macOS のキーチェーン、Linux では GNOME Keyring などの Secret Service）にだけ置き、環境変数やファイルには置きません。`dagon ai key status` で登録済みかを確かめ、`dagon ai key delete` で削除します。DAGON Console の「CLI 連携」でも登録できます。資格情報ストアが無い環境では、自分のログインで使う Claude Code を選んでください（`dagon ai link`）。詳しくは [Agent link: serve API](/reference/agent-api/) を見てください。

定義ファイルは秘密を書く場所ではありません。定義の中の名前（id、製品名、ピンのラベル、パーティション名）は DAGON Console に表示され、Claude に頼んだときは Claude にも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。詳しくは [MCU and board definitions](/reference/definitions/) を見てください。

## 3. ノードの crate を作る

```sh
dagon gen
```

ノードごとに `nodes/<id>/` に crate ができます。アプリは `nodes/<id>/src/app.rs` に書きます: 仕事をする task と、試行中のバージョンを確定してよいかを答える `self_check` です。構成を変えたら何度でも `dagon gen` を実行できます。書き直すのは `src/wiring.rs` と `Cargo.toml` の DAGON の crate の行だけで、`app.rs` には触れません（`main.rs` も `--force-main` のときだけ）。DAGON Console では『プロジェクトを作る』が同じことをします。

## 4. ビルド

```sh
dagon build            # または: dagon build seg
```

各ノードを `out/build/<id>/app-a.elf` と、スロット B 用の `app-b.elf` にビルドします（ESP32-S3 のイメージは両方のスロットで使えるので 1 つ）。`dagon.toml` の公開鍵が埋め込まれます。ノードのバージョンは、そのクレートの `Cargo.toml` の `version` です（ブートローダはブート用クレートの `Cargo.toml`）。書き込む変更ごとに、そこで上げてください。初回の書き込みも同じバージョンで署名します。`--version` は要りません。書いた場合は同じでなければならず、違うとコマンドは止まり、どこが違うかを示します。ノードのブートローダはアプリと一緒にパッケージに入り、ノードが動かしているブートローダとバージョンが違うときだけ書き込まれます。アプリと同じく、ブートローダを変えたらバージョンを上げてください。ビルドは、そのクレート自身の `Cargo.lock` の項目だけを上げたバージョンにそろえ、ほかの crate は固定したままです。`dagon gen` はブート用クレートの `version` を書き換えません。このファイルのうち、あなたが決めるのはこの欄だけです。

ブートローダの `version` は上げても `dagon gen` をやり直す必要はありません。悪いブートローダをノードが受け入れないことを確かめるには、自分の検査に失敗するブートローダを作ります。`[build.<id>]` に `boot_features = ["no-confirm"]` を足すと、リリースのブートローダがブート用クレートの `no-confirm` フィーチャでビルドされます（`boot_features` はブート用クレートのフィーチャを指定します。最初の書き込みでは使いません）。バージョンを上げて書き込むと書き込みは止まり、ノードは確定したブートローダと元のアプリのまま動きます。終わったらこの行を消してください。消さないと、次の書き込みでまた失敗するブートローダが送られます。

## 5. 最初のバージョンを書き込む（ノードごとに 1 回）

ノードには、ブートローダと最初のアプリを、そのノード自身のケーブルで 1 回だけ書き込みます。`DAGON_SIGNING_KEY` を設定し（手順 1）、ノードをつないで実行します:

```sh
dagon flash hub        # RP2040: BOOT を押しながら USB をつなぐと、rescue.uf2 を RPI-RP2 のドライブにコピー
dagon flash seg        # STM32F401RE: ST-LINK 経由で probe-rs が書き込む
dagon flash watch      # ESP32-S3: USB-C 経由で espflash が書き込む
```

`dagon flash` は先に手順を表示し、ボードに触れる前に確認を求めます（`--yes` で確認を省きます）。書き込み先は書き込みツールが見つけたデバイス（probe-rs のすべてのプローブ、すべてのシリアルポート、すべての UF2 ドライブ）から選びます。USB の ID や UF2 の Board-ID は表示に使うだけで、候補を絞らず、一致した候補を先頭に並べます。1 つならそれを選んで確認のときに示し、2 つ以上なら番号で選ぶ（一致した候補があれば Enter でそれを選べる）か `--probe <vid:pid[:serial]>`・`--port <port>`・`--drive <path>` で指定します（Windows では大文字と小文字を区別せず、`--drive e:` は `E:\` を指します）。2 つ以上で端末がない（または `--yes`）のに指定がないとき、または 1 つもないときは書く前に止まります。`--dry-run` も書き込みの手前まではすべて行います: 部品とアプリをビルドし、初回書込みのファイルを provision のフォルダ（`dagon.toml` の横の `out/provision/<id>/`、`dagon.toml` に `provision_dir` があればその下、`--out <dir>` で別の場所）に作り、ボードにだけ触れません。ブートローダ（親機は `stage0` も）を CLI をビルドした DAGON のクローン（または `DAGON_HOME`）から、ノードのアプリと一緒に、署名の鍵の公開側でビルドします。ブートローダは見本の構成の tag（`hub-rp2040-lcd`、`seg-f401-7seg`）を受け付けるので、これらのボードでは `topology.yaml` の tag をそのままにします。

ESP32-S3 のノードでは、`dagon flash` がボードの factory エージェントとアプリをビルドし、アプリを ESP のイメージにして（`espflash save-image --chip esp32s3 --flash-size 16mb`）署名し、`espflash` で書き込みます: 同梱の無改変の ESP-IDF 2nd stage を 0x0 に（SHA-256 が `targets/mcu/esp32s3/vendor/SHA256SUMS` と一致しなければ書きません）、パーティション表（`targets/mcu/esp32s3/partitions.csv`、0x8000）、factory エージェント、ota_0（0x110000）にアプリ、`dagon` パーティション（0x710000）にその署名済みヘッダ、確定済みとして起動させる otadata の項目（0xD000）。`--dry-run` はファイルとそのオフセット、`espflash` のコマンドを示します（同じ provision のフォルダの `commands.txt`）。

そのあと親機をつなぎ、すべてのノードが応答するか確かめます:

```sh
dagon status
```

各ノードの `versions:` の行に、stage0（リセットの後で最初に動く小さなプログラム）、ブートローダのスロット A/B（と動いている方）、アプリのスロット、通信のバージョン、MCU の ID が出ます。`unknown` は、それを伝えられないノード（伝える前に作られたエージェントや stage0）です。親機をつないでいれば、`dagon doctor` もツールの確認のあとに同じ行を出します。

`dagon flash` を使わない場合（自分でビルドしたブート用のイメージを使うとき）: `dagon rescue hub` と `dagon provision <id>` が、`dagon.toml` の `[provision.<id>]`（親機は `stage0`、ほかに `bootcore`、`app`、`version`）から同じファイルを作ります。ブート用のイメージのビルドは マイコンの定義の README にあります。

## 6. 新しいバージョンを書き込み、必要なら戻す

コードを変えたら:

```sh
dagon deploy                       # または: dagon deploy seg watch
```

`dagon deploy` はスロット A と B をビルドし、署名し、リリースを作り、ノードごとに今のバージョンと新しいバージョンを並べて、書き込みの前に確認を求めます（`--yes` で省けます）。今のバージョンは、前回のリリース（`out/release-latest.dgb`、または `dagon.toml` の `release`）がその鍵で検証できたときに、そのリリースにあるノードのバージョンです。ボードから読むのではありません。新しいバージョンは、各ノードのクレートのバージョンです。ノードの id を省くと `[build.<id>]` のすべてのノードが対象です。ESP32-S3 のノードは 1 回だけビルドし（イメージはどちらの OTA スロットでも動く）、deploy がそれを ESP のイメージにして（`espflash save-image`）署名し、親機がほかのノードと同じように中継します。1 つずつの手順は `dagon build`、`dagon image sign`、`dagon release build`、`dagon apply` です（`dagon --help`）。DAGON Console では、準備の「ビルドする」が全ノードをビルドして署名し、パッケージを作ります。書込では、カードのチェックで書き込むノードを選び、ヘッダーでバージョン（最新のビルド、または前に戻すときは保存済みのもの）を選んで「書き込む」と、Web Serial から書き込みます。カードには今 → 新のバージョンと書き込み全体の % が出て、終わると結果が出ます。

ノードへ書き込んだパッケージ（Console、`dagon apply`、`dagon deploy`）は、すべてプロジェクトの `out/deploy/` に `v<バージョン>.dgb` として保存されます（ノードごとにバージョンが違うときは `v<いちばん新しいバージョン>-<ダイジェスト>.dgb`）。前のバージョンに戻すには、保存された古いバージョンをもう一度書き込みます。Console はその一覧を出し、コマンドでは `dagon apply out/deploy/v1.0.0.dgb` で書き込めます。

書き込みでは、署名と、リリースがこの構成に対して作られたことを確かめ、構成を親機へ送り、子機に書き込んでから親機に書き込みます。ノードは新しいバージョンを試行起動し、確定されなければウォッチドッグでリセットされて前のバージョンで起動し直します。

```sh
dagon rollback --node seg     # 前に確定したバージョンへ戻す
dagon deploy seg              # 応答しなくなったノードも。そう伝えて確かめてから（`--yes` で省略）ROM ローダで書き換える（ROM がなければ `dagon flash` を案内）
```

すべてのコマンドとオプションは `dagon --help` で確認できます。
