---
title: イベント
description: DAGONが発行する全イベントです。
editUrl: false
---

:::note
状態機械のカタログから生成しています。直接編集せず、
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`で再度生成してください。
:::

DAGONの状態の変化は、いずれも状態機械の遷移で、遷移はいずれもイベントを1つ発行します。
このページは状態機械の宣言から作成しているので、全イベントを掲載しています。

イベントの形式: 1（`dagon_events::EVENT_FORMAT`）。種類やペイロードのフィールドを追加しても
番号は不変です。名称の変更、削除、意味の変更の場合は番号が変化します。読込側は、
未知の種類とフィールドを無視し、解釈不能な形式の番号は拒否します。

## 表の凡例

- **種類**: `layer.kind`。ペイロードの型名をsnake caseに変換した名称です。
- **コード**: そのレイヤー内での種類の番号です（`EventRecord.kind`）。
- **遷移**: イベントを発行するメソッドと、遷移前後の状態です（`*`は任意の状態）。
- **ペイロード**: フィールドと、記録内でのバイト数です（`EventRecord.payload`、順番に配置し、
  合計16バイトまで）。
- **受信側**: *live*は同一イメージの拡張がイベント発生時に、*replay*は起動時にアプリのエージェントが
  `replayed: true`を付与して再発行したイベントを、*host*はCLI、serveのイベント、`--events`、
  アクションが受信します。
- **備考**: *action*はブートのアクションプログラムを起動可能です。*retired*は発行を停止済で、
  次回のイベントの形式まで保持しています。

## 判断点

この表にない項目は通知です。応答は、チェーンの順番（公開元、ボードの定義、ライブラリ、利用者）で
最初の応答を使用します。どの拡張も応答しない場合は、既定の値を使用します。

| 判断 | レイヤー | 質問 | 応答側 | 時間の上限 | 既定 | 禁止事項 |
|---|---|---|---|---|---|---|
| `accept_update` | app | `UpdateOffer` -> `Accept` | ボードの定義、ライブラリ、利用者（最初の応答が優先） | 呼出ごとに`DECISION_BOUND_MS`以内（遅延した応答は無視します）。DAGONの全部のガードに合格した後、消去の直前に照会します | 受理 | DAGONが拒否したものの受理（その場合は照会しません） |
| `allow_reboot_now` | app | `RebootRequest` -> `Delay` | ボードの定義、ライブラリ、利用者（最初の応答が優先） | 呼出ごとに`DECISION_BOUND_MS`以内。`Now`か`After(ms)`で、msの上限はマイコン定義の短期側ウォッチドッグ周期の半分。DAGONは最初に返答し、待機中もキックを継続します | 即時 | 再起動の拒否。上限を超過する遅延 |
| `self_check` | app | `TrialInfo` -> `Health` | ボードの定義、ライブラリ、利用者（最初の応答が優先） | 呼出ごとに`DECISION_BOUND_MS`以内（遅延した応答は無視します）。`Pending`は"state"のステータスを維持し、ホストが再試行します | 健全 | 試行中以外のスロットの確定。`confirmed`の遷移の外部での確定 |
| `confirm_plan` | host | デプロイの一覧 -> はいかいいえ | 対話する利用者（ターミナル、ページ）か`--yes`。アクションやホストの拡張は応答しません | 利用者の応答まで | 照会 | 一覧の表示前に実施する、キーの確認とイメージの検証の省略 |

## レイヤー`boot`

### 状態機械`boot_extensions`

ブート段階の拡張の呼出と、その時間の上限です。

状態: `Ready` (初期状態)。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `boot.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (8 B), `budget_us: u32` (4 B), `us: u32` (4 B) | live |  | 拡張のハンドラの実行時間が`budget_us`（マニフェストの`max_run_us`）を超過しました。ブートは同一の判断で続行しました。 |

### 状態機械`sequence`

ウォッチドッグの有効化からジャンプか停止までの、ブートローダの手順です。そのイベントは発生時にブートローダの拡張へ到達します。拡張の動作に関係なく、判断は変化しません。

状態: `Started` (初期状態), `WatchdogArmed`, `LogRead`, `BeforeDecision`, `Chosen`, `NoImage`, `Jumping`, `Halted`。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `boot.watchdog_armed` | 0x01 | `watchdog_armed`: Started → WatchdogArmed | `period_ms: u32` (4 B) | live |  | ウォッチドッグが試行の周期で動作します。以降は拡張が動作します。 |
| `boot.log_read` | 0x02 | `log_read`: WatchdogArmed → LogRead | `ok: bool` (1 B), `pending: bool` (1 B), `confirmed: bool` (1 B), `attempts: u8` (1 B) | live |  | 状態ログを読込しました（`ok: false`: 有効な面が不在）。 |
| `boot.before_decision` | 0x03 | `before_decision`: LogRead → BeforeDecision | `pending: bool` (1 B), `confirmed: bool` (1 B), `attempts: u8` (1 B) | live |  | 判断の直前の最終イベント。 |
| `boot.slot_chosen` | 0x04 | `slot_chosen`: BeforeDecision → Chosen | `slot: u8` (1 B), `trial: bool` (1 B) | live |  | 起動するスロットが確定しました（`trial`: 計数対象の試行での待機中のスロット）。 |
| `boot.no_image` | 0x05 | `no_image`: BeforeDecision → NoImage | – | live |  | 起動可能なスロットが不在です。 |
| `boot.before_jump` | 0x06 | `before_jump`: Chosen → Jumping | `slot: u8` (1 B), `trial: bool` (1 B) | live |  | 制御がDAGONから離脱する直前の最終イベント。 |
| `boot.halt` | 0x07 | `halt`: NoImage → Halted | – | live |  | ブートローダはこの状態で恒久的に停止します。 |

## レイヤー`app`

### 状態機械`decisions`

拡張への判断の呼出と、その上限です。

状態: `Ready` (初期状態)。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (8 B), `point: DecisionId` (1 B), `us: u32` (4 B) | live |  | 拡張が`DECISION_BOUND_MS`の経過後に判断点に応答しました。その応答は無視しました。 |

### 状態機械`image`

イメージのスロット1つのライフサイクルです（マイコンの定義が付与するAPPのスロットそれぞれ）。スロットのストアの変更メソッドは、これらの遷移の内部だけで動作します。`trial_started`と`rolled_back`はブートローダの変更で、起動時にスロットのストアからリプレイします。

状態: `Empty` (初期状態), `Written`, `Pending`, `Trial`, `Confirmed`, `Reverted`。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.transfer_started` | 0x20 | `erase`: Empty \| Written \| Pending \| Trial \| Confirmed \| Reverted → Empty | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | 新規のイメージのためにスロットを消去しました（書込側が動作しているスロットは消去しません。アプリは確定済のスロットも消去しません）。 |
| `app.written` | 0x21 | `written`: Empty → Written | `target: u8` (1 B), `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 本体を検証し、ヘッダを書込して再読込しました。スロットにイメージがあります。 |
| `app.pending_set` | 0x22 | `pending_set`: Written \| Pending \| Confirmed \| Reverted → Pending | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 次回のブートで試行するスロットになりました。 |
| `app.pending_moved` | 0x23 | `pending_moved`: Pending \| Trial → Written | `from: u8` (1 B), `to: u8` (1 B) | live, host |  | 別のスロットが待機状態になりました。このスロットは待機状態ではなくなりました。 |
| `app.trial_started` | 0x24 | `trial_started`: Pending \| Trial → Trial | `slot: u8` (1 B), `version: Version` (4 B), `attempt: u8` (1 B), `max: u8` (1 B) | replay, host |  | ブートローダが待機中のスロットを試行として起動しました（これまでのブート回数は`attempt`回で、ブートローダの許可する上限は`max`回）。 |
| `app.confirmed` | 0x25 | `confirmed`: Trial → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 試行中のスロットを、自己検査の後に確定しました。 |
| `app.demoted` | 0x26 | `demoted`: Confirmed → Written | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 他方のスロットが確定しました。このスロットは確定状態ではなくなりました。 |
| `app.rolled_back` | 0x27 | `rolled_back`: Pending \| Trial → Reverted | `from: u8` (1 B), `to: u8` (1 B), `reason: u8` (1 B), `version: Version` (4 B) | replay, host |  | ブートローダがスロットをロールバックし（試行回数の上限に到達したか、待機中のイメージが検査に失敗しました）、`to`を起動しました。`version`はロールバック対象のイメージの版です。ロールバック表示は該当イメージ専用で（スロットへの新規イメージ書込で解除）、消費側は新規のロールバックと起動ごとに再生される既存のロールバックを識別可能です。 |
| `app.pending_withdrawn` | 0x28 | `pending_withdrawn`: Pending \| Trial → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | live, replay, host |  | 確定済のスロットが再度待機状態に設定されていました。別のスロットが待機状態になったか、ブートローダがその試行を終了したので、確定済のスロットに復帰します。 |

### 状態機械`core_image`

stage0のあるself-bootのマイコンの、ブートローダのスロット1つのライフサイクルです（定義の`bootcore`のスロット）。状態ログのブートローダの記録に保持します。エージェントが新規のコアを未使用のスロット（マイコンにスロットが1つだけの場合はそのスロット）に書込して待機状態にし、stage0がそのコアを試行し、コアが自身で試行を決着させます。新規のコアが自身の検査の後に`confirmed`、stage0が新規のコアを断念した場合は旧コアが`rolled_back`を記録します。アプリは次回の起動でそれらを参照します。

状態: `Empty` (初期状態), `Written`, `Pending`, `Confirmed`, `Reverted`。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.core_transfer_started` | 0x40 | `erase`: Empty \| Written \| Pending \| Confirmed \| Reverted → Empty | `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | 新規のコアのために、未使用のブートローダのスロットを消去しました。動作中かつ確定済のコアのスロットを消去するのは、マイコンにブートローダのスロットが1つしかない場合だけです。 |
| `app.core_written` | 0x41 | `written`: Empty → Written | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | コアの本体を検証し、そのヘッダを書込して再読込しました。 |
| `app.core_pending_set` | 0x42 | `pending_set`: Written \| Pending \| Reverted → Pending | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | 次回のリセットでstage0が試行するコアになりました。 |
| `app.core_confirmed` | 0x43 | `confirmed`: Pending → Confirmed | `slot: u8` (1 B), `version: Version` (4 B) | host |  | 新規のコアが自身の検査に合格し、自身を確定しました。 |
| `app.core_demoted` | 0x44 | `demoted`: Confirmed → Written | `slot: u8` (1 B), `version: Version` (4 B) | host |  | 別のコアが自身を確定しました。このコアは確定状態ではなくなりました。 |
| `app.core_rolled_back` | 0x45 | `rolled_back`: Pending → Reverted | `from: u8` (1 B), `to: u8` (1 B), `reason: u8` (1 B), `version: Version` (4 B) | replay, host |  | stage0が待機中のコアを断念し（起動回数の上限に到達したか、イメージが検査に失敗しました）、`to`のコアがその結果を記録しました。`version`は`image`と同様、断念したコアの版です。 |

### 状態機械`session`

書込のセッションです: 申し出からヘッダ、本体、終了までを扱います。

状態: `Idle` (初期状態), `Header`, `Body`。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.update_offered` | 0x10 | `update_offered`: Idle → Header | `target: u8` (1 B), `slot: u8` (1 B), `body_len: u32` (4 B) | live, host |  | 書込の申し出がDAGONの全部のガードに合格しました。次に`accept_update`へ照会します。 |
| `app.header_received` | 0x11 | `header_received`: Header → Header | `got: u16` (2 B) | live |  | これまでに受信したヘッダのバイト。 |
| `app.body_opened` | 0x12 | `body_opened`: Header → Body | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live |  | スロットを消去し、ヘッダ（ターゲットにヘッダがある場合）を受理しました。以降は本体のチャンクを受信します。 |
| `app.transfer_progress` | 0x13 | `data_written`: Body → Body | `done: u32` (4 B), `total: u32` (4 B) | live, host |  | 本体のチャンクを書込しました。 |
| `app.verify_started` | 0x14 | `verify_started`: Body → Idle | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live |  | 本体全体が到着しました。次にハッシュを計算して再読込します。 |
| `app.transfer_aborted` | 0x15 | `aborted`: Header \| Body → Idle | `cause: AbortCause` (1 B) | live, host |  | 転送を破棄しました。 |

### 状態機械`node`

ノードの起動とそのDAGONのリンクです: マイコン定義のコードの起動処理と応答ループが、エージェント経由で報告します。

状態: `Off` (初期状態), `Up`。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.starting` | 0x01 | `starting`: * → Up | `tag: NameDigest` (8 B), `role: u8` (1 B), `mode: u8` (1 B) | live |  | DAGONの起動を開始しました（エージェントを作成したか、再開しました）。次にスロットのストアを読込します。`tag`: ノードのイメージに付与したターゲットタグの名称ダイジェスト（`NameDigest`: 短縮形式の「ダイジェストの先頭部」で、マイコンの識別情報と同様に作成します）。ホストは構成のノードのタグから解決します。 |
| `app.watchdog_taken` | 0x02 | `watchdog_taken`: Up → Up | `period_ms: u32` (4 B), `trial_policy: TrialPolicy` (5 B) | live |  | 起動処理がブートローダのウォッチドッグを継承しました: その周期と、イメージの試行中にガードが実施する処理。 |
| `app.bring_up` | 0x07 | `bring_up`: Up → Up | `board: NameDigest` (8 B) | live |  | ボードの起動処理を実行しました。`board`: ボード定義のidの名称ダイジェスト（`NameDigest`）。ホストは既知のボード定義から解決します。 |
| `app.link_up` | 0x03 | `link_up`: Up → Up | `link: u8` (1 B), `addr: u8` (1 B) | live |  | DAGONのリンクがフレームを送受信します: 起動時か、リンク切断後の最初のフレームが到着しました。`link`: その種類のコード（`dagon_format::with_link_kinds!`）。`addr`: そのリンクでのノード自身のアドレス（アドレスのないリンクでは`NO_ADDR`）。 |
| `app.link_down` | 0x04 | `link_down`: Up → Up | `link: u8` (1 B), `addr: u8` (1 B) | live |  | DAGONのリンクがフレームの送受信を停止しました: 返答の送信に失敗したか、リンクがクローズしました。 |
| `app.request` | 0x05 | `request`: Up → Up | `protocol: u8` (1 B), `cmd: u8` (1 B), `seq: u8` (1 B), `via: u8` (1 B) | live |  | リンク`via`にリクエストが到着しました。 |
| `app.replied` | 0x06 | `replied`: Up → Up | `status: u8` (1 B), `ms: u32` (4 B) | live |  | 最後のリクエストへの返答を、リクエストの到着から`ms`の経過後に送信しました。 |

### 状態機械`requests`

応答または拒否したリクエストと、照会した判断点です。

状態: `Ready` (初期状態)。

| 種類 | コード | 遷移 | ペイロード | 受信側 | 備考 | 説明 |
|---|---|---|---|---|---|---|
| `app.store_opened` | 0x30 | `store_opened`: Ready → Ready | `active: u8` (1 B), `trial: bool` (1 B), `version: Version` (4 B), `attempts: u8` (1 B), `last_reason: u8` (1 B) | live |  | 起動時にスロットのストアを読込しました。 |
| `app.hello` | 0x31 | `hello`: Ready → Ready | – | live, host |  | 識別情報に応答しました。 |
| `app.status_asked` | 0x32 | `status_asked`: Ready → Ready | – | live, host |  | ステータスに応答しました。 |
| `app.version_asked` | 0x3e | `version_asked`: Ready → Ready | – | live, host |  | VERSIONに応答しました。 |
| `app.update_refused` | 0x33 | `update_refused`: Ready → Ready | `status: u8` (1 B), `by_extension: bool` (1 B) | live, host |  | 書込の申し出を、DAGONのガードか拡張の`accept_update`が拒否しました。 |
| `app.header_verified` | 0x34 | `header_verified`: Ready → Ready | `slot: u8` (1 B), `version: Version` (4 B) | live, host |  | ヘッダの署名、キー、互換性を検証しました。 |
| `app.header_rejected` | 0x35 | `header_rejected`: Ready → Ready | `status: u8` (1 B) | live, host |  | ヘッダを拒否しました（署名、キー、互換性、サイズ）。 |
| `app.verify_ok` | 0x36 | `verify_ok`: Ready → Ready | `target: u8` (1 B), `slot: u8` (1 B), `len: u32` (4 B) | live, host |  | 再読込した本体がダイジェストと一致しました。 |
| `app.verify_failed` | 0x37 | `verify_failed`: Ready → Ready | `status: u8` (1 B) | live, host |  | 本体かそのヘッダの検証に失敗したか、保存に失敗しました。 |
| `app.pending_refused` | 0x38 | `pending_refused`: Ready → Ready | `status: u8` (1 B) | live, host |  | ブート対象の切替を拒否しました。 |
| `app.self_check` | 0x39 | `self_check`: Ready → Ready | `slot: u8` (1 B), `version: Version` (4 B) | live |  | 試行中のスロットの確定時に`self_check`へ照会します。 |
| `app.confirm_refused` | 0x3a | `confirm_refused`: Ready → Ready | `status: u8` (1 B) | live, host |  | 確定を拒否しました（試行中以外、自己検査、ストア）。 |
| `app.reboot_requested` | 0x3b | `reboot_requested`: Ready → Ready | `mode: u8` (1 B), `delay_ms: u32` (4 B) | live, host |  | `allow_reboot_now`の後に、再起動を受理しました。 |
| `app.before_reboot` | 0x3c | `before_reboot`: Ready → Ready | – | live |  | 返答を送信しました。その後にリセットします。 |
| `app.percent_told` | 0x3d | `percent_told`: Ready → Ready | `percent: u8` (1 B) | live |  | ホストが、書込の進捗率（1つの数値）をノードに通知しました。ホストの呼出側がこのノードについて表示する数値と同一です。 |
