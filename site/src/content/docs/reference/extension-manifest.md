---
title: "拡張のマニフェスト"
description: "dagon-extension.toml、dagon.tomlの拡張のキー、そのキーからdagon genが出力するファイルです。"
---

拡張は`Cargo.toml`と同一のフォルダに`dagon-extension.toml`があるRustのcrateです。マニフェストは拡張の置換対象、観測対象、応答対象、必要なものを提示します。`dagon gen`はマニフェストをポートのカタログ（[ports.md](/reference/ports/)）、イベントのカタログ（[events.md](/reference/events/)）、ノードのマイコンとボードの定義（[マイコンとボードの定義](/reference/definitions/)）と照合して検証し、ノードの保護のファイルを出力します。

## dagon-extension.toml（`extension-manifest`形式1）

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
| `format` | 整数 | 必須 | 最初に読込します。このdagonが解釈しない番号は、両方の番号を明示して拒否します。 |
| `id` | 文字列 | 必須 | 拡張のid: `a-z 0-9 -`、1から32文字。 |
| `layers` | 文字列の配列 | 必須 | 動作するレイヤー: `boot`、`app`、`relay`、`host`。 |
| `provides` | 文字列の配列 | 任意 | 一覧の全レイヤーにあるポート。信頼された拡張専用のポートには`trusted = true`が必要です。 |
| `observes` | 文字列の配列 | 任意 | `layer.kind`、`layer.prefix*`（カタログの種類の最低1つに一致すること）、または`layer.*`。 |
| `decides` | 文字列の配列 | 任意 | 一覧のいずれかのレイヤーで照会する判断点。信頼された拡張はどれにも応答しません。 |
| `trusted` | 真偽値 | 任意 | この拡張は、そのプロジェクトにとってDAGONの一部です。特権で、DAGON自身のメモリ内で動作し、サンドボックスの制限はどれも無効です。信頼された拡張専用のポートには必要です。観測専用の拡張も指定可能です（ブートローダで動作する、プロジェクト自身のコード）。 |
| `capabilities` | `[capabilities]`テーブル | 任意 | DAGONが拡張に付与するもの（以下の`[capabilities]`のキー）。 |
| `limits` | `[limits]`テーブル | 任意 | デバイスのレイヤー: 信頼されていない拡張と、イベントを観測する全部の拡張では、`max_run_us`は1から判断の上限（50 ms）まで（ハンドラの呼出ごとにこの値と照合して計測します）。信頼されていない拡張のみ: `stack_bytes`は256以上、`text_bytes`は0超（以下の`[limits]`のキー）。 |

`[capabilities]`:

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `peripherals` | 文字列の配列 | 任意 | ボードの`[[peripheral]]`のid: ウィンドウ化が可能なブロックへのウィンドウ。 |
| `pins` | 文字列の配列 | 任意 | ボードの`[[pin]]`の名称。ピンのサービス経由で使用します。 |
| `buses` | 文字列の配列 | 任意 | ボードの`[[bus]]`のid。バスのサービス経由で使用します。 |
| `timer` | 真偽値 | 任意 | 時間のサービス。 |

`[limits]`:

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `max_run_us` | 整数 | 必須 | ハンドラの呼出ごと。 |
| `stack_bytes` | 整数 | 任意 | サンドボックスの拡張には必須です（`STACK_MIN`以上）。信頼された拡張はDAGONのスタックで動作します。 |
| `ram_bytes` | 整数 | 任意 | `.ext.data` + `.ext.bss`。 |
| `text_bytes` | 整数 | 任意 | `.ext.text` + `.ext.rodata` + `.ext.data`のロードイメージ。 |
<!-- end of generated table -->

crateは拡張を`pub type DagonExtension`と`pub const DAGON_EXTENSION: DagonExtension`として公開します。生成したチェーンがこの名称を使用します。

## dagon.toml

```toml nocheck
[extensions]
packages = ["extensions/*"]          # folders, or folder/* for each subfolder
[extensions.nodes.<id>]
boot = ["uds-transport"]             # the boot core's chain, in order
app = ["uds-transport"]              # the app's chain (a hub's relay too)
trusted = [{ id = "hsm-verify", sha256 = "<source SHA-256, 64 hex digits>" }]
```

`sha256`は拡張のcrateのフォルダにあるソースのハッシュです。フォルダ内の全ファイルをパス（`/`区切り）の順序に整列し、ファイルごとにパス、ゼロのバイト、バイト長（リトルエンディアンのu64）、バイト列（CRLFはLFとして読込）を連結したもののSHA-256です。フォルダ内に除外対象はありません（コンパイラは`include!`や`#[path]`で任意のファイルを読込可能なため）。そのため拡張のフォルダ内でcargoを実行すると、そこに`target/`と`Cargo.lock`を出力し、ハッシュが変化します。`dagon gen`はこのハッシュを検証し、不一致の場合は検出したハッシュを表示します。拡張自身が依存するレジストリのcrateは、このハッシュではなく、ブート用crateの`Cargo.lock`で固定します。

`packages`はプロジェクト内のフォルダです。外部のフォルダは`dagon gen`が拒否します。DAGONのソースツリーにある拡張（後述のテンプレートや`samples/`配下のサンプル）を使用する場合は、そのフォルダをプロジェクトにコピーし、`path = "../../..."`の依存先を、プロジェクトの`[dagon] path`が指定するDAGONのチェックアウトに変更してください。コピーのハッシュはこれらのパスで変化するため、プロジェクトごとに異なります。最初の`dagon gen`がそのハッシュを表示して拒否します。

`dagon gen`はノードごとに以下のものを、拡張の名称と理由を表示して拒否します。

- 拡張に該当レイヤーがないイメージに列挙した拡張
- 1つのチェーンで2度提供するポート
- ハッシュがない、またはソースのハッシュが不一致になった信頼された拡張
- 信頼されていないブート拡張（ブートローダは信頼された拡張だけを実行します）
- DAGONのブート段階がないマイコン上のブート拡張（ESP32-S3はベンダーの第2段階からアプリを起動するため、ブート拡張は動作しません）
- ボードが宣言していない能力
- ウィンドウ化が不可能なペリフェラル
- マイコンの`ext_ram_max`を超過するスタックとRAM
- 試行用ウォッチドッグ周期の4分の1を超過するブートの呼出
- コード、データ、スタックの分を除外すると、保護ユニットのリージョン数を超過するウィンドウ
- イメージに収容不可能なリージョン
- 拡張が宣言していないブロックまで公開するウィンドウ
- データキャッシュ（`cache = "id-cache"`）や密結合メモリ（`tcm`）があり、その種類の生成器がバウンスバッファやリージョンを配置しないマイコン

ウィンドウはデバイスメモリです（キャッシュしません）。コード、データ、スタックは通常のメモリです。

## 生成されるファイル（`nodes/<id>/dagon/`）

| ファイル | 内容 |
|---|---|
| `boot_wiring.rs` | DAGONのブート段階があるマイコン: ボードのLED、停止のパターン、電源の保持、ブートのアクションプログラム。ウォッチドッグの周期。ブート記録とヒューズの配置。隔離のレベル、種類、上限、ブート時に再読込するレジスタ。チェーンの順序の拡張（`BootChain`、`BOOT_CHAIN`）と、ブートローダのスロットごとの保護マップ |
| `memory-<image>-<slot>.x` | RAMを定義済のマイコン: 拡張のリージョンを除外したイメージのFLASHとRAM、拡張ごとのリージョンとセクション、ブート記録とヒューズのセル |
| `protection.toml` | 全イメージの拡張、メモリ、保護マップ。可読形式です |

`nodes/<id>/dagon/`には最後の`dagon gen`の出力だけがあります。そのフォルダの他のファイルのうち、genの目印（dagon genの生成物である旨の記載が先頭行にある）があるものは削除します（目印がないものは残存させて報告します）。genが拒否した、または生成に失敗したノードには、genのファイルを残存させません。`nodes`、`nodes/<id>`、`nodes/<id>/dagon`がシンボリックリンクかジャンクションの場合、genはそのフォルダへの出力も削除もせず、その旨を報告します。

レベルが`none`の場合はマップを作成せず、`dagon gen`はノードの拡張が保護なしで動作することを通知します。

## ブート拡張

DAGONのブート段階があるマイコン（RP2040、STM32F401RE）では、`dagon gen`はノードのブート用crateもプロジェクトの`boot/<id>/`に出力します。生成元はマイコンの定義にあるブートローダのテンプレート（`bootcore/template/`。`[package.metadata.dagon] bootcore = "<mcu id>"`で検出します）です。出力対象は`Cargo.toml`（ブートローダのライブラリ、`dagon-events`、各ブート拡張のcrate、そして`[package.metadata.dagon.boot]`配下のノード、マイコン、ボード、信頼された拡張）、`boot.rs`（ブートローダの`main`。`nodes/<id>/dagon/boot_wiring.rs`から生成）、`graph.stamp`（後述）、`.cargo/config.toml`（ターゲット、`DAGON_BOOT_CRATE`、ノードの`DAGON_TARGET_TAG`）です。全マイコンで、全ノードの`boot/<id>/mcu.toml`も出力します。これはノードのファームウェアのビルドが読込するマイコンの定義です（[マイコンとボードの定義](/reference/definitions/)）。独立したフォルダにする理由は、cargoがcrateより上位の全フォルダの`.cargo/config.toml`を統合して読込し、ノードのアプリのcrateにはアプリ用の設定があるためです。`boot/<id>/`には`nodes/<id>/dagon/`と同様に、最後の`dagon gen`の出力だけがあります。そのフォルダのcargoの`target/`と`Cargo.lock`は変更しません。利用者が決定する欄が1つあります。`boot/<id>/Cargo.toml`の`[package]`配下の`version`、つまりブートローダのバージョンです。`dagon gen`はテンプレートの値を1度だけ出力し、以後は利用者の値を維持します。ブートローダのリリースごとに値を増加してください（ノードは、動作中のものとバージョンが異なるブートローダを書込します）。`dagon gen`とビルドは`Cargo.lock`にあるそのcrate自身の項目をこの値に統一し、ロックの他の部分は変更しません。そのファイルの読込か解析に失敗した場合、`dagon gen`は理由を表示して停止し、ファイルは無変更です。ノードの処理を省略する場合は、他のブート用crateのファイルは削除しますが、このファイルは残存させます。ビルドはそのフォルダで`cargo build --release`を実行します（`DAGON_PUBKEY`は全イメージで同一です）。

ブートローダは拡張を特権で実行するため、信頼された拡張（マニフェストが`trusted = true`で、`[extensions.nodes.<id>] trusted`にソースのハッシュと共に列挙したもの）だけを実行します。`boot.watchdog_armed`から`boot.before_jump`または`boot.halt`までの各ブートイベント（[イベント](/reference/events/)、レイヤー`boot`）は、そのイベントを`observes`に列挙した拡張に、チェーンの順序で到達します。呼出ごとに`max_run_us`と照合して時間を計測します。超過した呼出は停止しません。ただしブートローダはその後に`boot.extension_overran`（拡張、その割当時間、所要時間）を、その種類を観測する拡張に通知し、処理を継続します。ブート拡張の処理内容に関係なく、ブートローダが起動するスロットは不変です。呼出の合計が試行用ウォッチドッグ周期の4分の1を超過する可能性がある組合せは、`dagon gen`が拒否します。計上するのは観測するブートイベントごとに1回の呼出と、`boot.extension_overran`を観測する拡張では、いずれかのブート拡張の呼出ごとに追加の1回です。

`dagon gen`はブート用crateの解決済の依存グラフもcargoで検証します（全フィーチャ、全ターゲット）。組込対象の全crateは、DAGONのcrate、宣言済の信頼された拡張のフォルダ内のcrate、またはブート用crateの`Cargo.lock`にチェックサムがあるレジストリのcrateのいずれかである必要があります。その後に、ブート用crateの`Cargo.toml`、`Cargo.lock`、信頼されたソースの最新の内容に対して`boot/<id>/graph.stamp`を出力します。このスタンプが不在の場合と、出力後にいずれかが変化した場合は、ブートローダのビルドは実行しません（`dagon gen`を再度実行してください）。ブートに判断点はありません。ブートローダが起動するスロットはDAGONだけが決定するため、拡張による変更は不可能です。呼出はブートローダのフラッシュの上限とウォッチドッグを共有します。復帰しない呼出があると、ウォッチドッグがチップをリセットします。

`boot/<id>/Cargo.lock`の作成後は`dagon gen`がグラフを`--locked`で解決します。そのため拡張の追加後はロックの段階で停止します。`boot/<id>/`で`cargo metadata --offline --format-version 1`を1度実行し（ロックに新規のcrateを追加します）、その後に`dagon gen`を再度実行してください。拡張なしでブートローダをビルドする場合は、ノードの`boot`と`trusted`の一覧からその拡張を除外し、`dagon gen`を再度実行します。

ブートのイベントは、ブートローダの処理と同一の速度で到来します。`boot.watchdog_armed`、`boot.log_read`、`boot.before_decision`は、間に状態ログのフラッシュの読出だけを挟んで連続します。`boot.slot_chosen`または`boot.no_image`はイメージの検査後に発生し、`boot.before_jump`はアプリが起動してチップを引継ぐ直前に到来します。拡張の表示内容はその時点までしか残存せず、ハンドラ内の待機は全部ブートの時間に加算します。

コピーして使用可能な最小の信頼されたブート拡張は`targets/boot-extension-template/`です（ブートのイベントを計数し、ブートローダが選択したスロットを記憶します）。

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

DAGONはブート拡張にペリフェラルもピンも提供しません（`Ctx`にあるのはレイヤーだけです）。ハードウェア（ディスプレイ、ピン）を操作する信頼された拡張は、ブートローダのHALの初期化後に、`on_event`の内部でマイコンのHALから自身で取得します（特権で動作するためです）。取得対象とその安全性は、拡張の作者の責任です。DAGONはいずれも検証しません。拡張はブートローダの使用対象（RP2040では、フラッシュ、ウォッチドッグ、クロック、ブートローダが停止した後のボードの`[boot] led`）にアクセスせず、各呼出を`max_run_us`以内に収める必要があります。ESP32-S3にはDAGONのブート段階がありません（ベンダーの第2段階がアプリを起動します）。そのためブート拡張は存在せず、`dagon gen`はその理由を表示して拒否します。
