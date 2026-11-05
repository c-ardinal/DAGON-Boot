<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/product-symbol/lockup-horizontal-dark-2400w.png">
    <img src="assets/product-symbol/lockup-horizontal-light-2400w.png" alt="DAGON Boot" width="480">
  </picture>
</p>

# DAGON Boot (DIY Assured-update Gateway for Own Nodes)

[English: README.md](README.md)

**動くハードウェアを簡単に作り、作ったあとも育て続ける。**

DAGON Boot は、いくつかのマイコンを組み合わせて何かを作り、机や壁や腕に置いたあとも良くしていきたい開発者（はじめての方から経験者まで）のためのものです。ボード（またはそのマイコン）を選び、つなぎ方を描き、アプリを Rust で書くだけで、残りは DAGON が受け持ちます。

- **ビルドできるプロジェクト。** `dagon new` と `dagon gen` が、ノードごとに起動・配線・自分のコードを書く空の `app.rs` を持つ Rust の crate を作ります。フラッシュの配置、イメージヘッダ、リンカの番地、ブートローダは DAGON が決めます。
- **全体を安全に更新。** PC の USB につないだ親機が、各ノードへ新しいバージョンを送ります。イメージはすべて署名付きです。ノードは新しいバージョンを試行起動し、確定されなければ自分で前のバージョンに戻ります。応答しなくなったノードも親機経由で書き直せます。
- **同じことができるブラウザのツールとコマンド。** DAGON Console（`dagon serve`）は、ノードとリンクをグラフで見せ、編集のたびに検査し、ビルド・署名・更新まで行います。どの手順も `dagon` コマンドでもできます。
- **Claude の手助け。** 新しいプロジェクトには、よくある場面（ビルドが失敗した、一覧に無いボードを使いたい、構成の問題が分からない）のための Claude Code の指示とスキルが入っています。

## この文書の言葉

- **ノード**: 構成の中の 1 つのマイコン。
- **親機**: PC と USB でつながるノード。ほかのノードへ更新を中継します。
- **子機**: 親機から I2C か UART でつながるノード。
- **構成**: ノード、そのリンク、更新の順番（`topology.yaml`）。

## 対応するハードウェア

| マイコン | デスクのデモでの役割 | 対応表のボード | 要るフラッシュ |
|---|---|---|---|
| RP2040（`rp2040`） | 親機 | Waveshare RP2040-Touch-LCD-1.28（`waveshare-rp2040-touch-lcd-1.28`） | 4 MiB |
| STM32F401RE（`stm32f401re`） | 子機（I2C） | NUCLEO-F401RE（`nucleo-f401re`） | 512 KiB |
| ESP32-S3（`esp32s3`） | 子機（UART） | M5StopWatch（`m5stopwatch`） | 16 MiB |

**ボードでもマイコンでも選べます。** 対応表のボードは、ピン・既定のリンク・付属の部品を持っているので、選ぶだけです。同じマイコンを載せたほかのボードはマイコンで選び、フラッシュ容量と親機につないだピンを入れます（[自分のボード（マイコンで選ぶ）](#自分のボードマイコンで選ぶ)を参照）。

## 必要なもの

| ツール | 要る場面 | 入れ方 |
|---|---|---|
| Rust（`rustup`） | いつも | <https://rustup.rs> |
| Rust のターゲット `thumbv6m-none-eabi`、`thumbv7em-none-eabihf` | RP2040、STM32F401RE | `rustup target add thumbv6m-none-eabi thumbv7em-none-eabihf` |
| `espup` と `esp` のツールチェーン | ESP32-S3 | `cargo install espup` のあと `espup install` |
| `probe-rs` と ST-LINK | STM32F401RE の初回書込み | `cargo install probe-rs-tools`（Windows は ST-LINK のドライバも） |
| `espflash` | ESP32-S3 の初回書込み | `cargo install espflash` |
| C コンパイラ | `dagon` CLI をソースからビルドするとき（`ring` crate） | Windows は MSVC の Build Tools、ほかは `cc` |
| Chrome か Edge 89 以降 | DAGON Console（Web Serial） | — |

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

## はじめから更新まで

例はデスクのデモです: 丸い LCD の RP2040 親機、NUCLEO-F401RE（I2C）、M5StopWatch（UART）。子機のサンプルのアプリの動きは `samples/f401-app` と `samples/esp32s3-app` にあります。手順 2 で自分の構成に置き換えます。

### 1. プロジェクトを始める

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new` は `dagon.toml`、`topology.yaml`（デスクのデモ。`--from <yaml>` で自分の構成）、`.gitignore`、Claude Code のファイル（下）を作り、まだ無ければ署名の鍵の組を `~/.dagon/keys/my-fleet`（または `--keys <dir>`）に作ります。秘密鍵はプロジェクトの外、git の作業ツリーの外に置かれ、`dagon.toml` には公開鍵の場所だけが書かれます。署名するときは秘密鍵の場所を CLI に教えます:

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

### 2. DAGON Console で構成を作る

```sh
dagon serve
```

DAGON Console が 127.0.0.1 だけで、使い捨てのパスフレーズ付きでブラウザに開きます。ノードリストからノードを置き（製品でもマイコンでも選べます）、リンクでつなぎ、検査が示す問題を直します。編集は `topology.yaml` に保存されます。プロジェクトが無いときは、作るか開く画面から始まります。

コマンドでは `dagon topo check topology.yaml` で検査します。言葉で書いた説明から Claude API で構成の案を作ることもできます:

```sh
export ANTHROPIC_API_KEY=...        # 読むのは CLI だけ。表示も保存もしない
dagon ai topology --in boards.txt --out topology.yaml
```

定義ファイルは秘密を書く場所ではありません。定義の中の名前（id、製品名、ピンのラベル、パーティション名）は DAGON Console に表示され、Claude に頼んだときは Claude にも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。詳しくは `targets/README.md` を見てください。

### 3. ノードの crate を作る

```sh
dagon gen
```

ノードごとに `nodes/<id>/` に crate ができます。アプリは `nodes/<id>/src/app.rs` に書きます: 仕事をする task と、試行中のバージョンを確定してよいかを答える `self_check` です。構成を変えたら何度でも `dagon gen` を実行できます。書き直すのは `src/wiring.rs` と `Cargo.toml` の DAGON のパッケージの行だけで、`app.rs` には触れません（`main.rs` も `--force-main` のときだけ）。DAGON Console では『プロジェクトを作る』が同じことをします。

### 4. ビルド

```sh
dagon build            # または: dagon build seg
```

各ノードを `out/build/<id>/app-a.elf` と、スロット B 用の `app-b.elf` にビルドします（ESP32-S3 のイメージは両方のスロットで使えるので 1 つ）。`dagon.toml` の公開鍵が埋め込まれます。ノードのバージョンは、そのクレートの `Cargo.toml` の `version` です（ブートコアはブート用クレートの `Cargo.toml`）。書き込む変更ごとに、そこで上げてください。`[build.<id>]` の `version` や `--version` は要りません。書いた場合は同じでなければならず、違うとコマンドは止まり、どこが違うかを示します。ノードのブートコアはアプリと一緒にパッケージに入り、ノードが動かしているブートコアとバージョンが違うときだけ書き込まれます。アプリと同じく、ブートコアを変えたらバージョンを上げてください。

### 5. 最初のバージョンを書き込む（ノードごとに 1 回）

ノードには、ブートコアと最初のアプリを、そのノード自身のケーブルで 1 回だけ書き込みます。`DAGON_SIGNING_KEY` を設定し（手順 1）、ノードをつないで実行します:

```sh
dagon flash hub        # RP2040: BOOT を押しながら USB をつなぐと、rescue.uf2 を RPI-RP2 のドライブにコピー
dagon flash seg        # STM32F401RE: ST-LINK 経由で probe-rs が書き込む
dagon flash watch      # ESP32-S3: USB-C 経由で espflash が書き込む
```

`dagon flash` は先に手順を表示し、ボードに触れる前に確認を求めます（`--yes` で確認を省きます）。書き込み先は書き込みツールが見つけたデバイス（probe-rs のすべてのプローブ、すべてのシリアルポート、すべての UF2 ドライブ）から選びます。USB の ID や UF2 の Board-ID は表示に使うだけで、候補を絞らず、一致した候補を先頭に並べます。1 つならそれを選んで確認のときに示し、2 つ以上なら番号で選ぶ（一致した候補があれば Enter でそれを選べる）か `--probe <vid:pid[:serial]>`・`--port <port>`・`--drive <path>` で指定します（Windows では大文字と小文字を区別せず、`--drive e:` は `E:\` を指します）。2 つ以上で端末がない（または `--yes`）のに指定がないとき、または 1 つもないときは書く前に止まります。`--dry-run` も書き込みの手前まではすべて行います: 部品とアプリをビルドし、初回書込みのファイルを provision のフォルダ（`dagon.toml` の横の `out/provision/<id>/`、`dagon.toml` に `provision_dir` があればその下、`--out <dir>` で別の場所）に作り、ボードにだけ触れません。ブートコア（親機は `stage0` も）を CLI をビルドした DAGON のクローン（または `DAGON_HOME`）から、ノードのアプリと一緒に、署名の鍵の公開側でビルドします。ブートコアはデスクのデモの tag（`hub-rp2040-lcd`、`seg-f401-7seg`）を受け付けるので、これらのボードでは `topology.yaml` の tag をそのままにします。

ESP32-S3 のノードでは、`dagon flash` がボードの factory エージェントとアプリをビルドし、アプリを ESP のイメージにして（`espflash save-image --chip esp32s3 --flash-size 16mb`）署名し、`espflash` で書き込みます: 同梱の無改変の ESP-IDF 2nd stage を 0x0 に（SHA-256 が `targets/mcu/esp32s3/vendor/SHA256SUMS` と一致しなければ書きません）、パーティション表（`targets/mcu/esp32s3/partitions.csv`、0x8000）、factory エージェント、ota_0（0x110000）にアプリ、`dagon` パーティション（0x710000）にその署名済みヘッダ、確定済みとして起動させる otadata の項目（0xD000）。`--dry-run` はファイルとそのオフセット、`espflash` のコマンドを示します（同じ provision のフォルダの `commands.txt`）。

そのあと親機をつなぎ、すべてのノードが応答するか確かめます:

```sh
dagon status
```

`dagon flash` を使わない場合（自分でビルドしたブート用のイメージを使うとき）: `dagon rescue hub` と `dagon provision <id>` が、`dagon.toml` の `[provision.<id>]`（親機は `stage0`、ほかに `bootcore`、`app`、`version`）から同じファイルを作ります。ブート用のイメージのビルドは MCU のパッケージの README にあります。

### 6. 更新し、必要なら戻す

コードを変えたら:

```sh
dagon deploy                       # または: dagon deploy seg watch
```

`dagon deploy` はスロット A と B をビルドし、署名し、リリースを作り、ノードごとに今のバージョンと新しいバージョンを並べて、適用の前に確認を求めます（`--yes` で省けます）。今のバージョンは、前回のリリース（`out/release-latest.dgb`、または `dagon.toml` の `release`）がその鍵で検証できたときに、そのリリースにあるノードのバージョンです。ボードから読むのではありません。新しいバージョンは、各ノードのクレートのバージョンです。ノードの id を省くと `[build.<id>]` のすべてのノードが対象です。ESP32-S3 のノードは 1 回だけビルドし（イメージはどちらの OTA スロットでも動く）、deploy がそれを ESP のイメージにして（`espflash save-image`）署名し、親機がほかのノードと同じように中継します。1 つずつの手順は `dagon build`、`dagon image sign`、`dagon release build`、`dagon apply` です（`dagon --help`）。DAGON Console では、準備の「ビルドする」が全ノードをビルドして署名し、パッケージを作ります。書込では、カードのチェックで書き込むノードを選び、ヘッダーでバージョン（最新のビルド、または前に戻すときは保存済みのもの）を選んで「書き込む」と、Web Serial から書き込みます。カードには今 → 新のバージョンと書き込み全体の % が出て、終わると結果が出ます。

ノードへ書き込んだパッケージ（Console、`dagon apply`、`dagon deploy`）は、すべてプロジェクトの `out/deploy/` に `v<バージョン>.dgb` として保存されます（ノードごとにバージョンが違うときは `v<いちばん新しいバージョン>-<ダイジェスト>.dgb`）。前のバージョンに戻すには、保存された古いバージョンをもう一度書き込みます。Console はその一覧を出し、コマンドでは `dagon apply out/deploy/v1.0.0.dgb` で書き込めます。

適用では、署名と、リリースがこの構成に対して作られたことを確かめ、構成を親機へ送り、子機を更新してから親機を更新します。ノードは新しいバージョンを試行起動し、確定されなければウォッチドッグでリセットされて前のバージョンで起動し直します。

```sh
dagon rollback --node seg     # 前に確定したバージョンへ戻す
dagon recover seg             # 応答しなくなったノード
```

すべてのコマンドとオプションは `dagon --help` で確認できます。

## プロジェクトでの Claude の手助け

`dagon new` はプロジェクトに Claude Code のファイルも書きます。プロジェクトのフォルダで開いた Claude Code は、構成のファイル、コマンド、守ることを知った状態で始まります:

| ファイル | 役目 |
|---|---|
| `CLAUDE.md` | プロジェクトの配置、自分のファイルと生成されるファイルの区別、コマンド、守ること（鍵に触れない、ハードウェアの値は推測せず質問する） |
| `.claude/skills/fix-a-failing-build/SKILL.md` | `dagon build` が失敗したとき: 原因を探し、`nodes/<id>/src` の自分のコードだけを直して、もう一度ビルドする |
| `.claude/skills/add-a-board-by-mcu/SKILL.md` | 対応表に無いボードをマイコンで選んで加える。値はボードの資料から出典付きで |
| `.claude/skills/explain-a-topology-problem/SKILL.md` | 構成の検査が示す問題を分かる言葉で説明し、いちばん小さい直し方を示す |

これらのファイルには鍵も、この PC のパスも入りません。署名の鍵と Claude の API キーは `dagon` CLI の中にとどまります。Claude にはそれらを開かないよう指示してあり、署名と実機への書込みは開発者が行います。

このリポジトリの `plugin/` は、DAGON を新しいマイコンへ移植するための Claude Code のプラグイン（`/dagon-port`）で、適合試験（`dagon conform`）で確かめます。

## 自分のボード（マイコンで選ぶ）

`topology.yaml` のそのノードから `board:` を外し、ボードの値を `dagon.toml` に書きます:

```yaml
  - id: seg
    mcu: stm32f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

```toml
[board.seg]
flash_bytes = 524288
```

I2C の子機では、リンクにノード側のピン（`node_pins`）を書きます。足りないものは検査と `dagon gen` が知らせます。`add-a-board-by-mcu` スキルを使えば Claude と一緒に進められます。

## ハードウェアなしで試す

```sh
dagon demo                         # 模擬の机で DAGON Console を開く
dagon demo --sim-fail watch        # 同じで、watch の更新が失敗して元に戻る
dagon demo --sim-speed 4           # 4 倍の速さ（録画用）
```

`dagon demo` は、模擬の親機と子機の 3 つのノード（デスクのデモの構成: hub、seg、watch、すべて 1.0.0）を相手に DAGON Console を開きます。`dagon sim run` と同じシミュレータを、実機と同じ dagon-host のコードで動かします。つないで、最新のリリース（demo-1.1.0）を選び、3 つのノードすべてを進み具合を見ながら更新できます。`--sim-fail seg` か `--sim-fail watch` を付けると、その子機の新しいバージョンが起動時に落ち、1.0.0 に戻ります。リンクとフラッシュは実機に近い時間がかかります（`--sim-speed` で 0.1〜20 倍）。ヘッダには常に「模擬」が出ます。デモは一時フォルダに自分のプロジェクト（`dagon-demo-<ユーザー名のハッシュ>`、起動のたびに新しく作る）と、そのためだけの DEMO の鍵を作ります。あなたの鍵は使わず、デモで署名したものは実機では動きません。

```sh
dagon sim run
```

シミュレータ上の 4 つの通しシナリオ: 転送中の電断を含む更新、確定しないイメージ、改ざんしたリリース、全消去した F401 の ROM ブートローダ経由の復活。

### つながりを差し替える

構成のホップには、それぞれ種類があります: `usb-cdc`、`uart`、`i2c`、`ble`、`wifi-tcp`、`ethernet`、`can`。親機は、上り（親機を動かす側へ）のつながりと、その先にあるものも書けます:

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

`uplink` を書かなければ、これまでどおりパソコンの dagon への `usb-cdc` です。上りのつながりはコンパイルした構成に入らないので、今ある構成とそのリリースは変わりません。DAGON Console の準備では、インスペクタでリンクごとの種類と、親機の上りのつながりを選べます。どちらかの端のファームウェアがまだ実装していない種類には ◇ まだ使えない が付きます。構成には書けます（問題「まだ使えないつながり」）が、`dagon gen` はそのホップの名前を挙げて断ります（`uplink of hub over ble: not available yet on rp2040`）。今ファームウェアにあるのは、各 MCU の定義（`[links]`）にあるとおり次のものです（上り: パソコンや親機へ、下り: 子機へ）:

- RP2040: 上り usb-cdc、下り i2c, uart
- STM32F401RE: 上り i2c
- ESP32-S3: 上り uart, usb-cdc

デモでは、新しい無線なしで差し替えを見られます:

```sh
dagon demo --uplink uart          # または usb-cdc（既定）、wifi-tcp
```

模擬の上りのつながりはデモの構成に従います。準備で親機の上りのつながりを変える（USB、UART、Wi-Fi）と、同じ更新が別の媒体で動きます。媒体ごとに、フレームの大きさ、速さ、親機の再起動中のふるまい（USB CDC と Wi-Fi はつなぎ直し、UART アダプタはつながったまま）が違います。

## 現在できること

このリポジトリのコードとテストが示していること:

- 形式とプロトコル: `dagon-format` と `dagon-proto`。`crates/dagon-format/golden/` の黄金ベクタで照合しています。
- ノード側: `dagon-agent`（非稼働スロットへイメージを受け取り、署名と SHA-256 を検査し、切り替えと確定を行う）と `dagon-core`（起動の判断: 回数を限った試行起動と巻き戻し）。
- PC 側: `dagon-host`（状態、自動で見つける、構成の送信、更新、前のバージョンに戻す、確定、復旧）、`dagon-romlink`（STM32 の I2C ROM ブートローダの手順）、`dagon` CLI。これらは `dagon-sim`（F401 子機付きの模擬親機、電断点付きのフラッシュ、損失のあるリンク、ROM ブートローダの模擬）を相手にテストしています。
- プロジェクト: `dagon new`、`dagon gen`、`dagon build` が、MCU ごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の支援パッケージの上にノードの crate を作り、ビルドします。
- `dagon conform`: `dagon-conform` の 12 件の適合試験。模擬の F401 に対し、親機経由でも直結でも 12 件すべて合格します。
- 構成 YAML: 22 件の検証規則と対応ボードの表（`dagon-topology`）。`topology/desk-demo.yaml` はこれを通ります。
- DAGON Console（`web/`。`dagon serve` が配る。いまできることは `web/README.md` を参照）。
- ファームウェア: RP2040 親機（`targets/rp2040-hub`）、F401 のデモ子機（`samples/f401-app`）、ESP32-S3 子機（`targets/boards/m5stopwatch`、`samples/esp32s3-app`）。親機のファームウェアは親機の実機に書き込み済みで起動します。それ以外の実機での確認はまだ進行中です（「制限事項」を参照）。

## 既にあるファームウェアに DAGON を組み込む

`dagon new` のプロジェクトではないファームウェアの場合:

1. crate をリリースのタグに固定した git 依存として加えます。例: `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port`、ブートコアを作るなら `dagon-core` も同様です。
2. 対応するマイコンなら、MCU のパッケージ（`targets/mcu/<mcu>`。フラッシュの配置、リンカの割り付け、ブート用のイメージ、見守り、リンク）とボードのパッケージ（`targets/boards/<board id>`）を使うか、自分のボード用に MCU のパッケージの `BoardDef` を実装します。MCU のパッケージの README に、約 20 行の完全なアプリがあります。ほかのマイコンでは同じ形にフラッシュを割り付けます: ブートコア、状態ログ 2 面、スロット A と B。各スロットの先頭に 512 バイトの署名付きイメージヘッダ、本体はスロット先頭 + 0x200。
3. ボード用に `dagon-port` の trait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS` か `BootSelectV`）と `dagon_agent::SlotStore` を実装し、アプリ層のチェーンを組み立て（拡張が判断点に答え、イベントを受け取ります。`docs/events.md` を参照）、リンクを渡してエージェントを起動します:

   ```rust
   let chain = dagon_events::compose(Layer::App, events, board_exts, user_exts, bounds);
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, chain);
   agent.run(&mut link, &mut reset).await;                    // 要求に応え続ける
   ```

   確定済みのあいだはウォッチドッグに給餌し、試行中は限られた時間だけ給餌して、確定されないイメージがリセットされ巻き戻るようにします。
4. `dagon conform` で移植を確かめます。

## リポジトリの構成

| パス | 内容 |
|---|---|
| `crates/dagon-format` | イメージヘッダ、マニフェスト、リリースバンドル、状態ログ、構成バイナリ、黄金ベクタ |
| `crates/dagon-proto` | 通信プロトコル v1: フレーム、コマンド、ペイロード |
| `crates/dagon-port` | ボードの trait（フラッシュ、リンク、リセット、ウォッチドッグ、起動先の選択、I2C、子機の制御線） |
| `crates/dagon-core` | 起動の判断（試行、回数、巻き戻し） |
| `crates/dagon-agent` | ノード側の更新の受け手 |
| `crates/dagon-conform` | 12 件の適合試験と結果ファイルの形式 |
| `crates/dagon-sim` | シミュレータ: フラッシュ、リンク、ROM ブートローダ、ノード、親機 |
| `crates/dagon-topology` | 構成 YAML、検証規則、対応ボード |
| `crates/dagon-sign` | 鍵、イメージの署名、リリースの作成、初期導入用ファイル |
| `crates/dagon-host` | CLI と DAGON Console が共有する PC 側の手順 |
| `crates/dagon-romlink` | STM32 の I2C ROM ブートローダの手順 |
| `crates/dagon-cli` | `dagon` コマンド（DAGON Console を組み込み） |
| `crates/dagon-wasm` | DAGON Console 用の WebAssembly バインディング |
| `targets/` | MCU ごと（`targets/mcu/*`）とボードごと（`targets/boards/*`）の支援パッケージ、RP2040 親機のファームウェア、立ち上げ用ツール |
| `samples/` | 支援パッケージを使うサンプルのアプリ（DAGON の一部ではない） |
| `web/` | DAGON Console（Svelte、Vite、TypeScript） |
| `topology/` | 構成の例 |
| `plugin/` | 新しいマイコンへの移植に使う Claude Code のプラグイン（エージェント、スキル、フック） |
| `scripts/licences.py` | 下の依存ライブラリのライセンス表を生成する |

## DAGON そのものを開発するとき

- [必要なもの](#必要なもの)のすべてと、`wasm32-unknown-unknown` のターゲット（ターゲットはすべて `rust-toolchain.toml` にあります）
- `crates/dagon-wasm` 用の `wasm-pack` 0.15.1（対応する `wasm-bindgen` と `wasm-opt` は自分で取得します）と、`web/` 用の Node.js と npm
- ESP32-S3 の 2nd stage を `targets/esp32s3-2ndstage` からビルドし直すときは ESP-IDF v6.1（ビルド済みのものを `targets/mcu/esp32s3/vendor/bootloader.bin` として同梱している。無改変の ESP-IDF v6.1。`vendor/PROVENANCE.md` を参照）
- RP2040 の立ち上げ用イメージに使う `elf2uf2-rs`
- `cargo install --path crates/dagon-cli` でクローンから CLI を入れます。プロジェクトは `dagon new --dagon-path <クローン>`（または `DAGON_HOME`）でそのクローンを使います
- リリースのアーカイブ: `python scripts/release.py [--target <triple>]` が、ページを組み込んだ `out/release/dagon-cli-<version>-<target>.*` を作ります。`.github/workflows/release.yml` は `cli-v*` のタグが push されたときに、コミットが求める上げ幅に版が足りているかを `scripts/bump_check.py` で確かめてから各プラットフォームで同じことをし、ワークフローの成果物として残します（公開はしません）

## ライセンス (MIT / Apache-2.0 デュアル)

このリポジトリで DAGON Boot のために書いたものは、[MIT ライセンス](LICENSE-MIT) と [Apache License, Version 2.0](LICENSE-APACHE) のどちらかを選んで利用できます。

## 依存ライブラリとそのライセンス

リポジトリ内のすべての `Cargo.lock` と `web/package-lock.json` から `python scripts/licences.py` で生成しています（表は英語のままです）。

注意点:

- `serialport`（MPL-2.0）は `dagon` CLI が改変せずに使っています。`dagon` の実行ファイルを配布する場合、MPL-2.0 により `serialport` のソースを入手できるようにする必要があります。公開されている crate <https://crates.io/crates/serialport>（リポジトリ <https://github.com/serialport/serialport-rs>）がそのソースです。
- `unescaper` は MIT OR GPL-3.0-only で提供されており、MIT で使っています。
- `ring` は Apache-2.0 AND ISC（両方が適用されます）。
- `webpki-roots` は CDLA-Permissive-2.0 です（`dagon ai topology` の TLS クライアントが使う外部データ、Mozilla のルート証明書のデータを含みます）。
- `subtle`、`rp-pac`、`rp2040-boot2`、`source-map-js` は BSD-3-Clause です。
- `lightningcss`（MPL-2.0）は web のビルドツールのビルド時の依存だけで、DAGON Console には何も含まれません。
- DAGON Console に含まれる npm のコードは、Svelte のクライアント用ランタイムと、それが読み込む `clsx` と `esm-env`（いずれも MIT）だけです。他の npm パッケージはビルドとテストの道具です（表の "Ships in" 列）。
- `web/public/fonts/` の書体は SIL Open Font License 1.1 です: Doto、DotGothic16、Zen Kaku Gothic New、JetBrains Mono（ライセンス文はファイルの隣にあります。`web/README.md` を参照）。
- 親機の表示の日本語の状態語は、Noto Sans JP（SIL Open Font License 1.1）から作ったビットマップです（ライセンス文は `targets/boards/waveshare-rp2040-touch-lcd-1.28/fonts/`）。

Rust crates: 316 packages, 350 name/version pairs. "Used by": `crates/` is the host workspace, `samples/...` the sample apps, other names are firmware packages under `targets/`.

| Crate | Versions | Licence | Used by |
|---|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| aligned | 0.4.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| allocator-api2 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| ascii-canvas | 4.0.0 | Apache-2.0/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| az | 1.2.1, 1.3.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bare-metal | 0.2.5 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| base64 | 0.23.1 | MIT OR Apache-2.0 | crates/ |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bit_field | 0.10.3 | Apache-2.0/MIT | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| bitfield | 0.13.2, 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| bitflags | 1.3.2, 2.13.2 | MIT OR Apache-2.0 / MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| block-buffer | 0.10.4, 0.12.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| block-device-driver | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates/ |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| byteorder | 1.5.0 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| bytes | 1.12.1 | MIT | crates/ |
| cc | 1.10.0, 1.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| codespan-reporting | 0.11.1 | Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| const-default | 1.0.0 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | crates/ |
| cordyceps | 0.3.5 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | crates/ |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | crates/ |
| cortex-m | 0.7.10 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| cortex-m-macros | 0.7.8 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| cortex-m-rt | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| cortex-m-rt-macros | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| cpufeatures | 0.2.17, 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| crc-any | 2.5.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| crunchy | 0.2.4 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| crypto-common | 0.1.7, 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| darling | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| darling_core | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| darling_macro | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| debug-helper | 0.3.14 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| defmt | 0.3.101, 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-rtt | 1.3.1 | MIT OR Apache-2.0 | f401-bringup, samples/f401-app |
| delegate | 0.13.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| digest | 0.10.7, 0.11.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| document-features | 0.2.12 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ed25519-compact | 2.6.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| either | 1.19.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-hal-internal | 0.4.0, 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| embassy-net-driver-channel | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-rp | 0.10.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-stm32 | 0.6.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| embassy-sync | 0.6.2, 0.7.2, 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, f401-bringup, mcu/esp32s3, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-usb | 0.6.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-usb-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-usb-synopsys-otg | 0.3.3, 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| embedded-hal | 0.2.7, 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-hal-nb | 1.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| embedded-io | 0.6.1, 0.7.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-io-async | 0.6.1, 0.7.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ena | 0.14.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| encode_unicode | 0.3.6 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| enumset | 1.1.15 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| enumset_derive | 0.15.1 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| esp-alloc | 0.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-bootloader-esp-idf | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-config | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal | 1.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal-co5300 | 0.1.0 | MIT OR Apache-2.0 | samples/esp32s3-app |
| esp-hal-procmacros | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-metadata-generated | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-riscv-rt | 0.15.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-rom-sys | 0.1.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-rtos | 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-storage | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-sync | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32 | 0.41.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c2 | 0.30.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c3 | 0.33.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c5 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c6 | 0.24.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c61 | 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32h2 | 0.20.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32p4 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s2 | 0.32.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s3 | 0.36.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s31 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| fixed | 1.29.0, 1.31.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| float-cmp | 0.9.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| fnv | 1.0.7 | Apache-2.0 / MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| fugit | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| gcd | 2.3.0 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| generator | 0.8.11 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| generic-array | 0.14.7 | MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| getrandom | 0.2.17, 0.4.3 | MIT OR Apache-2.0 | crates/ |
| half | 2.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| heapless | 0.8.0, 0.9.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| heck | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| http | 1.5.0 | MIT OR Apache-2.0 | crates/ |
| httparse | 1.10.1 | MIT OR Apache-2.0 | crates/ |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| ident_case | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| indoc | 2.0.8 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| instability | 0.3.14 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| io-kit-sys | 0.4.1 | MIT / Apache-2.0 | crates/ |
| itertools | 0.14.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| itoa | 1.0.18 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| jiff | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-core | 0.1.1 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-static | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| js-sys | 0.3.107 | MIT OR Apache-2.0 | crates/ |
| keccak | 0.1.6 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lalrpop | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lalrpop-util | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| libc | 0.2.191 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| libyaml-rs | 0.3.0 | MIT | crates/ |
| linked_list_allocator | 0.10.6 | Apache-2.0/MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| litrs | 1.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| log | 0.4.35 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| loom | 0.7.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | crates/ |
| matchers | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| memchr | 2.8.3 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| micromath | 2.1.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| nb | 0.1.3, 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| new_debug_unreachable | 1.0.6 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| nix | 0.26.4 | MIT | crates/ |
| nu-ansi-term | 0.50.3 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| object | 0.37.3 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| panic-halt | 1.0.0 | MIT OR Apache-2.0 | mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump |
| panic-probe | 1.0.0 | MIT OR Apache-2.0 | f401-bringup, samples/f401-app |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| paste | 1.0.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | crates/ |
| petgraph | 0.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| phf_shared | 0.11.3 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pico-args | 0.5.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| pio | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-core | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-parser | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-proc | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| precomputed-hash | 0.1.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| proc-macro-error-attr2 | 2.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro-error2 | 2.0.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| quote | 1.0.47 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | crates/ |
| rand_core | 0.10.1, 0.6.4, 0.9.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| redox_syscall | 0.5.18 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| regex | 1.13.1 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| rgb | 0.8.53 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| ring | 0.17.14 | Apache-2.0 AND ISC | crates/ |
| riscv | 0.15.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-pac | 0.2.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt | 0.16.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-target-parser | 0.1.3 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rlsf | 0.2.3 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rp-pac | 7.0.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| rp2040-boot2 | 0.3.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| rustc_version | 0.2.3 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | crates/ |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | crates/ |
| rustls-webpki | 0.103.16 | ISC | crates/ |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| same-file | 1.0.6 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sdio | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| sdio-host | 0.9.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| semver | 0.9.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| semver-parser | 0.7.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| serde | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | crates/ |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates/ |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| serialport | 4.10.1 | MPL-2.0 | crates/ |
| sha1 | 0.11.0 | MIT OR Apache-2.0 | crates/ |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| sha2-const-stable | 0.1.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sha3 | 0.10.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sharded-slab | 0.1.7 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| shlex | 2.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| slab | 0.4.12 | MIT | crates/ |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| smart-leds | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| smart-leds-trait | 0.3.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| somni-expr | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-parser | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-template | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| ssmarshal | 1.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| static_cell | 2.1.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| stm32-fmc | 0.4.0 | MIT/Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| stm32-metapac | 21.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| strsim | 0.11.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| strum | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| strum_macros | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| subtle | 2.6.1 | BSD-3-Clause | crates/ |
| syn | 1.0.109, 2.0.119, 3.0.9 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| term | 1.2.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| termcolor | 1.4.1 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tokio | 1.54.1 | MIT | crates/ |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates/ |
| toml_datetime | 0.6.11, 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_edit | 0.22.27, 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates/ |
| tracing | 0.1.44 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-attributes | 0.1.31 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-core | 0.1.36 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-log | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-subscriber | 0.3.23 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| trait-set | 0.3.0 | MIT | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| typenum | 1.20.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| unescaper | 0.1.10 | MIT OR GPL-3.0-only | crates/ |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| unicode-width | 0.1.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| unsafe-libyaml | 0.2.11 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| untrusted | 0.9.0 | ISC | crates/ |
| ureq | 3.5.0 | MIT OR Apache-2.0 | crates/ |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | crates/ |
| usb-device | 0.3.2 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid-descriptors | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid-macros | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | crates/ |
| valuable | 0.1.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| vcell | 0.1.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| version_check | 0.9.5 | MIT/Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| void | 1.0.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| volatile-register | 0.2.2 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| walkdir | 2.5.0 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-futures | 0.4.80 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | crates/ |
| winapi-util | 0.1.11 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-sys | 0.52.0, 0.61.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| winnow | 0.7.14, 1.0.4 | MIT | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 | crates/ |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | crates/ |
| zmij | 1.0.23 | MIT | crates/ |

npm packages (`web/package-lock.json`): 86. Only those marked "page" ship in the built page (the Svelte client runtime and what it imports); the rest are build and test tools.

| Package | Version | Licence | Ships in |
|---|---|---|---|
| @jridgewell/gen-mapping | 0.3.13 | MIT | build/test |
| @jridgewell/remapping | 2.3.5 | MIT | build/test |
| @jridgewell/resolve-uri | 3.1.2 | MIT | build/test |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT | build/test |
| @jridgewell/trace-mapping | 0.3.31 | MIT | build/test |
| @oxc-project/types | 0.156.0 | MIT | build/test |
| @rolldown/binding-android-arm-eabi | 1.2.16 | MIT | build/test |
| @rolldown/binding-android-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-darwin-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-darwin-x64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-freebsd-x64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm-gnueabihf | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm64-musl | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-ppc64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-s390x-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-x64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-x64-musl | 1.2.16 | MIT | build/test |
| @rolldown/binding-openharmony-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-win32-arm64-msvc | 1.2.16 | MIT | build/test |
| @rolldown/binding-win32-x64-msvc | 1.2.16 | MIT | build/test |
| @rolldown/pluginutils | 1.0.1 | MIT | build/test |
| @sveltejs/acorn-typescript | 1.0.13 | MIT | build/test |
| @sveltejs/load-config | 0.2.3 | MIT | build/test |
| @sveltejs/vite-plugin-svelte | 7.3.1 | MIT | build/test |
| @types/chai | 5.2.3 | MIT | build/test |
| @types/deep-eql | 4.0.2 | MIT | build/test |
| @types/estree | 1.0.10 | MIT | build/test |
| @vitest/mocker | 5.0.5 | MIT | build/test |
| @vitest/spy | 5.0.5 | MIT | build/test |
| acorn | 8.19.0 | MIT | build/test |
| aria-query | 5.3.1 | Apache-2.0 | build/test |
| assertion-error | 2.0.1 | MIT | build/test |
| axobject-query | 4.1.0 | Apache-2.0 | build/test |
| chai | 6.3.0 | MIT | build/test |
| chokidar | 4.0.3 | MIT | build/test |
| clsx | 2.1.1 | MIT | page |
| deepmerge | 4.3.1 | MIT | build/test |
| detect-libc | 2.1.2 | Apache-2.0 | build/test |
| devalue | 5.9.4 | MIT | build/test |
| es-module-lexer | 2.3.2 | MIT | build/test |
| esm-env | 1.2.2 | MIT | page |
| esrap | 2.4.0 | MIT | build/test |
| estree-walker | 3.0.3 | MIT | build/test |
| expect-type | 1.4.0 | Apache-2.0 | build/test |
| fdir | 6.5.0 | MIT | build/test |
| fsevents | 2.3.3 | MIT | build/test |
| is-reference | 3.0.3 | MIT | build/test |
| lightningcss | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-android-arm64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-darwin-arm64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-darwin-x64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-freebsd-x64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm-gnueabihf | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm64-gnu | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm64-musl | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-x64-gnu | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-x64-musl | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-win32-arm64-msvc | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-win32-x64-msvc | 1.33.0 | MPL-2.0 | build/test |
| locate-character | 3.0.0 | MIT | build/test |
| magic-string | 0.30.21 | MIT | build/test |
| magic-string | 1.4.5 | MIT | build/test |
| mri | 1.2.0 | MIT | build/test |
| nanoid | 3.3.20 | MIT | build/test |
| obug | 2.2.1 | MIT | build/test |
| obug | 3.0.0 | MIT | build/test |
| picocolors | 1.1.1 | ISC | build/test |
| picomatch | 4.0.7 | MIT | build/test |
| postcss | 8.5.32 | MIT | build/test |
| readdirp | 4.1.2 | MIT | build/test |
| rolldown | 1.2.16 | MIT | build/test |
| sade | 1.8.1 | MIT | build/test |
| source-map-js | 1.2.2 | BSD-3-Clause | build/test |
| std-env | 4.3.0 | MIT | build/test |
| svelte | 5.57.3 | MIT | page |
| svelte-check | 4.7.8 | MIT | build/test |
| tinybench | 6.2.3 | MIT | build/test |
| tinyexec | 1.3.1 | MIT | build/test |
| tinyglobby | 0.2.17 | MIT | build/test |
| typescript | 6.0.3 | Apache-2.0 | build/test |
| vite | 8.3.5 | MIT | build/test |
| vitefu | 1.1.3 | MIT | build/test |
| vitest | 5.0.5 | MIT | build/test |
| why-is-node-running | 3.2.1 | MIT | build/test |
| zimmerframe | 1.1.5 | MIT | build/test |

## 自作コードの範囲

- DAGON Boot のために書いたもの: `crates/`、`targets/`（下の無改変のベンダーコンポーネントを除く）、`web/src/`、`web/scripts/`、`plugin/`、`topology/`、`scripts/` のすべて。
- Cargo と npm を通して改変せずに使う依存: 上の表の crate とパッケージ（例: embassy、esp-hal、ed25519-compact、sha2、serde、Svelte、Vite）。
- 無改変のベンダーコンポーネント: 下を参照。

## 使用していないコンポーネント (公開されているブートローダ実装は使用していません)

公開されているブートローダ実装（例: embassy-boot、MCUboot、wolfBoot）のコードもロジックも使っていません。起動の判断、イメージとリリースの形式、状態ログ、更新のプロトコル、受け手はこのプロジェクトのために書いたものです。

## 無改変のベンダーコンポーネント

- ESP32-S3 の 2nd stage ブートローダ: 公式の ESP-IDF v6.1 のブートローダを、アプリのロールバックを有効にして公式のソースからビルドしたもの（`targets/esp32s3-2ndstage` で自作したのはプロジェクトのファイルと `vendor/sdkconfig.defaults` の sdkconfig オプションだけ）。無改変のまま `targets/mcu/esp32s3/vendor/bootloader.bin` として、出どころとライセンスを添えて同梱している（`vendor/PROVENANCE.md`、`vendor/LICENSE-ESP-IDF`）。
- `esp-bootloader-esp-idf`（crate、MIT OR Apache-2.0）: ESP32-S3 のポートが otadata の読み書きに改変せずに使っています。この 2nd stage でのスロットの選択は otadata で行います。
- RP2040 の boot2: `rp2040-boot2` crate（BSD-3-Clause）の generic 03h の second stage を、`targets/mcu/rp2040/stage0` がフラッシュの先頭に改変せずに置いています。
- STM32F401 の ROM にあるシステムメモリのブートローダ（文書化された I2C の手順で復旧に使います）。

## 制限事項

- ESP32-S3 は公式の ESP-IDF の 2nd stage を使います。その 2nd stage 自体が壊れた場合、DAGON では救えません。ベンダーのツールで USB から書き直す必要があります。
- 署名の鍵は PC 上のファイルで、公開鍵はファームウェアに組み込まれます。ハードウェアの信頼の起点（セキュアブートのヒューズやハードウェアの鍵保管）はありません。
- 実機でのすべてのフラッシュ操作に対する電断の網羅的な調査はまだ行っていません。電断はシミュレータでテストしています。
- USB の VID:PID `c0de:cafe` は試験用の ID で、割り当てを受けたものではありません。
- F401 子機と ESP32-S3 子機のファームウェアの実機での確認はまだです。親機（LCD 付き RP2040）のファームウェアは実機に書き込み済みで起動しますが、PC との通信と表示はまだ確認中で、3 ノードでの通しの流れも同様です。
- `dagon conform` は同期の試験手順を動かすもので CLI 専用です。ブラウザでは適合試験を実行しません。
