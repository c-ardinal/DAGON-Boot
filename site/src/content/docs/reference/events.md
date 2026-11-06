---
title: イベント
description: DAGON が出すすべてのイベントです。
editUrl: false
---

:::note
状態機械のカタログから生成しています。直接編集せず、
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs` でもう一度生成してください。
判断点の表の中身と、状態機械とイベントの説明は、元のソースのままで英語です。
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

ここに無いものは通知です。答えは、チェーンの順（公開元、ボードパッケージ、ライブラリ、利用者）で
最初のものが使われます。どの拡張も答えなければ、既定の値を使います。

| 判断 | レイヤー | 問い | 答える側 | 時間の上限 | 既定 | してはいけないこと |
|---|---|---|---|---|---|---|
| `accept_update` | app | `UpdateOffer` -> `Accept` | board package, libraries, user (first answer wins) | each call within `DECISION_BOUND_MS` (a later answer is ignored); asked after every DAGON guard passed, before erase | accept | accepting what DAGON refused (not asked then) |
| `allow_reboot_now` | app | `RebootRequest` -> `Delay` | board package, libraries, user (first answer wins) | each call within `DECISION_BOUND_MS`; `Now` or `After(ms)`, ms capped at half the shorter watchdog period of the MCU definition; DAGON replies first and keeps feeding while it waits | now | refusing a reboot; a delay beyond the cap |
| `self_check` | app | `TrialInfo` -> `Health` | board package, libraries, user (first answer wins) | each call within `DECISION_BOUND_MS` (a later answer is ignored); `Pending` keeps the "state" status and the host retries | healthy | confirming a slot not on trial; confirming outside the `confirmed` transition |
| `confirm_plan` | host | the deploy listing -> yes or no | the interactive user (terminal, page) or `--yes`; never actions or host extensions | until the user answers | ask | skipping the key check and image verification, done before the listing is shown |

## レイヤー `boot`

### 状態機械 `boot_extensions`

Boot-stage extension calls and their time limits.

状態: `Ready` (最初の状態)。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `boot.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (8 B), `budget_us: u32` (4 B), `us: u32` (4 B) | live |  | An extension's handler ran longer than its `budget_us` (the manifest's `max_run_us`); the boot went on with the same decision. |

### 状態機械 `sequence`

The boot core's sequence from the armed watchdog to the jump or the halt. Its events reach the boot core's extensions as they happen; nothing an extension does changes the decision.

状態: `Started` (最初の状態), `WatchdogArmed`, `LogRead`, `BeforeDecision`, `Chosen`, `NoImage`, `Jumping`, `Halted`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `boot.watchdog_armed` | 0x01 | `watchdog_armed`: Started → WatchdogArmed | `period_ms: u32` (4 B) | live |  | The watchdog runs with the trial period; from here extensions run. |
| `boot.log_read` | 0x02 | `log_read`: WatchdogArmed → LogRead | `ok: bool` (1 B), `pending: bool` (1 B), `confirmed: bool` (1 B), `attempts: u8` (1 B) | live |  | The state log was read (`ok: false`: no valid face). |
| `boot.before_decision` | 0x03 | `before_decision`: LogRead → BeforeDecision | `pending: bool` (1 B), `confirmed: bool` (1 B), `attempts: u8` (1 B) | live |  | The last event before the decision is made. |
| `boot.slot_chosen` | 0x04 | `slot_chosen`: BeforeDecision → Chosen | `slot: u8` (1 B), `trial: bool` (1 B) | live |  | The slot to start is final (`trial`: the pending slot on a counted attempt). |
| `boot.no_image` | 0x05 | `no_image`: BeforeDecision → NoImage | – | live |  | No slot can be started. |
| `boot.before_jump` | 0x06 | `before_jump`: Chosen → Jumping | `slot: u8` (1 B), `trial: bool` (1 B) | live |  | The last event before control leaves DAGON. |
| `boot.halt` | 0x07 | `halt`: NoImage → Halted | – | live |  | The boot core stays here for good. |

## レイヤー `app`

### 状態機械 `decisions`

Decision calls to extensions and their bound.

状態: `Ready` (最初の状態)。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (8 B), `point: DecisionId` (1 B), `us: u32` (4 B) | live |  | An extension answered a decision point after `DECISION_BOUND_MS`; its answer was ignored. |

### 状態機械 `image`

The lifecycle of one image slot (the two APP slots, and the hub's topology copy). The slot store's mutators run only inside these transitions. `trial_started` and `rolled_back` are the boot core's changes, replayed at start from the slot store.

状態: `Empty` (最初の状態), `Written`, `Pending`, `Trial`, `Confirmed`, `Reverted`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.transfer_started` | 0x20 | `erase`: Empty \| Written \| Pending \| Reverted → Empty | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | The slot was erased for a new image (never the running or the confirmed slot). |
| `app.written` | 0x21 | `written`: Empty → Written | `target: u8` (1 B), `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The body verified and the header written and read back: the slot holds an image. |
| `app.pending_set` | 0x22 | `pending_set`: Written \| Pending \| Confirmed \| Reverted → Pending | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The slot is the one tried on the next boot. |
| `app.pending_moved` | 0x23 | `pending_moved`: Pending \| Trial → Written | `from: u8` (1 B), `to: u8` (1 B) | live, host |  | Another slot was set pending; this one no longer is. |
| `app.trial_started` | 0x24 | `trial_started`: Pending \| Trial → Trial | `slot: u8` (1 B), `version: Version` (4 B), `attempt: u8` (1 B), `max: u8` (1 B) | replay, host |  | The boot core started the pending slot on trial (`attempt` boots so far, of the `max` its boot core allows). |
| `app.confirmed` | 0x25 | `confirmed`: Trial → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The slot on trial was confirmed after its self-check. |
| `app.demoted` | 0x26 | `demoted`: Confirmed → Written | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The other slot was confirmed; this one is no longer. |
| `app.rolled_back` | 0x27 | `rolled_back`: Pending \| Trial → Reverted | `from: u8` (1 B), `to: u8` (1 B), `reason: u8` (1 B) | replay, host |  | The boot core rolled the slot back (attempts exhausted, or the pending image failed its check) and started `to`. |
| `app.pending_withdrawn` | 0x28 | `pending_withdrawn`: Pending \| Trial → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | live, replay, host |  | The confirmed slot had been set pending again; another slot was set pending, or the boot core ended its trial: it is the confirmed slot again. |

### 状態機械 `core_image`

The lifecycle of one boot core slot of a self-boot MCU with a stage0 (its definition's `bootcore` slots), kept in the boot core records of the state log. The agent writes a new core into the idle slot and sets it pending; stage0 tries it and the core settles the trial itself: `confirmed` by the new core after its own check, `rolled_back` by the old one when stage0 gave the new one up. The app sees those at its next start.

状態: `Empty` (最初の状態), `Written`, `Pending`, `Confirmed`, `Reverted`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.core_transfer_started` | 0x40 | `erase`: Empty \| Written \| Pending \| Reverted → Empty | `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | The idle boot core slot was erased for a new core (never the running or the confirmed core's slot). |
| `app.core_written` | 0x41 | `written`: Empty → Written | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The core's body verified and its header written and read back. |
| `app.core_pending_set` | 0x42 | `pending_set`: Written \| Pending \| Reverted → Pending | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The core is the one stage0 tries at the next reset. |
| `app.core_confirmed` | 0x43 | `confirmed`: Pending → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | host |  | The new core passed its own check and confirmed itself. |
| `app.core_demoted` | 0x44 | `demoted`: Confirmed → Written | `slot: u8` (1 B), `version: Version` (4 B) | host |  | Another core confirmed itself; this one is no longer. |
| `app.core_rolled_back` | 0x45 | `rolled_back`: Pending → Reverted | `from: u8` (1 B), `to: u8` (1 B), `reason: u8` (1 B) | replay, host |  | Stage0 gave the pending core up (its starts ran out, or its image failed the check) and the core in `to` recorded it. |

### 状態機械 `session`

The update session: an offer, the header, the body, the end.

状態: `Idle` (最初の状態), `Header`, `Body`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.update_offered` | 0x10 | `update_offered`: Idle → Header | `target: u8` (1 B), `slot: u8` (1 B), `body_len: u32` (4 B) | live, host |  | An update passed every DAGON guard; `accept_update` is asked next. |
| `app.header_received` | 0x11 | `header_received`: Header → Header | `got: u16` (2 B) | live |  | Header bytes received so far. |
| `app.body_opened` | 0x12 | `body_opened`: Header → Body | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live |  | The slot is erased and the header (where the target has one) accepted: body chunks are taken from here. |
| `app.transfer_progress` | 0x13 | `data_written`: Body → Body | `done: u32` (4 B), `total: u32` (4 B) | live, host |  | A body chunk was written. |
| `app.verify_started` | 0x14 | `verify_started`: Body → Idle | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live |  | The whole body arrived; it is hashed and read back next. |
| `app.transfer_aborted` | 0x15 | `aborted`: Header \| Body → Idle | `cause: AbortCause` (1 B) | live, host |  | The transfer was dropped. |

### 状態機械 `node`

The node's start and its DAGON links: what the MCU package's start and serving loop report through the agent.

状態: `Off` (最初の状態), `Up`。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.starting` | 0x01 | `starting`: * → Up | `tag: NameDigest` (8 B), `role: u8` (1 B), `mode: u8` (1 B) | live |  | DAGON's start began (the agent was made, or restarted); the slot store is read next. `tag`: the node's image target tag as a name digest (`NameDigest`: the compact form "digest prefix", made as MCU identities are); a host resolves it from the topology's node tags. |
| `app.watchdog_taken` | 0x02 | `watchdog_taken`: Up → Up | `period_ms: u32` (4 B), `trial_policy: TrialPolicy` (5 B) | live |  | The start took over the boot core's watchdog: its period, and what the guard does with it while the image is on trial. |
| `app.bring_up` | 0x07 | `bring_up`: Up → Up | `board: NameDigest` (8 B) | live |  | The board's bring-up ran. `board`: the board definition's id as a name digest (`NameDigest`); a host resolves it from the board definitions it knows. |
| `app.link_up` | 0x03 | `link_up`: Up → Up | `link: u8` (1 B), `addr: u8` (1 B) | live |  | A DAGON link carries frames: the first one arrived, at start or after the link went down. `link`: its kind's code (`dagon_format::with_link_kinds!`); `addr`: the node's own address on it (`NO_ADDR` where a link has none). |
| `app.link_down` | 0x04 | `link_down`: Up → Up | `link: u8` (1 B), `addr: u8` (1 B) | live |  | A DAGON link stopped carrying frames: a reply could not be sent, or the link closed. |
| `app.request` | 0x05 | `request`: Up → Up | `protocol: u8` (1 B), `cmd: u8` (1 B), `seq: u8` (1 B), `via: u8` (1 B) | live |  | A request arrived on link `via`. |
| `app.replied` | 0x06 | `replied`: Up → Up | `status: u8` (1 B), `ms: u32` (4 B) | live |  | The reply to the last request left, `ms` after it arrived. |

### 状態機械 `requests`

Requests answered or refused, and the decision points asked.

状態: `Ready` (最初の状態)。

| 種類 | コード | 遷移 | ペイロード | 受け取り | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.store_opened` | 0x30 | `store_opened`: Ready → Ready | `active: u8` (1 B), `trial: bool` (1 B), `version: Version` (4 B), `attempts: u8` (1 B), `last_reason: u8` (1 B) | live |  | The slot store was read at start. |
| `app.hello` | 0x31 | `hello`: Ready → Ready | – | live, host |  | Identity answered. |
| `app.status_asked` | 0x32 | `status_asked`: Ready → Ready | – | live, host |  | Status answered. |
| `app.update_refused` | 0x33 | `update_refused`: Ready → Ready | `status: u8` (1 B), `by_extension: bool` (1 B) | live, host |  | An update was refused, by a DAGON guard or by an extension's `accept_update`. |
| `app.header_verified` | 0x34 | `header_verified`: Ready → Ready | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | The header's signature, key and compatibility checked out. |
| `app.header_rejected` | 0x35 | `header_rejected`: Ready → Ready | `status: u8` (1 B) | live, host |  | The header was refused (signature, key, compatibility, size). |
| `app.verify_ok` | 0x36 | `verify_ok`: Ready → Ready | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | The body read back matches its digest. |
| `app.verify_failed` | 0x37 | `verify_failed`: Ready → Ready | `status: u8` (1 B) | live, host |  | The body or its header did not verify, or could not be stored. |
| `app.pending_refused` | 0x38 | `pending_refused`: Ready → Ready | `status: u8` (1 B) | live, host |  | A switch of the boot target was refused. |
| `app.self_check` | 0x39 | `self_check`: Ready → Ready | `slot: u8` (1 B), `version: Version` (4 B) | live |  | A confirm of the slot on trial asks `self_check`. |
| `app.confirm_refused` | 0x3a | `confirm_refused`: Ready → Ready | `status: u8` (1 B) | live, host |  | A confirm was refused (not on trial, self-check, store). |
| `app.reboot_requested` | 0x3b | `reboot_requested`: Ready → Ready | `mode: u8` (1 B), `delay_ms: u32` (4 B) | live, host |  | A reboot was accepted, after `allow_reboot_now`. |
| `app.before_reboot` | 0x3c | `before_reboot`: Ready → Ready | – | live |  | The reply has left; the reset follows. |
