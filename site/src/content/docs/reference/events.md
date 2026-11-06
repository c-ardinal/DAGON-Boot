---
title: イベント
description: DAGON が出すすべてのイベントです。
editUrl: false
---

:::note
状態機械のカタログから生成しています。直接編集せず、
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs` でもう一度生成してください。
:::

DAGON の状態の変化は、どれも状態機械の遷移で、遷移はどれもイベントを 1 つ出します。
このページは状態機械の宣言から作っているので、すべてのイベントが載っています。

イベントの形式: 1（`dagon_events::EVENT_FORMAT`）。種類やペイロードのフィールドを足しても
番号は変わりません。名前を変える、消す、意味を変えるときは番号が変わります。読む側は、
知らない種類とフィールドを読み飛ばし、読めない形式の番号は受け付けません。

## 表の見方

- **種類**: `layer.kind`。ペイロードの型名を snake case にしたものです。
- **コード**: そのレイヤーの中での種類の番号です（`EventRecord.kind`）。
- **遷移**: イベントを出すメソッドと、移る前後の状態です（`*` はどの状態でも）。
- **ペイロード**: フィールドと、記録の中でのバイト数です（`EventRecord.payload`、順に並び、
  合わせて 16 バイトまで）。
- **受け取り**: *live* は同じイメージの拡張が起きたときに、*replay* は起動時にアプリのエージェントが
  `replayed: true` を付けて出し直したものを、*host* は CLI、serve のイベント、`--events`、
  アクションが受け取ります。
- **備考**: *action* はブートのアクションプログラムを動かせます。*retired* はもう出さず、
  次のイベントの形式まで残しています。

## 判断点

ここに無いものは通知です。答えは、チェーンの順（公開元、ボードの定義、ライブラリ、利用者）で
最初のものが使われます。どの拡張も答えなければ、既定の値を使います。

| 判断 | レイヤー | 問い | 答える側 | 時間の上限 | 既定 | してはいけないこと |
|---|---|---|---|---|---|---|
| `accept_update` | app | `UpdateOffer` -> `Accept` | ボードの定義、ライブラリ、利用者（最初の答えが勝つ） | 呼び出しごとに `DECISION_BOUND_MS` 以内（遅れた答えは無視します）。DAGON のすべてのガードを通った後、消去の前に尋ねます | 受け入れる | DAGON が拒んだものを受け入れること（そのときは尋ねません） |
| `allow_reboot_now` | app | `RebootRequest` -> `Delay` | ボードの定義、ライブラリ、利用者（最初の答えが勝つ） | 呼び出しごとに `DECISION_BOUND_MS` 以内。`Now` か `After(ms)`、ms の上限はマイコンの定義の短いほうのウォッチドッグの周期の半分。DAGON はまず返答し、待つあいだキックを続けます | すぐ | 再起動を拒むこと。上限を超える遅れ |
| `self_check` | app | `TrialInfo` -> `Health` | ボードの定義、ライブラリ、利用者（最初の答えが勝つ） | 呼び出しごとに `DECISION_BOUND_MS` 以内（遅れた答えは無視します）。`Pending` は "state" のステータスのままで、ホストが再試行します | 健全 | 試行中でないスロットを確定すること。`confirmed` の遷移の外で確定すること |
| `confirm_plan` | host | デプロイの一覧 -> はいかいいえ | 対話する利用者（ターミナル、ページ）か `--yes`。アクションやホストの拡張は答えません | 利用者が答えるまで | 尋ねる | 一覧の表示の前に行う、鍵の確認とイメージの検証を飛ばすこと |

## レイヤー `boot`

### 状態機械 `boot_extensions`

ブート段階の拡張の呼び出しとその時間の上限。

状態: `Ready` (最初の状態)。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `boot.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (8 B), `budget_us: u32` (4 B), `us: u32` (4 B) | live |  | 拡張のハンドラが `budget_us`（マニフェストの `max_run_us`）より長く動いた。ブートは同じ判断で進んだ。 |

### 状態機械 `sequence`

有効にしたウォッチドッグからジャンプか停止までのブートローダの手順。そのイベントは起きたときにブートローダの拡張に届く。拡張が何をしても判断は変わらない。

状態: `Started` (最初の状態), `WatchdogArmed`, `LogRead`, `BeforeDecision`, `Chosen`, `NoImage`, `Jumping`, `Halted`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `boot.watchdog_armed` | 0x01 | `watchdog_armed`: Started → WatchdogArmed | `period_ms: u32` (4 B) | live |  | ウォッチドッグが試行の周期で動く。ここから拡張が動く。 |
| `boot.log_read` | 0x02 | `log_read`: WatchdogArmed → LogRead | `ok: bool` (1 B), `pending: bool` (1 B), `confirmed: bool` (1 B), `attempts: u8` (1 B) | live |  | 状態ログを読んだ（`ok: false`: 有効な面が無い）。 |
| `boot.before_decision` | 0x03 | `before_decision`: LogRead → BeforeDecision | `pending: bool` (1 B), `confirmed: bool` (1 B), `attempts: u8` (1 B) | live |  | 判断の前の最後のイベント。 |
| `boot.slot_chosen` | 0x04 | `slot_chosen`: BeforeDecision → Chosen | `slot: u8` (1 B), `trial: bool` (1 B) | live |  | 起動するスロットが決まった（`trial`: 数える試行での待機のスロット）。 |
| `boot.no_image` | 0x05 | `no_image`: BeforeDecision → NoImage | – | live |  | 起動できるスロットが無い。 |
| `boot.before_jump` | 0x06 | `before_jump`: Chosen → Jumping | `slot: u8` (1 B), `trial: bool` (1 B) | live |  | 制御が DAGON を離れる前の最後のイベント。 |
| `boot.halt` | 0x07 | `halt`: NoImage → Halted | – | live |  | ブートローダはここにとどまる。 |

## レイヤー `app`

### 状態機械 `decisions`

拡張への判断の呼び出しとその上限。

状態: `Ready` (最初の状態)。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (8 B), `point: DecisionId` (1 B), `us: u32` (4 B) | live |  | 拡張が `DECISION_BOUND_MS` の後で判断点に答えた。その答えは無視した。 |

### 状態機械 `image`

イメージのスロット 1 つのライフサイクル（2 つの APP のスロットと、親機の構成のコピー）。スロットのストアの変更メソッドは、これらの遷移の中でだけ動く。`trial_started` と `rolled_back` はブートローダの変更で、起動時にスロットのストアからリプレイする。

状態: `Empty` (最初の状態), `Written`, `Pending`, `Trial`, `Confirmed`, `Reverted`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.transfer_started` | 0x20 | `erase`: Empty \| Written \| Pending \| Reverted → Empty | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | 新しいイメージのためにスロットを消去した（動いているスロットや確定したスロットは消さない）。 |
| `app.written` | 0x21 | `written`: Empty → Written | `target: u8` (1 B), `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 本体を検証し、ヘッダを書いて読み返した。スロットにイメージがある。 |
| `app.pending_set` | 0x22 | `pending_set`: Written \| Pending \| Confirmed \| Reverted → Pending | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 次のブートで試すスロットになった。 |
| `app.pending_moved` | 0x23 | `pending_moved`: Pending \| Trial → Written | `from: u8` (1 B), `to: u8` (1 B) | live, host |  | 別のスロットが待機になった。このスロットはもう待機ではない。 |
| `app.trial_started` | 0x24 | `trial_started`: Pending \| Trial → Trial | `slot: u8` (1 B), `version: Version` (4 B), `attempt: u8` (1 B), `max: u8` (1 B) | replay, host |  | ブートローダが待機のスロットを試行として起動した（これまで `attempt` 回のブートで、ブートローダが許すのは `max` 回）。 |
| `app.confirmed` | 0x25 | `confirmed`: Trial → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 試行中のスロットを、自己検査の後で確定した。 |
| `app.demoted` | 0x26 | `demoted`: Confirmed → Written | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | もう一方のスロットが確定した。このスロットはもう確定ではない。 |
| `app.rolled_back` | 0x27 | `rolled_back`: Pending \| Trial → Reverted | `from: u8` (1 B), `to: u8` (1 B), `reason: u8` (1 B) | replay, host |  | ブートローダがスロットをロールバックし（試行回数を使い切った、または待機のイメージが検査に失敗した）、`to` を起動した。 |
| `app.pending_withdrawn` | 0x28 | `pending_withdrawn`: Pending \| Trial → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | live, replay, host |  | 確定したスロットがもう一度待機にされていた。別のスロットが待機になったか、ブートローダがその試行を終えたので、確定したスロットに戻る。 |

### 状態機械 `core_image`

stage0 のある self-boot のマイコンの、ブートローダのスロット 1 つのライフサイクル（定義の `bootcore` のスロット）。状態ログのブートローダの記録に持つ。エージェントが新しいコアを空いているスロットに書いて待機にし、stage0 がそれを試し、コアが自分で試行を決着させる。新しいコアが自分の検査の後で `confirmed`、stage0 が新しいコアをあきらめたときは古いコアが `rolled_back`。アプリは次の起動でそれらを見る。

状態: `Empty` (最初の状態), `Written`, `Pending`, `Confirmed`, `Reverted`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.core_transfer_started` | 0x40 | `erase`: Empty \| Written \| Pending \| Reverted → Empty | `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | 新しいコアのために空いているブートローダのスロットを消去した（動いているコアや確定したコアのスロットは消さない）。 |
| `app.core_written` | 0x41 | `written`: Empty → Written | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | コアの本体を検証し、そのヘッダを書いて読み返した。 |
| `app.core_pending_set` | 0x42 | `pending_set`: Written \| Pending \| Reverted → Pending | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 次のリセットで stage0 が試すコアになった。 |
| `app.core_confirmed` | 0x43 | `confirmed`: Pending → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | host |  | 新しいコアが自分の検査を通り、自分を確定した。 |
| `app.core_demoted` | 0x44 | `demoted`: Confirmed → Written | `slot: u8` (1 B), `version: Version` (4 B) | host |  | 別のコアが自分を確定した。このコアはもう確定ではない。 |
| `app.core_rolled_back` | 0x45 | `rolled_back`: Pending → Reverted | `from: u8` (1 B), `to: u8` (1 B), `reason: u8` (1 B) | replay, host |  | stage0 が待機のコアをあきらめ（起動の回数が尽きた、またはイメージが検査に失敗した）、`to` のコアがそれを記録した。 |

### 状態機械 `session`

書き込みのセッション: 申し出、ヘッダ、本体、終わり。

状態: `Idle` (最初の状態), `Header`, `Body`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.update_offered` | 0x10 | `update_offered`: Idle → Header | `target: u8` (1 B), `slot: u8` (1 B), `body_len: u32` (4 B) | live, host |  | 書き込みの申し出が DAGON のすべてのガードを通った。次に `accept_update` に尋ねる。 |
| `app.header_received` | 0x11 | `header_received`: Header → Header | `got: u16` (2 B) | live |  | これまでに受け取ったヘッダのバイト。 |
| `app.body_opened` | 0x12 | `body_opened`: Header → Body | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live |  | スロットを消去し、ヘッダ（ターゲットが持つところでは）を受け付けた。ここから本体のチャンクを受け取る。 |
| `app.transfer_progress` | 0x13 | `data_written`: Body → Body | `done: u32` (4 B), `total: u32` (4 B) | live, host |  | 本体のチャンクを書いた。 |
| `app.verify_started` | 0x14 | `verify_started`: Body → Idle | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live |  | 本体全体が届いた。次にハッシュして読み返す。 |
| `app.transfer_aborted` | 0x15 | `aborted`: Header \| Body → Idle | `cause: AbortCause` (1 B) | live, host |  | 転送を捨てた。 |

### 状態機械 `node`

ノードの起動とその DAGON のリンク: マイコンの定義のコードの起動と応答のループがエージェントを通して報告するもの。

状態: `Off` (最初の状態), `Up`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.starting` | 0x01 | `starting`: * → Up | `tag: NameDigest` (8 B), `role: u8` (1 B), `mode: u8` (1 B) | live |  | DAGON の起動が始まった（エージェントを作った、または再開した）。次にスロットのストアを読む。`tag`: ノードのイメージのターゲットのタグの名前のダイジェスト（`NameDigest`: 短い形の「ダイジェストの先頭」で、マイコンの識別と同じように作る）。ホストは構成のノードのタグから解決する。 |
| `app.watchdog_taken` | 0x02 | `watchdog_taken`: Up → Up | `period_ms: u32` (4 B), `trial_policy: TrialPolicy` (5 B) | live |  | 起動がブートローダのウォッチドッグを引き継いだ: その周期と、イメージが試行のあいだガードがそれをどう扱うか。 |
| `app.bring_up` | 0x07 | `bring_up`: Up → Up | `board: NameDigest` (8 B) | live |  | ボードの立ち上げを実行した。`board`: ボードの定義の id の名前のダイジェスト（`NameDigest`）。ホストは知っているボードの定義から解決する。 |
| `app.link_up` | 0x03 | `link_up`: Up → Up | `link: u8` (1 B), `addr: u8` (1 B) | live |  | DAGON のリンクがフレームを送受信する: 起動時か、リンクが切れた後の最初のフレームが届いた。`link`: その種類のコード（`dagon_format::with_link_kinds!`）。`addr`: そのリンクでのノード自身のアドレス（アドレスの無いリンクでは `NO_ADDR`）。 |
| `app.link_down` | 0x04 | `link_down`: Up → Up | `link: u8` (1 B), `addr: u8` (1 B) | live |  | DAGON のリンクがフレームを送受信しなくなった: 返答を送れなかった、またはリンクが閉じた。 |
| `app.request` | 0x05 | `request`: Up → Up | `protocol: u8` (1 B), `cmd: u8` (1 B), `seq: u8` (1 B), `via: u8` (1 B) | live |  | リンク `via` にリクエストが届いた。 |
| `app.replied` | 0x06 | `replied`: Up → Up | `status: u8` (1 B), `ms: u32` (4 B) | live |  | 最後のリクエストへの返答が、届いてから `ms` の後に出た。 |

### 状態機械 `requests`

答えたか拒んだリクエストと、尋ねた判断点。

状態: `Ready` (最初の状態)。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.store_opened` | 0x30 | `store_opened`: Ready → Ready | `active: u8` (1 B), `trial: bool` (1 B), `version: Version` (4 B), `attempts: u8` (1 B), `last_reason: u8` (1 B) | live |  | 起動時にスロットのストアを読んだ。 |
| `app.hello` | 0x31 | `hello`: Ready → Ready | – | live, host |  | 識別に答えた。 |
| `app.status_asked` | 0x32 | `status_asked`: Ready → Ready | – | live, host |  | ステータスに答えた。 |
| `app.update_refused` | 0x33 | `update_refused`: Ready → Ready | `status: u8` (1 B), `by_extension: bool` (1 B) | live, host |  | 書き込みの申し出を、DAGON のガードか拡張の `accept_update` が拒んだ。 |
| `app.header_verified` | 0x34 | `header_verified`: Ready → Ready | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | ヘッダの署名、鍵、互換性を確かめた。 |
| `app.header_rejected` | 0x35 | `header_rejected`: Ready → Ready | `status: u8` (1 B) | live, host |  | ヘッダを拒んだ（署名、鍵、互換性、大きさ）。 |
| `app.verify_ok` | 0x36 | `verify_ok`: Ready → Ready | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | 読み返した本体がダイジェストと一致した。 |
| `app.verify_failed` | 0x37 | `verify_failed`: Ready → Ready | `status: u8` (1 B) | live, host |  | 本体かそのヘッダを検証できなかった、または保存できなかった。 |
| `app.pending_refused` | 0x38 | `pending_refused`: Ready → Ready | `status: u8` (1 B) | live, host |  | ブートの対象の切り替えを拒んだ。 |
| `app.self_check` | 0x39 | `self_check`: Ready → Ready | `slot: u8` (1 B), `version: Version` (4 B) | live |  | 試行中のスロットの確定が `self_check` に尋ねる。 |
| `app.confirm_refused` | 0x3a | `confirm_refused`: Ready → Ready | `status: u8` (1 B) | live, host |  | 確定を拒んだ（試行中でない、自己検査、ストア）。 |
| `app.reboot_requested` | 0x3b | `reboot_requested`: Ready → Ready | `mode: u8` (1 B), `delay_ms: u32` (4 B) | live, host |  | `allow_reboot_now` の後で、再起動を受け付けた。 |
| `app.before_reboot` | 0x3c | `before_reboot`: Ready → Ready | – | live |  | 返答が出た。リセットが続く。 |
