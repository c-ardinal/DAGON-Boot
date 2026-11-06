---
title: "拡張のマニフェスト"
description: "dagon-extension.toml、dagon.toml の拡張のキー、そこから dagon gen が書き出すファイルです。"
---

拡張は、`Cargo.toml` の隣に `dagon-extension.toml` を持つ Rust の crate です。マニフェストは、拡張が何を置き換え、何を観測し、何に答えるか、何を必要とするかを示します。`dagon gen` は、これをポートのカタログ（[ports.md](/reference/ports/)）、イベントのカタログ（[events.md](/reference/events/)）、ノードのマイコンとボードの定義（[マイコンとボードの定義](/reference/definitions/)）に照らして確かめ、ノードの保護のファイルを書き出します。

## dagon-extension.toml（`extension-manifest` 形式 1）

```toml
format = 1
id = "uds-transport"                 # a-z 0-9 -, 1 to 32
layers = ["boot", "app"]             # boot, app, relay, host
provides = ["UpdateTransport", "UpdateProtocol"]
observes = ["boot.*", "app.update_*", "app.transfer_*"]
decides = ["accept_update"]
trusted = false
[capabilities]
peripherals = ["can0"]               # the board's [[peripheral]] ids
pins = []                            # the board's [[pin]] names
buses = []                           # the board's [[bus]] ids, never a dagon = true one
timer = true
[limits]
max_run_us = 2000                    # per handler call
stack_bytes = 1024
ram_bytes = 4096                     # .ext.data + .ext.bss
text_bytes = 16384                   # .ext.text + .ext.rodata + the data load image
```

<!-- generated from dagon-extension's manifest types: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `format` | 整数 | 必須 | 最初に読みます。この dagon が読まない番号は、両方の番号を示して受け付けません。 |
| `id` | 文字列 | 必須 | 拡張の id: `a-z 0-9 -`、1 から 32 文字。 |
| `layers` | 文字列の配列 | 必須 | 動くレイヤー: `boot`、`app`、`relay`、`host`。 |
| `provides` | 文字列の配列 | 任意 | 一覧のすべてのレイヤーにあるポート。信頼された拡張だけのポートには `trusted = true` が要ります。 |
| `observes` | 文字列の配列 | 任意 | `layer.kind`、`layer.prefix*`（カタログの種類の少なくとも 1 つに一致すること）、または `layer.*`。 |
| `decides` | 文字列の配列 | 任意 | 一覧のレイヤーのどれかで尋ねられる判断点。信頼された拡張はどれにも答えません。 |
| `trusted` | 真偽値 | 任意 | この拡張は、そのプロジェクトにとって DAGON の一部です。特権で、DAGON 自身のメモリの中で動き、サンドボックスの制限はどれも効きません。信頼された拡張だけのポートには必要です。観測するだけの拡張も指定できます（ブートローダで動く、プロジェクト自身のコード）。 |
| `capabilities` | `[capabilities]` テーブル | 任意 | DAGON が拡張に渡すもの（下の `[capabilities]` のキー）。 |
| `limits` | `[limits]` テーブル | 任意 | デバイスのレイヤー: 信頼されていない拡張と、イベントを観測するすべての拡張では、`max_run_us` は 1 から判断の上限（50 ms）まで（ハンドラの呼び出しごとにこれに照らして測ります）。信頼されていないものだけ: `stack_bytes` は 256 以上、`text_bytes` は 0 より大きいこと（下の `[limits]` のキー）。 |

`[capabilities]`:

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `peripherals` | 文字列の配列 | 任意 | ボードの `[[peripheral]]` の id: ウィンドウにできるブロックへのウィンドウ。 |
| `pins` | 文字列の配列 | 任意 | ボードの `[[pin]]` の名前。ピンのサービスを通して使います。 |
| `buses` | 文字列の配列 | 任意 | ボードの `[[bus]]` の id。バスのサービスを通して使います。 |
| `timer` | 真偽値 | 任意 | 時間のサービス。 |

`[limits]`:

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `max_run_us` | 整数 | 必須 | ハンドラの呼び出しごと。 |
| `stack_bytes` | 整数 | 任意 | サンドボックスの拡張には必須です（`STACK_MIN` 以上）。信頼された拡張は DAGON のスタックで動きます。 |
| `ram_bytes` | 整数 | 任意 | `.ext.data` + `.ext.bss`。 |
| `text_bytes` | 整数 | 任意 | `.ext.text` + `.ext.rodata` + `.ext.data` のロードイメージ。 |
<!-- end of generated table -->

crate は、拡張を `pub type DagonExtension` と `pub const DAGON_EXTENSION: DagonExtension` として公開します。生成されるチェーンがこの名前を使います。

## dagon.toml

```toml nocheck
[extensions]
packages = ["extensions/*"]          # folders, or folder/* for each subfolder
[extensions.nodes.<id>]
boot = ["uds-transport"]             # the boot core's chain, in order
app = ["uds-transport"]              # the app's chain (a hub's relay too)
trusted = [{ id = "hsm-verify", sha256 = "<source SHA-256, 64 hex digits>" }]
```

`sha256` は、拡張の crate のフォルダのソースのハッシュです。フォルダの中のすべてのファイルをパス（`/` 区切り）の順に並べ、ファイルごとにパス、ゼロのバイト、長さ（リトルエンディアンの u64）、バイト列（CRLF は LF として読む）を続けたものの SHA-256 です。フォルダの中に除外するものはありません（コンパイラは `include!` や `#[path]` でどれでも読めるため）。そのため、拡張のフォルダの中で cargo を実行すると、そこに `target/` と `Cargo.lock` が書かれ、ハッシュが変わります。`dagon gen` はこれを確かめ、2 つが違うときは見つけたハッシュを示します。拡張自身が依存するレジストリの crate は、このハッシュではなく、ブート用 crate の `Cargo.lock` で固定します。

`packages` はプロジェクトの中のフォルダです。外のものは `dagon gen` が受け付けません。DAGON のソースツリーにある拡張（下のテンプレートや `samples/` の下のサンプル）を使うには、そのフォルダをプロジェクトにコピーし、`path = "../../..."` の依存先を、プロジェクトの `[dagon] path` が示す DAGON のチェックアウトに向けてください。コピーのハッシュはこれらのパスで変わるので、プロジェクトごとに違います。最初の `dagon gen` が受け付けず、そのハッシュを示します。

`dagon gen` は、ノードごとに次のものを、拡張の名前と理由を示して受け付けません。拡張が持たないレイヤーのイメージに挙げられた拡張。1 つのチェーンで 2 度提供されるポート。ハッシュが無い、またはソースのハッシュがもう一致しない信頼された拡張。信頼されていないブート拡張（ブートローダは信頼された拡張だけを動かします）。DAGON のブート段階を持たないマイコン上のブート拡張（ESP32-S3 はベンダーの第 2 段階からアプリを起動するので、そこではブート拡張は動きません）。ボードが宣言していない能力。ウィンドウにできないペリフェラル。マイコンの `ext_ram_max` を超えるスタックと RAM。試行のウォッチドッグの周期の 4 分の 1 を超えるブートの呼び出し。コード、データ、スタックの後で、保護ユニットのリージョンの数を超えるウィンドウ。イメージに収まらないリージョン。拡張が宣言していないブロックまで開くことになるウィンドウ。そして、その種類の生成器がバウンスバッファやリージョンを置かない、データキャッシュ（`cache =
"id-cache"`）や密結合メモリ（`tcm`）を持つマイコン。ウィンドウはデバイスメモリです（キャッシュされません）。コード、データ、スタックは通常のメモリです。

## 生成されるファイル（`nodes/<id>/dagon/`）

| ファイル | 中身 |
|---|---|
| `boot_wiring.rs` | DAGON のブート段階を持つマイコン: ボードの LED、停止のパターン、電源の保持、ブートのアクションプログラム。ウォッチドッグの周期。ブート記録とヒューズの配置。隔離のレベル、種類、上限、ブート時に読み返すレジスタ。チェーンの順の拡張（`BootChain`、`BOOT_CHAIN`）と、ブートローダのスロットごとの保護マップ |
| `memory-<image>-<slot>.x` | RAM を定義しているマイコン: 拡張のリージョンを除いたイメージの FLASH と RAM、拡張ごとのリージョンとセクション、ブート記録とヒューズのセル |
| `protection.toml` | すべてのイメージの拡張、メモリ、保護マップ。読める形です |

`nodes/<id>/dagon/` には、最後の `dagon gen` が書いたものだけがあります。そこにあるほかのファイルのうち、gen の印（dagon gen が生成したと最初の行にある）を持つものは消します（印の無いものは残して報告します）。gen が受け付けなかった、または生成できなかったノードには、gen のファイルを残しません。`nodes`、`nodes/<id>`、`nodes/<id>/dagon` がシンボリックリンクかジャンクションのときは、gen はそこに何も書かず、何も消さず、そのことを報告します。

レベルが `none` のときはマップを作らず、`dagon gen` はノードの拡張が保護なしで動くことを知らせます。

## ブート拡張

DAGON のブート段階を持つマイコン（RP2040、STM32F401RE）では、`dagon gen` はノードのブート用 crate もプロジェクトの `boot/<id>/` に書き出します。元になるのは、マイコンの定義にあるブートローダのテンプレート（`bootcore/template/`、`[package.metadata.dagon] bootcore = "<mcu id>"` で見つけます）です。書き出すのは `Cargo.toml`（ブートローダのライブラリ、`dagon-events`、各ブート拡張の crate、そして `[package.metadata.dagon.boot]` の下のノード、マイコン、ボード、信頼された拡張）、`boot.rs`（ブートローダの `main`。`nodes/<id>/dagon/boot_wiring.rs` から）、`graph.stamp`（後述）、`.cargo/config.toml`（ターゲット、`DAGON_BOOT_CRATE`、ノードの `DAGON_TARGET_TAG`）です。独立したフォルダにしているのは、cargo が crate より上のすべてのフォルダの `.cargo/config.toml` を合わせて読み、ノードのアプリの crate にはアプリ用のものがあるからです。`boot/<id>/` には、`nodes/<id>/dagon/` と同じく、最後の `dagon gen` が書いたものだけがあります。そこにある cargo の `target/` と `Cargo.lock` には触れません。あなたが決める欄が 1 つあります。`boot/<id>/Cargo.toml` の `[package]` の下の `version`、つまりブートローダのバージョンです。`dagon gen` はテンプレートの値を 1 度だけ書き、それ以降はあなたの値を保ちます。ブートローダをリリースするたびに上げてください（ノードは、動かしているものとバージョンが違うブートローダを書き込みます）。`dagon gen` とビルドは、`Cargo.lock` にあるその crate 自身の項目をこの値にそろえ、ロックのほかの部分は変えません。そのファイルを読めない、または解析できないときは、`dagon gen` は理由を示して止まり、ファイルはそのままです。ノードを飛ばすときは、ほかのブート用 crate のファイルは消しますが、このファイルは残します。ビルドは、そのフォルダで `cargo build --release` を実行します（`DAGON_PUBKEY` はどのイメージとも同じです）。

ブートローダは拡張を特権で動かすので、信頼された拡張（マニフェストが `trusted = true` で、`[extensions.nodes.<id>] trusted` にソースのハッシュとともに挙げられたもの）だけを動かします。`boot.watchdog_armed` から `boot.before_jump` または `boot.halt` まで、ブートのイベント（[イベント](/reference/events/)、レイヤー `boot`）はそれぞれ、`observes` がそのイベントを挙げている拡張に、チェーンの順に届きます。呼び出しごとに `max_run_us` に照らして時間を測ります。それより長くかかった呼び出しは止めませんが、ブートローダはその後、`boot.extension_overran`（拡張、その持ち時間、かかった時間）を、その種類を観測している拡張に届け、先に進みます。ブート拡張が何をしても、ブートローダが起動するスロットは変わりません。呼び出しの合計が試行のウォッチドッグの周期の 4 分の 1 を超える可能性がある組み合わせは、`dagon gen` が受け付けません。数えるのは、観測するブートイベントごとに 1 回の呼び出しと、`boot.extension_overran` を観測する拡張については、いずれかのブート拡張の呼び出しごとにさらに 1 回です。

`dagon gen` は、ブート用 crate の解決済みの依存グラフも cargo で確かめます（すべてのフィーチャ、すべてのターゲット）。組み込まれるすべての crate は、DAGON のもの、宣言された信頼された拡張のフォルダの中のもの、またはブート用 crate の `Cargo.lock` にチェックサムがあるレジストリの crate でなければなりません。その後で、ブート用 crate の `Cargo.toml`、`Cargo.lock`、信頼されたソースのいまの内容に対して `boot/<id>/graph.stamp` を書きます。ブートローダのビルドは、このスタンプが無いとき、またはそれ以降にどれかが変わったときはビルドしません（`dagon gen` をもう一度実行してください）。ブートに判断点はありません。ブートローダが起動するスロットは DAGON だけが決めるので、拡張には変えられません。呼び出しはブートローダのフラッシュの上限とウォッチドッグを共有します。戻らない呼び出しがあると、ウォッチドッグがチップをリセットします。

`boot/<id>/Cargo.lock` ができた後は、`dagon gen` はグラフを `--locked` で解決します。そのため、拡張を足した後はロックのところで止まります。`boot/<id>/` で `cargo metadata --offline --format-version 1` を 1 度実行し（ロックに新しい crate を足します）、その後でもう一度 `dagon gen` を実行してください。拡張なしでブートローダをビルドするには、ノードの `boot` と `trusted` の一覧からそれを外し、もう一度 `dagon gen` を実行します。

ブートのイベントは、ブートローダの処理と同じ速さで来ます。`boot.watchdog_armed`、`boot.log_read`、`boot.before_decision` は、あいだに状態ログのフラッシュの読み出しだけを挟んで続きます。`boot.slot_chosen` または `boot.no_image` はイメージの検査の後に続き、`boot.before_jump` はアプリが起動してチップを引き継ぐ直前に来ます。拡張が表示したものはそこまでしか残らず、ハンドラの中の待ちはすべてブートの時間に加わります。

コピーして使える最小の信頼されたブート拡張は `targets/boot-extension-template/` です（ブートのイベントを数え、ブートローダが選んだスロットを覚えます）。

```toml
# dagon-extension.toml
format = 1
id = "boot-count"
layers = ["boot"]
observes = ["boot.*"]
trusted = true
[limits]
max_run_us = 1000
```

```rust
#![no_std]
use dagon_boot::sequence;
use dagon_events::{Ctx, Event, Extension};

pub type DagonExtension = BootCount;
pub const DAGON_EXTENSION: DagonExtension = BootCount { events: 0, chosen: None };

pub struct BootCount { events: u32, chosen: Option<(u8, bool)> }

impl Extension for BootCount {
    fn id(&self) -> &'static str { "boot-count" }
    fn on_event(&mut self, e: &Event<'_>, _ctx: &Ctx<'_>) {
        self.events += 1;
        if let Some(c) = e.get::<sequence::SlotChosen>() {
            self.chosen = Some((c.slot, c.trial));
        }
    }
}
```

```toml
# dagon.toml
[extensions]
packages = ["extensions/*"]
[extensions.nodes.hub]
boot = ["boot-count"]
trusted = [{ id = "boot-count", sha256 = "<the hash dagon gen names>" }]
```

DAGON はブート拡張にペリフェラルもピンも渡しません（`Ctx` が持つのはレイヤーだけです）。ハードウェア（ディスプレイ、ピン）を動かす信頼された拡張は、ブートローダの HAL の初期化の後で、`on_event` の中でマイコンの HAL から自分で取ります（特権で動くためです）。何を取るか、それが安全かどうかは、拡張の作者の責任です。DAGON はどれも確かめません。ブートローダが使うもの（RP2040 では、フラッシュ、ウォッチドッグ、クロック、そしてブートローダが停止した後のボードの `[boot] led`）には触れず、各呼び出しを `max_run_us` に収めなければなりません。ESP32-S3 には DAGON のブート段階がない（ベンダーの第 2 段階がアプリを起動する）ので、そこにブート拡張は存在せず、`dagon gen` はその理由を示して受け付けません。
