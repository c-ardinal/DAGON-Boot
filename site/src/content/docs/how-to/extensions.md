---
title: "拡張の作成"
description: "アプリで書込のイベントを観測して判断点に応答し、ポートを置換します。"
---

拡張は、DAGONが呼出す利用者自身のRustコードです。
拡張の動作箇所は2つです: ノードのアプリと、ホスト（ビルド、署名、書込を実行するPC）です。

DAGONは拡張を隔離しません。
拡張は周囲のコードと同一の権限で動作します。アプリでは、アプリが到達可能な全資源に到達し、ホストでは利用者の権限で動作します。
拡張の内容、依存するcrate、取込むソースの審査は利用者の責任です（[制限事項](/explanation/limitations/)を参照）。

## アプリでの拡張

アプリの拡張の機能は2つです。

- ノードの全`app.*`イベントを観測します（[イベント](/reference/events/)）。
- 判断点に応答します: 書込の開始可否、即時の再起動可否、試行中のバージョンの健全性です。

### 1. 依存関係の追加

ノードの`Cargo.toml`に`dagon-events`、`dagon-agent`、`static_cell`を追加します。
DAGONの2つのパッケージは、同一ファイルの`dagon-mcu-...`の行と同一のソースを指定します（同一の`git`と`tag`、または同一のDAGONソース内の`path`）。

```toml nocheck
dagon-events = { git = "<as the dagon-mcu line>", tag = "<as the dagon-mcu line>" }
dagon-agent = { git = "<as the dagon-mcu line>", tag = "<as the dagon-mcu line>" }
static_cell = "2"
```

`dagon-events`は`Extension`トレイトを提供します。
`dagon-agent`はイベントのペイロードの型（`dagon_agent::image::Confirmed`など）を提供します。

### 2. 拡張の記述

必須のメソッドは`id`だけです。
他のメソッドには既定の実装があります。`on_event`は無処理で、判断のメソッドは`None`（棄権）を返却します。

```rust
use dagon_agent::image;
use dagon_events::{Accept, Ctx, Delay, Event, Extension, Health, RebootRequest, TrialInfo, UpdateOffer};

/// Keeps an update away while the motor runs, and refuses to confirm a
/// new version whose sensor does not answer.
pub struct Machine {
    pub motor_running: fn() -> bool,
    pub sensor_ok: fn() -> bool,
}

impl Extension for Machine {
    fn id(&self) -> &'static str {
        "machine"
    }

    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        if let Some(c) = e.get::<image::Confirmed>() {
            defmt::info!("slot {} confirmed", c.slot);
        }
    }

    fn accept_update(&mut self, _: &UpdateOffer, _: &Ctx<'_>) -> Option<Accept> {
        (self.motor_running)().then_some(Accept::No)
    }

    fn allow_reboot_now(&mut self, _: &RebootRequest, _: &Ctx<'_>) -> Option<Delay> {
        (self.motor_running)().then_some(Delay::After(2_000))
    }

    fn self_check(&mut self, _: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        (!(self.sensor_ok)()).then_some(Health::Unhealthy)
    }
}
```

`e.get::<T>()`は、イベントの種類が`T`の場合にペイロードを返却します。
イベントの種類とフィールドは[イベント](/reference/events/)に記載しています。

### 3. DAGONへの登録

`dagon new`で作成したプロジェクトの`src/app.rs`には、アプリがDAGONの開始処理に提供する値を返却する関数`dagon()`があります。
拡張を`static`に配置し、`exts`に指定します。

```rust
use dagon_events::{extensions, Chain, Nop};
use static_cell::StaticCell;

static EXTS: StaticCell<Chain<Machine, Nop>> = StaticCell::new();

pub fn dagon() -> App {
    let exts = EXTS.init(extensions![Machine { motor_running, sensor_ok }]);
    App { exts: Some(exts), ..App::new(self_check) }
}
```

`extensions![a, b, c]`は複数の拡張を1つのチェーンに合成します。型は`Chain<A, Chain<B, Chain<C, Nop>>>`です。
`dagon gen`は`src/app.rs`を再生成しません。

### 判断点

| メソッド | DAGONの照会時点 | 応答 | 応答なしの場合 |
|---|---|---|---|
| `accept_update` | 書込のリクエストがDAGONの全ガードに合格した後、スロットの消去前 | `Accept::Yes`、`Accept::No` | 書込を開始します |
| `allow_reboot_now` | DAGONが再起動を受理した時点 | `Delay::Now`、`Delay::After(ms)` | 即時に再起動します |
| `self_check` | 試行中のバージョンの確定を要求された時点 | `Health::Healthy`、`Health::Unhealthy`、`Health::Pending` | `src/app.rs`の`self_check()`が応答します |

- DAGONは最初にボードの拡張へ照会し、次に利用者の拡張へ`extensions![..]`の記述順に照会します。最初の応答を採用します。
- `src/app.rs`の`self_check()`は最後に照会され、必ず応答します。そのため、拡張が`None`を返却した場合の判断は`self_check()`が担当します。
- 応答の期限は`DECISION_BOUND_MS`（50 ms）です。期限超過の応答は無視され、`app.extension_overran`がその拡張を通知します。メソッド内での待機やブロックは禁止です。状態を事前に準備し、即時に返却してください。
- `Accept::No`を後続の拡張は変更不可です。また、DAGON自身のガードが拒否した対象を、拡張の応答で受理させる方法はありません。署名、ターゲット、ダイジェストが不正なイメージは`accept_update`に到達しません。
- `Delay::After`の上限は、ノードのウォッチドッグの周期で決定します。
- `Health::Pending`の場合、バージョンは試行中を継続し、ホストが再度照会します。確定に至らない試行中のバージョンはロールバックされます。

### タスクからの描画とログ出力

拡張のメソッドはDAGONの通信処理のタスク内で動作するため、短時間での返却が必要です。
時間を要する処理（ディスプレイ、低速なリンクへのログ出力）には、開始処理が返却するイベント（`AppEvents`）を利用者自身のタスクから使用します。タスクは最新の状態を待機し、直近のイベントを読出します。
[ライフサイクルイベントと拡張](/explanation/lifecycle-events/)の8.1節に、ディスプレイのタスクの例があります。

## ポートの置換

ポートは、Rustのトレイトで定義したDAGONの部品で、DAGON自身の実装が既定です。
2つのポートはコードなしで置換可能です。他のポートは、DAGONのcrateを使用する利用者自身のプログラムで置換します。`dagon`コマンドは既定の実装を使用します。

| ポート | トレイト | DAGONの既定 | 置換方法 |
|---|---|---|---|
| `KeyStore` | `dagon_sign::KeyStore` | キーファイル | コードなし: `DAGON_SIGNING_KEY_COMMAND`に、キーを出力するコマンドを設定します（[導入から書込まで](/getting-started/first-project/)を参照）。利用者自身のプログラム: `key()`を実装します。 |
| `Agent` | `dagon_host::ports::Agent` | エージェント定義の、接続済のエージェント | コードなし: エージェント定義のファイルを追加します（[エージェント定義の形式](/reference/agent-definition/)）。利用者自身のプログラム: `FnMut(&Prompt) -> Result<String, String>`は全て`Agent`です。 |
| `Signer` | `dagon_sign::Signer` | メモリ上のEd25519のキーペア | `public_key()`と`sign()`を実装し、`dagon_sign::image::sign_image`と`dagon_sign::release::assemble`に指定します。 |
| `BuildRunner` | `dagon_host::ports::BuildRunner` | `cargo build` | `build()`を実装します。`build()`はノードのイメージを1つビルドし、実行ファイルを返却します。 |
| `CryptoProvider`、`Digest` | `dagon_format::CryptoProvider`、`dagon_format::Digest` | ソフトウェアのSHA-256とSHA-512（`dagon_format::Software`） | 2つのハッシュを実装し、`dagon_agent::Agent::with_crypto`と`dagon_ed25519::Ed25519Small::with_crypto`に指定します。 |
| `DecisionPolicy` | `dagon_core::boot::DecisionPolicy` | 待機のスロットを確定済のスロットより優先し、最大`max_attempts`回試行 | `trial()`を実装し、`dagon_core::boot::decide_with`に指定します。 |

キーをHSM内に保持する署名器の例です。

```rust
use dagon_sign::Signer;

pub struct Hsm {
    pub public_key: [u8; 32],
}

impl Signer for Hsm {
    fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    /// Ed25519 over `domain` followed by `message`.
    fn sign(&self, domain: &[u8], message: &[u8]) -> [u8; 64] {
        hsm_sign_ed25519(&[domain, message].concat())
    }
}
```

`hsm_sign_ed25519`は、利用者のHSMの呼出を表現しています。

置換したポートは、既定の実装と同一の動作を維持する必要があります。`Signer`は、`public_key()`が返却するキーのEd25519で、`domain`と後続の`message`に署名します。`CryptoProvider`は、ソフトウェアの実装と同一のダイジェストを返却します。
DAGONは、置換した実装と既定の実装を照合しません。ノードは全イメージの署名とダイジェストを自身のコードで検証するため、署名やハッシュが不正なホストのポートが作成したイメージを、ノードは拒否します。

## 拡張の使用前の確認

- 拡張のソースと、依存するcrateを審査します。拡張は、アプリの権限、またはホストの利用者アカウントの権限で動作します。
- アプリでは、各メソッドが期限内に返却し、バス、ロック、タイマーを待機しない点を確認します。
- 拡張自身の入力が欠落した場合の`accept_update`と`self_check`の応答を確認します。`None`は判断を次の拡張に委任し、`Accept::No`と`Health::Unhealthy`は書込や確定を中止します。
- ボード上で、確定に至る書込と、ロールバックに至る書込を1回ずつ実行します。
