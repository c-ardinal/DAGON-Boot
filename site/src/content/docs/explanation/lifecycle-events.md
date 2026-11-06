---
title: "ライフサイクルイベントと拡張"
description: "ブートローダ、エージェント、リレー、ホストに共通の1つのモデル: 状態機械、イベント、ポート、保護です。"
---

この文書はDAGONのライフサイクルイベントと拡張のモデルの設計です。実装と、DAGONを拡張するすべての人のための参照です。次のことを示します。

- すべてのレイヤーのすべての状態遷移を、観測できるイベントにする方法
- ボードの定義、ライブラリ、アプリケーションがDAGONを拡張する方法。ブート段階を含むすべてのレイヤーで、イベントを観測し、少数の判断点に答え、ポートを通してDAGONの一部を置き換えます。
- マイコンの保護ハードウェアが、書き込みを安全にしているものに拡張を触れさせない方法
- ボードの値とブート時のアクションが定義から来る方法
- モデルのバージョンの付け方

目標はDAGONと利用者のコードを完全に分けることです。利用者（ボードの定義、ライブラリ、アプリケーション）は、DAGONの公開API、イベント、拡張のポート、定義で、必要なことをすべて満たします。DAGONの中を編集も、コピーも、フォークもしません。DAGONはどのサンプル、ボード、ペリフェラルについての知識も持ちません。このモデルは決まった一覧ではなく、前もって分からないニーズ（車載の診断の転送、追加のセキュリティ処理、ディスプレイ、別のストレージ）のために書かれています。

この文書で使う言葉です。

- *ブートローダ*: DAGONがアプリの前に動かすイメージ（self-bootのマイコンではstage0とブートローダ。vendor-stage2のマイコンではベンダーの2nd stage）。
- *stage0*: リセットの後で最初に動く小さなプログラム。起動するブートローダのスロットを選びます。
- *エージェント*: アプリの中の`dagon-agent`。
- *リレー*: 親機のPCへのリンクと子機へのリンク。
- *ホスト*: CLIと`dagon serve`が動かす`dagon-host`。
- *拡張*: DAGONの外にあって、DAGONが拡張点のどこかで動かすコードかデータ。
- 親機/子機: 英語のhub/nodeにあたります。
- *キック*: ウォッチドッグのリセット（カウンタを戻して時間切れを防ぐこと）。
- *ウォッチドッグによるリセット*: ウォッチドッグが時間切れでチップをリセットすること。

## 1. 状態機械とそのイベント

DAGONの状態の変化は、どれも宣言された状態機械の遷移を通して起き、遷移はどれもイベントを出します。状態機械は`state_machine!`でそれぞれ1度だけ宣言します（1.7）。規範となるカタログは生成されたものです。[イベント](/reference/events/)は状態機械の`CATALOG`の定数（種類、ペイロード、属性、観測の経路）から作り、テストでそれらに照らして確かめます。この節のイベントの表は、状態機械が作るものの例です。生成されたカタログに無い項目は持たず、2つが違うときは生成されたカタログが正しいものです。1.8は同じ但し書きのもとで、イベントの区分ごとの観測の仕方を示します。

### 1.1 イメージのライフサイクル（スロットごと、永続）

イメージのスロットの状態は、リセットをまたいでスロットのストアに残ります（kind S: 状態ログ。kind V: otadataのようなベンダーの選択データ）。2つのレイヤーに遷移を持つ1つの状態機械です。エージェントはリンク越しに変え、ブートローダはブート時に変えます。この状態機械の外で永続のイメージの状態を変える公開の経路はありません。

```
                 erase                  written                 pending_set
  Empty <────────────────── Written ───────────────> ... Written ────────────> Pending
    ^                          ^   ^                                              │
    │ erase                    │   │ pending_moved (another slot set pending)     │ attempt (boot)
    │                          │   └──────────────────────────────────────────────┤
    │                          │ demoted (the other slot confirmed)               v
  Reverted <── rolled_back ──  │                                                Trial ──confirmed──> Confirmed
    │        (boot)            │                                                  │                    │
    └──── erase ───> Empty     └──────────────────────────────────────────────────┘ attempt (boot), until max
```

| 遷移 | レイヤー | ガード（拒否のときのステータス） | 永続の形 | イベント |
|---|---|---|---|---|
| `erase` | app | XFER_BEGINがすべてのガードと`accept_update`を通った。`Empty`、`Written`、`Pending`、`Reverted`から（動いているスロットや確定したスロットからは不可）。`Pending`からなのは、プロトコルが、待機にしたが起動していないイメージを新しいイメージで置き換えられるからです。消したヘッダはもうSLOT_PENDINGの記録と一致しないので、そのスロットは空と読まれます | スロットの本体とヘッダの場所を消去 | `app.transfer_started` |
| `written` | app | XFER_END: 本体のダイジェストがヘッダのものと同じで、ヘッダを書いて検証し直した | ヘッダが所定の場所にある | `app.written { slot, version }` |
| `pending_set` | app | SET_PENDING: ヘッダがあり、署名と互換性、本体のダイジェストが正しい。動いているスロットは不可。`Written`、`Reverted`、`Pending`（SET_PENDINGを繰り返すと、もう一度追記します）、`Confirmed`（もう一方のスロットが試行のあいだに、確定したスロットをもう一度待機にして、そちらへ戻します）から | SLOT_PENDINGの記録/ ベンダーの選択 | `app.pending_set { slot, version }` |
| `pending_moved` | app | このスロットが`Pending`か`Trial`のときに、もう一方のスロットへのSET_PENDINGがあり、このスロットが確定したものでない | 同じSLOT_PENDINGの記録（ログは待機のスロットを1つ示します） | `app.pending_moved { from, to }` |
| `pending_withdrawn` | app | このスロットが確定したもので、もう一度待機にされ、別のスロットが待機になるか、その試行がロールバックされた。ログの`confirmed`がこのスロットを示し続けるので、`Confirmed`に戻ります | 同じSLOT_PENDINGかROLLBACKの記録 | `app.pending_withdrawn { slot, version }` |
| `attempt` | boot | 待機のスロットを検査した（署名、ダイジェスト）、`attempts < max` | BOOT_ATTEMPTの記録/ ベンダーの"new" → "pending verify" | `boot.attempt_recorded { slot, attempt, max }` |
| `confirmed` | app | 試行中のCONFIRM、`self_check`が健全 | CONFIRMEDの記録/ ベンダーの"valid" | `app.confirmed { slot, version }` |
| `demoted` | app | もう一方のスロットの`confirmed`（ログの`confirmed`はもう一方のスロットを示します） | 同じCONFIRMEDの記録 | `app.demoted { slot, version }` |
| `rolled_back` | boot | 試行回数を使い切った、または待機のイメージが検査に失敗した | ROLLBACKの記録/ ベンダーの中止 | `boot.rollback { from, to, reason }` |
| `chosen` | boot | 起動するスロットが決まった（確定か、試行） | なし | `boot.slot_chosen { slot, trial, version }` |
| `no_image` | boot | 起動できるスロットが無い | なし | `boot.no_image { reason }` |

`erase`と`pending_set`の追加の遷移元の状態と`pending_withdrawn`があるのは、プロトコルがそれらのリクエストをすでに許しているからです。状態機械はリンク上の動作を狭めず、それに従います。

1つの永続の記録が2つのスロットを動かすことがあります（`confirmed`と`demoted`、`pending_set`と`pending_moved`か`pending_withdrawn`）。状態機械はスロットごとに1つの状態を持ち、1つのトリガーが両方の遷移を実行し、両方のイベントをこの順に出します。テストはログから畳み込んだストア（`LogState`）の上でエージェントの`ImageStore`を、準備済みのデバイスから、エージェントとブートローダが書くもの（転送、切り替え、確定、ブートの試行、ロールバック）のあらゆる順序で動かし、各ステップの後ですべてのスロットの状態機械の状態がSTATUSと同じであることを確かめます。そのため、遷移の無い畳み込みの結果は存在せず、エージェントは2つ目のスロットの分も含めて遷移を実行します。

状態機械はスロットのストアのポートを包みます。`ImageStore<S: SlotStore>`がストアを持ち、ポートの変更メソッド（`erase`、`write`、`write_header`、`set_pending`、`confirm`）は、状態機械だけが作る`Transition`を、その変更のイベントを出す遷移の中で受け取ります。`SlotStore`のトレイトは、ボードが実装するポートのままです。エージェントがストアを持ち、だれにも渡しません。ボードのほかの資源（フラッシュの読み手、ウォッチドッグ）は、マイコンの定義のコードが自分で持つハンドルです。それらのハンドルが届くもの（生のフラッシュドライバ、シミュレータの準備用の補助）は、この保証の外です。同じイメージでフラッシュに触れるものと同じく、ボードやテスト環境の信頼されたコードで、拡張からはサンドボックス（4）が守ります。ヘッダの書き込みが読み返せないときは、スロットをもう一度消去します。そのため、故障が状態機械の記録していないイメージを残すことはありません。エージェントは`images() -> &ImageView`を公開します（読み取り専用: ステータス、各スロットのライフサイクルの状態、自分のバージョン、試行中かどうか）。起動時に自分で確定するアプリ（テスト環境用）は、起動にそれを求めます（`App { confirm: Confirm::Own, .. }`）。起動はボードの立ち上げの後で`confirm_own()`を呼びます。これが`confirmed`の遷移です。まず`self_check`の判断、次にポートの確定、イベントは必ず出します。ブートのレイヤーの遷移は、アプリのステージの観測者にも見えるように、アプリが起動時にリプレイします（1.2、5.5）（`app.trial_started`、`app.rolled_back`）。

### 1.2 ブートの手順

self-bootのマイコン（stage0とブートローダ）: `Entry → HalReady →
WatchdogArmed → LogRead → BeforeDecision → Planned → Committed →
Jumping | Halted`。`plan`は純粋です（フラッシュを読み、判断と追記する記録を計算します）。`commit`がイメージのライフサイクルの遷移`attempt`、`rolled_back`、`chosen`、`no_image`を実行します。stage0が持つのは`Entry`、`CoreChosen`、`Halted`だけです。状態ログのブートローダの記録と試行のセルから、ブートローダのスロットを選びます（`dagon_core::bootcore::choose`、retained-formats.md 7）。数えた起動が`trial_attempts`より少ないあいだは待機のコアを、それ以外は確定したコアを選びます。`core_slot_chosen`の前に、コアのヘッダが記録の示すものであることと、本体がヘッダのダイジェストと一致することを確かめ（4.7）、試行のコアの前にウォッチドッグを始め、拡張は動かしません。stage0はフラッシュに書きません。起動したブートローダは、自分のブートの判断を実行する前にブートローダの試行を決着させます（`settle_log`）。待機のコアは自分の検査（起動できるアプリがあり、そのアプリのヘッダがコアの鍵で検証できる）の後でだけCONFIRMED（ターゲットBOOTCORE）を追記し、それ以外はリセットします。別のコアが待機のときに起動した古いコアは、ROLLBACKを追記します。ウォッチドッグのルールは4.1にあります。

ブートローダのイメージのライフサイクル（エージェントの`core_image`の状態機械）は、1.1のアプリのイメージのライフサイクルに、ブートローダの経路が持つ遷移で従います。エージェントの`erase`、`written`、`pending_set`（ターゲットBOOTCOREのXFERとSET_PENDINGで、空いているブートローダのスロットへ。動いているコアや確定したコアのスロットは不可）、新しいコア自身のCONFIRMEDによる`confirmed`と`demoted`、古いコアのROLLBACKによる`rolled_back`（アプリの起動時にリプレイします）です。アプリから見える`Trial`の状態はありません。どのアプリも動く前に、コアが試行を決着させるからです。ターゲットのバイトが`BOOTCORE`のSTATUSは、ブートローダのスロット（バージョン、ヘッダのダイジェスト、動いているコアと確定したコア）を報告します。マイコンの定義がブートローダのスロットを宣言しないボードは、`Unsupported`を返します。ブートローダのスロットの数と配置は、マイコンの定義の`[[slot]] image = "bootcore"`の項目から来ます（持つマイコンではどれも2つです）。

ブート段階の拡張（2）とブートのアクションプログラム（5.2）は、`WatchdogArmed`以降でだけ動きます。それより前のイベントはどれも記録してリプレイしますが、実行中の観測者はいません。そのため、ウォッチドッグがブートを守る前にDAGONの外のものは動きません。ブートのイベントはどれも、フラッシュの書き込みを伴わない保持メモリであるブート記録（5.5）にも追記され、アプリのエージェントが起動時に同じ`boot.*`のイベントとして`replayed: true`を付けてリプレイします。この記録が`plan`、`commit`、`self_check`、`confirm`の入力になることはありません。

| イベント | いつ | ペイロード | ブート時にlive |
|---|---|---|---|
| `boot.entry` | リセット後のDAGONのコードの最初の命令、HALは未初期化 | `stage`、`reset_cause` | いいえ（記録のみ） |
| `boot.hal_ready` | HALとクロックの初期化の後 | `stage` | いいえ |
| `boot.watchdog_armed` | ウォッチドッグを始めた（4.1）。ここから拡張が動きます | `period_ms` | はい |
| `boot.log_read` | 状態ログを読んだ | `ok`、`pending`、`confirmed`、`attempts` | はい |
| `boot.before_decision` | `plan`を実行する直前。判断の前の最後のイベント | `pending`、`confirmed`、`attempts` | はい |
| `boot.decision_planned` | 判断が決まり、何も書いていない | `Decision` | はい |
| `boot.core_slot_chosen` | stage0がブートローダのスロットを選んだ | `slot`、`version` | いいえ |
| `boot.slot_chosen` | イメージのライフサイクルの`chosen` | `slot`、`trial`、`version`、`entry` | はい |
| `boot.attempt_recorded`、`boot.rollback`、`boot.no_image` | イメージのライフサイクル（1.1） | | はい |
| `boot.before_jump` | 制御がDAGONを離れる前の最後のイベント | `slot`、`trial` | はい |
| `boot.recovery` | アプリの代わりに復旧の経路に入る | `path` | はい |
| `boot.halt` | コアが完全に止まる | `code` | はい（ブートローダ） |
| `boot.extension_ran` / `boot.extension_faulted` / `boot.extension_overran` | 拡張のハンドラが戻った/ フォールトした（保護、バス、命令、不正なサービス）/ 時間を超えた | `id`、`us` / `id`、`fault`、`pc?`、`addr?` / `id`、`budget_us` | はい |
| `boot.extension_disabled` / `boot.extension_irq_storm` | 拡張をブートの残りのあいだ無効にした（フォールトの上限か、前のブートのロックアップヒューズ）/ 宣言されたペリフェラルの割り込み元を無効にした | `id`、`why` / `id`、`peripheral` | はい |
| `boot.program_step` / `boot.program_skipped` | ブートのアクションを1つ実行した/ プログラムを実行しなかった | `index`、`op`、`ok` / `why` | はい |
| `boot.step` | 診断のステップに達した | `n` | いいえ |

vendor-stage2のマイコンにはDAGONのブート段階がありません。アプリが起動時に選択データから`boot.slot_chosen`、`boot.rollback`、`boot.recovery`を`replayed:
true`で出し、ブート段階の拡張はそこには存在しません（4.5）。

### 1.3 エージェントのセッション

状態は`Idle`、`Header { slot, body_len, got }`、`Body { target, slot,
len, next }`です。プロトコルのリクエストはどれも遷移です（HELLO、STATUS、VERSION、進み具合は自己遷移）。プロトコル自体もポートです（2.1）。状態機械のトリガーはプロトコルに依存せず（`update_offered`、`header_chunk`、`data_chunk`、`transfer_end`、`switch`、`reboot`、`confirm`、`status`）、プロトコルの拡張（DAGONのリンク、UDSなど）がメッセージをそこに対応づけます。

生成される起動時の順序: `app.starting` → `app.store_opened`（または`app.store_failed`）→ ブートのリプレイ（`replayed: true`の`boot.*`）→ `app.boot_replayed`（または`app.boot_record_missing`）→ `app.trial_started` / `app.rolled_back` → `app.watchdog_taken` → `app.bring_up` → `app.link_up` …

| イベント | いつ | ペイロード |
|---|---|---|
| `app.starting` | DAGONの起動関数に入った | `tag`（名前のダイジェストとして、2.4）、`role`、`mode` |
| `app.store_opened` / `app.store_failed` | 状態ログや選択データを読んだ/ 読めない | `active`、`trial`、`version`、`attempts`、`last_reason` / `error` |
| `app.boot_replayed` / `app.boot_record_missing` | ブート記録の有効なイベントをリプレイした（見つかったブートごとに要約1つ、古い順）/ 有効なイベントがまったく無い（5.5） | ブートの識別、`writers`、`entries`、`invalid_entries` / `why` |
| `app.trial_started` / `app.rolled_back` | 状態ログからリプレイした | `slot`、`version`、`attempt`、`max` / `from`、`to`、`reason` |
| `app.watchdog_taken` | ブートローダのウォッチドッグを引き継いだ | `period_ms`、`trial_policy` |
| `app.bring_up` | ボードの立ち上げを実行した | `board_id`（名前のダイジェストとして、2.4） |
| `app.extension_started` / `app.extension_faulted` / `app.extension_overran` | 拡張のハンドラが動いた/ フォールトした/ 超過した（4.2） | `id`、`point` / `id`、`point` / `id`、`point`、`us` |
| `app.link_up` / `app.link_down` | DAGONのリンクがフレームを送受信する（起動時か、切れた後の最初のフレーム）/ 止まった（返答を送れない、またはリンクが閉じた） | `link`、`addr` |
| `app.request` / `app.replied` | リクエストが届いた（どのプロトコルでも）/ その返答が出た | `protocol`、`cmd`、`seq`、`via` / `status`、`ms` |
| `app.hello` / `app.status_asked` / `app.version_asked` | 識別/ ステータス/ VERSIONに答えた | `via` |
| `app.update_offered` | 書き込みのリクエストがDAGONのすべてのガードを通った。**判断**`accept_update` | `target`、`slot`、`body_len` |
| `app.update_refused` | 拒んだ | `status`、`by: Dagon \| Extension` |
| `app.header_verified` / `app.header_rejected` | ヘッダの署名と互換性（検証器のポートを通して） | `slot`、`version`、`key_id` / `status` |
| `app.transfer_started`、`app.transfer_progress`、`app.transfer_aborted` | 消去が終わった/ 各チャンクを書いた/ 無通信のタイムアウト、ストアのエラー、新しいリクエスト | `target`、`slot`、`len` / `done`、`total` / `cause` |
| `app.verify_started`、`app.verify_ok`、`app.verify_failed` | 本体のハッシュと読み返し | `len` / `slot`、`version` / `status` |
| `app.written`、`app.pending_set`、`app.pending_moved`、`app.pending_refused`、`app.confirmed`、`app.demoted`、`app.confirm_refused` | イメージのライフサイクル（1.1） | |
| `app.percent_told` | ホストが、書き込みの進み具合のひとつの数字を伝えた（DAGON Consoleの%と同じ。転送のあいだ、ノードが受けられるときだけ。表示のタスクは`AppSnapshot::percent`で読みます） | `percent` |
| `app.reboot_requested` | 再起動を受け付けた。**判断**`allow_reboot_now` | `mode` |
| `app.before_reboot` | 返答が出た。リセットが続きます | |
| `app.self_check` | 試行中の確定。**判断**`self_check` | `slot`、`version` |
| `app.rom_entry` | 復旧への入り口を受け付けた | `method` |
| `app.conformance` | 適合試験に応じた | `test` |
| `app.liveness_overdue` | タスクの遅れでガードがキックをやめた | `task` |

### 1.4 リレー

リレーもアプリです（自分のエージェントが`app.*`を出します）。リレー自身の状態機械が持つのは、PCへのリンクの接続/切断、中継したリクエストや表示のリクエストの後の無通信の時間、ポーリングと中継した返答による子機ごとの`Unknown → Present | Absent`、進み具合の画面です。子機へのリンクはポートです（子機のプロトコルの拡張は、DAGONのフレームだけでなくUDSも中継できます）。

| イベント | いつ | ペイロード |
|---|---|---|
| `relay.pc_up` / `relay.pc_down` / `relay.pc_quiet` | PCへのリンクを開いた/ 閉じた/ 無通信の時間が終わった | |
| `relay.relay_request` | 中継のリクエストを受けた | `link`、`addr`、`cmd` |
| `relay.child_seen` | 中継した識別やステータスの返答を解析した | `index`、`link`、`addr`、`Seen` |
| `relay.child_found` / `relay.child_lost` / `relay.child_polled` | 不在の後でポーリングに答えた/ 最初の無応答/ どのポーリングでも | `index`、`link`、`addr`、`Seen` |
| `relay.child_update_progress` / `relay.child_update_result` | 中継した転送の返答/ 中継した切り替えや確定の返答、または最後の段階の表示のリクエスト | `index`、`done`、`total` / `index`、`outcome` |
| `relay.children_set` | 子機の一覧を読み込んだ（構成のコピーか、プロジェクトの予備） | `Subs` |
| `relay.child_control` | 子機の制御線を動かした/ 生のバスアクセス | `line`、`level` / `addr`、`len` |
| `relay.show` / `relay.show_expired` | 表示のリクエストを受けた/ 進み具合の画面を消した | `Progress` / |

### 1.5 ホストの操作

操作（`apply`、`rollback`、`recover`、`confirm`、`provision`、`push_topology`、`build`）ごとに1つの状態機械があり、ノードごとの副状態を持ちます。ほかにプロジェクトのセッションとエージェントとのつなぎがあります。ホストの転送、署名、ビルドの実行はポートです（2.1）。

| イベント | いつ | ペイロード |
|---|---|---|
| `host.build.started` / `host.build.finished` | ノードとスロットごと | `node`、`slot`、`ok`、`elapsed_ms`、`image_sha256` |
| `host.release.prepared` | バンドルを作った | `version`、`nodes[]`、`path` |
| `host.deploy.planned` | 一覧を表示した。**判断**`confirm_plan` | 一覧 |
| `host.deploy.started` / `host.deploy.finished` | | `release`、`nodes[]` / `ok`、`results[]` |
| `host.node.discovered` | 検出 | `target`、識別 |
| `host.node.step` | ノードの各段階（TRANSFER、VERIFY、REBOOT、CONFIRM、REJECTED、FAILED、ROM） | `node`、`target`、`phase`、`percent`、`version` |
| `host.node.reconnecting` | 再起動後のバージョンを待っている | `node`、`elapsed_ms`、`other_version?` |
| `host.node.result` | | `node`、`Done(v) \| Failed(reason, status?) \| NotStarted` |
| `host.confirm.result` | | `Confirmed \| NotOnTrial \| NeedsChoice` |
| `host.rollback.*`、`host.recover.*` | deployと同じ形 | |
| `host.provision.started` / `host.provision.recorded` | ROMを通した最初のインストール。インストールしたブートイメージの識別を記録した | `node`、`boot_sha256` |
| `host.topology.pushed` | 表示用のコピーを親機に書いた | `sha256` |
| `host.project.opened` / `host.project.closed` | プロジェクトのセッション | `path` |
| `host.agent.linked` / `host.agent.unlinked` | 同意を記録した/ 取り消した | `agent`、`way` |
| `host.version.read` | VERSIONに答えた、またはUnsupported。隔離のレベルと、ノードが報告する信頼された拡張を含みます | `target`、`VersionResp?` |
| `host.extension.loaded` / `host.extension.rejected` | ホストの拡張（データで定義、またはcrate）を読み込んだ/ 拒んだ | `id`、`kind` / `why` |
| `host.job.*`、`host.device.*` | serveの作業とデバイスのメッセージ | |
| `host.action.ran` | データで定義したアクションが終わった | `id`、`ok`、`elapsed_ms` |

### 1.6 永続の状態とその状態機械

リセットや再起動を越えて残るものはすべて状態機械の後ろにあり、そこへの書き込みはどれも最後の列の意味でアトミックです。どの時点のリセットや電断でも、前の状態か新しい状態のどちらかが残り、読み手が受け付ける半端な状態は残りません。

| 永続の状態 | 場所 | 状態機械 | 書き込む遷移 | アトミックである方法 |
|---|---|---|---|---|
| アプリのイメージのスロット（ヘッダ、本体）とその選択（状態ログkind S） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `erase`、`written`、`pending_set`、`pending_moved`、`attempt`、`confirmed`、`demoted`、`rolled_back` | 本体が先、ヘッダが最後（スロットにイメージがあるのは、ヘッダを読み返して検証できてからです）。状態ログは追記だけの記録で、それぞれCRCを持ち、交互に使う2つの面にあります。途中で切れた記録はCRCに失敗して無視され、前の記録が有効なままです |
| ベンダーの選択（kind V） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `pending_set`、`attempt`、`confirmed`、`rolled_back` | ベンダーの2セクタの方式: 連番とCRCを持つ項目全体を別のセクタに書き、有効で最新の連番が勝ちます |
| ブートローダのスロットとその記録（ブートローダのログ、またはマイコンの定義が両方のログに同じ面を与えているところではAPPの記録と共有する状態ログ） | ノードのフラッシュ（stage0のあるself-boot） | ブートローダのイメージのライフサイクル（`core_image`、1.2） | エージェントの`erase`、`written`、`pending_set`。新しいコアの`confirmed` / `demoted`。古いコアの`rolled_back`。ホストの`provision`（`host.provision.recorded`）。stage0の`chosen`（`boot.core_slot_chosen`、書き込みなし） | アプリのスロットと状態ログと同じです。stage0の起動の回数は、フラッシュではなく試行のセルにあります（retained-formats.md 7） |
| 工場出荷の復旧用エージェント（vendor-stage2のマイコン） | ノードのフラッシュ、factoryのパーティション、ヘッダは分離 | イメージのライフサイクル（1.1）、ターゲットAGENTの`erase`と`written` | エージェント。確定したアプリがOTAスロットから動いているあいだだけ | 本体が先、ヘッダが最後。書いているあいだは、動いているアプリ（そのエージェントを含む）が戻り道です |
| インストールしたブートイメージの識別（vendor-stage2のマイコン） | プロジェクトの準備の記録 | ホストの`provision` | `host.provision.recorded` | 一時ファイルに書き、古いものの上に名前を変えます |
| ブート記録（保持メモリ、5.5） | ノードのRAMか保持のレジスタ | ブート記録: 追記だけで、記録したイベントのほかに状態はありません | `boot.*`の遷移がそれぞれイベントを1つ追記します | 必要な性質: どのバイトで中断された書き込みも、失うのは最大でそのイベントで、別のイベントを有効と読めるようにはしません。これを満たす形式は実行可能なモデルが定めます（5.5） |
| ロックアップヒューズ（保持メモリ、5.5） | ノードのRAMか保持のレジスタ | ヒューズ: `Clear → Armed → Clear`（呼び出し）、`Armed → Tripped → Clear`（ロックアップ、その後のクリア） | サンドボックスの呼び出しの前後で有効化とクリア。次の起動でトリップ | 必要な性質: どの時点のリセットも、戻った呼び出しのヒューズを偽造せず、ロックアップした呼び出しのヒューズを失いません。これを満たす形式は実行可能なモデルが定めます（5.5） |
| 構成の表示用のコピー | 親機のフラッシュ | 構成のコピー: `Absent → Written → Written`（置き換え） | ターゲットTOPOLOGYの`written`（`app.written`、`host.topology.pushed`） | 必要な性質: どのバイトで中断された置き換えも、親機が読むのは前のコピーか新しいコピーで、混ざったものではありません。これを満たす`topology-bin`の形式と書き込みの順は実行可能なモデルが定めます（5.5） |
| エージェントの同意 | `~/.dagon/agent.json` | つなぎ: `Unlinked → Linked → Unlinked` | `host.agent.linked` / `host.agent.unlinked` | 一時ファイルと名前の変更 |
| プロジェクトのセッション（開いているプロジェクト、リリースの鍵） | serveのプロセス | プロジェクト: `Closed → Open` | `host.project.opened` / `host.project.closed` | 1つのロックの下のプロセスのメモリ |
| 診断のステップ、拡張の時間、無通信の時間、見たものの表 | RAMだけ、プロセスの寿命 | 永続ではなく、イベントだけ | | |

このモデルの永続の構造に、その場で書き直すヘッダ、カウンタ、状態の欄を持つものはありません。永続の書き込みはどれも、1単位でコミットする新しい記録か、名前の変更で置き換えるファイル全体です。このモデルが保持や永続のデータのために導入する形式（ブート記録、ロックアップヒューズ、マイコンの識別を持つコンパイル済みの構成）は、文章では定めません。ランダムな故障の注入を持つ実行可能なモデルとして設計し、それを通った形式が仕様です（5.5）。

### 1.7 遷移の基本要素

イベントの無い遷移は存在できません。状態機械の状態を変える唯一の方法が、イベントを届ける生成された遷移メソッドだからです。crateはすべての状態機械を1つの`state_machine!`で宣言し、crateのルートで1度だけ呼び出します。

```rust
dagon_events::state_machine! {
    /// The update session.
    pub machine session in App {
        initial Idle;
        states { Idle, Header { slot: u8, body_len: u32 }, Body { target: u8, slot: u8, len: u32, next: u32 } }
        transitions {
            update_offered(target: u8, slot: u8, body_len: u32): Idle => Header { slot, body_len }
                emits UpdateOffered = 0x10 { target: u8, slot: u8, body_len: u32 } observe(live, host);
            data_written(end: u32): Body { target, slot, len } => Body { target, slot, len, next: end }
                emits TransferProgress = 0x12 { done: u32 = end, total: u32 = len } observe(live, host);
            aborted(cause: AbortCause): * => Idle
                emits TransferAborted = 0x13 { cause: AbortCause } observe(live, host);
            // …
        }
    }
}
```

このマクロは状態機械ごとに次のものを生成します。生成された`Machine`型の非公開の欄の後ろにある状態の列挙型（宣言したcrateでだけ作れ、`Default`も`Clone`も無く、変更メソッドは遷移メソッドだけです。各メソッドはレイヤーの合成されたチェーン（2.3）を受け取り、イベントと次の状態を作り、イベントを届け、その後で状態を設定します。遷移元でない状態からは何も変えず、何も届けません。`from`の状態は名前で、任意でフィールドを束ね、値のパターンにはなりません）。遷移ごとに1つのペイロードの構造体（`#[non_exhaustive]`で、`Copy`でも`Clone`でもないので、宣言したcrateだけが作れます）。各ペイロードの所有する短い形とその解読（2.4）。`CATALOG`（状態機械、状態、そして遷移ごとにfrom、to、トリガー、種類、コード、バイト数付きのペイロードのフィールド、`observe`の経路、`action`と`retired`のフラグ）。すべての状態からすべての遷移を動かして記録をリプレイする、状態機械ごとのテスト。crateごとに`MACHINES`のstaticを定義し、カタログの登録簿（`dagon-catalog`）がそれを挙げます。`dagon-events`に依存するcrateが欠けているあいだは、そこのテストが失敗します。どの遷移も届かない状態、`emits`や`observe`の経路の無い遷移、重複したコード、大きすぎるペイロード、crateの中の2つ目の`state_machine!`、その外の状態機械は、コンパイルエラーです。[イベント](/reference/events/)はカタログから生成し、テストが2つを同じに保ちます。

判断点（3）は`emits`の節にはなりません。新しい判断点は設計の変更で、まずここに、上限、既定、答える側、安全の論拠とともに書き、独立したトレイトのメソッドにします。

イベントを出すのはDAGONだけです。拡張は観測するだけで、作ることはありません。

### 1.8 だれが何を観測するか

どのイベントにも少なくとも1つの観測の経路があり、経路は生成されたカタログの属性です。そのため、生成された[イベント](/reference/events/)がこの表をイベントごとに持ちます。経路の無い種類があると、生成されたテストが失敗します。*Live*は同じイメージの拡張（2）です。*replay*は、アプリのエージェントが起動時にブート記録（5.5）か状態ログから出し直すものです。*host*はCLI、serveのストリーム、アクション（2.5）で、返答やVERSIONが含むデバイス側のすべてのイベントも見ます。リプレイが届けるのは、カタログが`replay`と印を付け、`retired`でない種類だけです。ほかの記録は推測せず、受け付けません。

| イベント | レイヤー | Live | Replay | Host |
|---|---|---|---|---|
| `boot.entry`、`hal_ready`、`core_slot_chosen`、`step` | boot | いいえ（ウォッチドッグの前、またはstage0） | 破損していない記録済みのイベントはすべて → `boot.*` `replayed: true`、少なくとも1回、識別子付き | VERSIONの記録のTLV |
| `boot.watchdog_armed`とそれ以降のすべてのブートのイベント（`halt`と拡張やプログラムのイベントを含む） | boot | はい: ブート段階の拡張（2）、4のサンドボックスの中 | 同じ | VERSION |
| `boot.halt`、および`before_jump`の前にハングしたブートや拡張でフォールトしたブートのイベント | boot | 停止やハングの時点まで | イベントは記録に残ります。最初に動くアプリが、残っているすべてのブートを古い順にリプレイします | VERSION（上書きされるまで読めます） |
| `boot.slot_chosen`、`attempt_recorded`、`rollback`、`no_image` | boot | はい | ブート記録、そして記録が無くても状態ログ（`app.trial_started`、`app.rolled_back`） | VERSION、STATUS |
| `app.*` | app | はい | `boot_replayed`、`trial_started`、`rolled_back`はそれ自体がリプレイです | 返答、リレーの`child_*` |
| `relay.*` | relay | はい（親機のアプリのチェーン） | | 中継した返答、表示の結果 |
| `host.*` | host | `on_event`のコールバック（CLI、serve）、ホストの拡張 | | ストリーム、`--events`、アクション |

## 2. 拡張のモデル

すべてのレイヤーで形は1つです。DAGONが置き換えのために公開する**ポート**。ポートを提供し、イベントを観測し、判断に答える**拡張**。既定では何もしない**イベント**。そして**1つの合成の順序**（ボードの定義 → ライブラリ → 利用者）。実行の文脈は違いますが（ブートローダと、マイコンが許すところではアプリの非特権のサンドボックス。アプリのembassyのタスク。ホストのスレッドとJSON）、形は同じです。宣言的なアクションプログラム（5.2）は、コードの要らない用途のための、いちばん単純な拡張の種類です。

### 2.1 ポート

ポートは、DAGONが置き換えられる1つの部分のために呼ぶトレイトで、DAGON自身の実装が既定です。ポートを提供する拡張は、リンクされたイメージのその部分を置き換えます。ポートはすべてのレイヤーにあります。

ポートのカタログは、コードの中の1つの表`dagon_events::PORTS`で、ポートごとに1つの記録（`id`、存在するレイヤーの集合、レイヤーごとの既定、信頼の方針、必要なサービス、説明）です。ここから、この表、生成された[ポート](/reference/ports/)、マニフェストの検証（`provides`は拡張のレイヤーのポートを指定すること）、4.2のサービスの表、`dagon status`のポートの列が派生します。テストがこの抜粋をカタログと同じに保ちます。ポートで*ない*DAGONの部分も、同じカタログに`port = false`と理由とともに挙げるので、「これは置き換えられるか」に1つのソースが答えます。

<!-- generated from dagon_events::PORTS -->
| ポート | レイヤー | 既定 | 信頼の方針 | サービス |
|---|---|---|---|---|
| `UpdateTransport` (リンクに出入りするバイト) | boot, app, relay, host | boot, app, relay: USB CDC、UART、I2Cターゲット、PIO UART; host: シリアル | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` (メッセージ ↔ セッションのトリガー: 識別、ステータス、転送、切り替え、再起動、確定) | boot, app, relay, host | DAGONのリンク | untrusted (ガードは飛ばせません。ガードを実行するのはセッションの状態機械で、プロトコルではありません) | return, time, memory |
| `ImageVerifier` (ヘッダの署名、鍵の検索) | boot, app | プロジェクトの鍵でのEd25519 | trusted only | – |
| `Digest` (本体のハッシュ) | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` (検証器とダイジェストが使う基本要素。ハードウェアの加速、HSM) | boot, app, host | ソフトウェア | trusted only | – |
| `DecisionPolicy` (試行の上限、どのスロットが勝つか) | boot | 定義の`max_attempts`、確定より待機を先に | trusted only | – |
| `WatchdogDriver` (ブートと試行を守るウォッチドッグ) | boot, app | マイコンのもの | trusted only | – |
| `SlotStore` (イメージの置き場所) | boot, app | マイコンの定義のフラッシュの配置 | trusted only | – |
| `LogStorage` (状態ログの置き場所) | boot, app | マイコンの定義のフラッシュの配置 | trusted only | – |
| `Recovery` (追加の復旧の経路: 転送のためのサービスの時間枠、ベンダーのダウンロードモード) | boot, app | マイコンとボードの定義のとおりのBOOTSEL/UF2、factory、ROMの入り口 | untrusted, additive (拡張は経路を足すだけで、DAGONのものを消せません) | return, transport, time, memory |
| `Diagnostics` (追加のVERSIONのTLV、追加のステータス、UDSのDIDの表) | boot, app, relay | なし | untrusted | return, time, memory |
| `Display` (段階やブートの結果の見え方) | boot, app, relay | boot: アクションプログラムを通した、ボードの定義の`[boot] led`と`halt`; app, relay: なし | untrusted (ブート時は、宣言されたピンとバスでサンドボックスの中で動きます) | return, pins, buses, time, memory |
| `LogSink` (`Recent`のあふれと拡張のログの行き先) | app, relay, host | なし | untrusted | return, events, time, memory |
| `SelfCheck` (試行中のスロットを確定するときの健全さの答え) | app | 健全 | untrusted (判断点、3) | return, time, memory |
| `BoardBringUp` (アプリの起動時のボードの電源、ピン、バス) | app, relay | ボードの定義のもの | untrusted | return, pins, buses, time, memory |
| `ChildLink` (親機が子機に届く方法) | relay | I2Cコントローラ、PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` (イメージとリリースの署名) | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `KeyStore` (利用者が選んだ置き場所（環境変数、OSのキーストア、1Passwordなど）から署名の鍵を受け取る) | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `BuildRunner` (ノードのイメージのビルド) | host | cargo | untrusted | – |
| `Agent` (CLIとページがリクエストを渡す外部のエージェント) | host | データで定義（エージェントの定義） | untrusted | – |
| `Action` (ホストのイベントの後で実行するプログラムやリクエスト) | host | データで定義（アクションの定義） | untrusted | – |
<!-- end of generated table -->

ポートの既定はDAGONのコードです。信頼された拡張だけのポートの置き換えは特権で動き、4.7のもとでだけ受け付けます。マニフェストの`provides`は、レイヤーの集合がマニフェストの`layers`のすべてのレイヤーを含むポートを指定します（8.4のUDSの例は、`boot`と`app`に`UpdateTransport`、`UpdateProtocol`、`Diagnostics`を宣言し、カタログはこれを許します）。ほかのポートの置き換えは、マニフェストが宣言する能力で、レイヤーのサンドボックス（4）の中で動きます。

### 2.2 拡張とマニフェスト

拡張はマニフェスト`dagon-extension.toml`を持つRustのcrateです（いちばん単純な種類では、定義の中のアクションプログラムです）。

```toml
format = 1
id = "uds-transport"                 # a-z 0-9 -, up to 32
layers = ["boot", "app"]             # where it may be linked
provides = ["UpdateTransport", "UpdateProtocol"]   # ports it replaces (may be empty: observer only)
observes = ["boot.*", "app.update_*", "app.transfer_*"]   # event kinds, validated against CATALOG
decides = ["accept_update"]          # decision points it answers (may be empty)
trusted = false                      # 4.4
[capabilities]                       # what DAGON hands it, nothing else (4.2)
peripherals = ["can0"]               # declared in the board definition's peripheral table
pins = []
buses = []
timer = true
[limits]
max_run_us = 2000                    # per handler call at boot; per call in the app
stack_bytes = 1024
ram_bytes = 4096
```

コードの形はすべてのレイヤーで同じです。

```rust
pub trait Extension {
    /// Its identity in events: the manifest id.
    fn id(&self) -> &'static str;

    /// Every event this extension observes, in order. Default: nothing.
    fn on_event(&mut self, _e: &Event<'_>, _ctx: &Ctx) {}

    // Decision points (3). `None` = abstain. Defaults abstain.
    fn accept_update(&mut self, _o: &UpdateOffer, _ctx: &Ctx) -> Option<Accept> { None }
    fn allow_reboot_now(&mut self, _r: &RebootRequest, _ctx: &Ctx) -> Option<Delay> { None }
    fn self_check(&mut self, _t: &TrialInfo, _ctx: &Ctx) -> Option<Health> { None }
}
```

`Ctx`は、マニフェストが宣言した能力のハンドル（GPIO、バス、タイマ、ペリフェラルのウィンドウ）を持ち、ほかには何も持ちません。ポートの実装は同じ型への独立した`impl UpdateTransport for MyExt`です。`Ctx`に、フラッシュ、状態ログ、ウォッチドッグ、スロットのストア、DAGONのメモリへのハンドルはありません。サンドボックスのレイヤーでは、コードが何をしてもハードウェアがそれらを拒みます（4）。

拡張のインターフェースが信頼の境界です。拡張が受け取るのは`&Event`、`&Ctx`、判断の引数だけで、どれもチェーン、公開の状態、リプレイにはつながりません。チェーンの合成、そこへの配信、リプレイは、レイヤーの起動のAPIです。拡張のインターフェースの外で同じイメージに組み込まれたコード（DAGONのcrate、アプリケーション自身の起動コード）は、1つのアドレス空間のものと同じく信頼の基盤に入ります。信頼されていない拡張の隔離は、サンドボックスの役目です（4）。

### 2.3 合成

型付きのチェーンで、順序は型のレベルで、つまりビルド時に固定されます。

```rust
pub struct Chain<A, B> { /* private */ }   // Chain::new(a, b)
impl<A: Extension, B: Extension> Extension for Chain<A, B> { /* on_event: both; decisions: each in turn, first Some wins */ }
pub struct Nop;  // no extension
#[macro_export] macro_rules! extensions { ($a:expr $(, $b:expr)*) => { … nested Chain::new … } }
```

順序はすべてのレイヤーで常に**公開元 → ボードの定義 → ライブラリ → 利用者**です。レイヤーの起動（ブートローダの生成された`main`、マイコンの定義のコードの`start`、ホストの`Host::new`）が、`compose`（公開の状態と判断点を持つレイヤー: app、relay）か`compose_unpublished`（ブートローダ、ホスト）で自分でチェーンを作ります。`Published`の状態（2.4）のための公開元、次にボードの定義のチェーン、次に利用者の`extensions![..]`（ライブラリが先で、次にアプリ自身）です。結果は中身の見えない`Composed`です。部分に触れることも並べ替えることもできず、拡張ではなく、レイヤーの起動だけが持ちます。利用者のコードが自分を先頭に置くことはできず、`extensions![..]`が公開元を含むことはありません。通知はすべての拡張に届きます。判断は棄権でない最初の答えで止まります。ポートを提供できる拡張は、イメージごとに最大1つです。チェーンがポートを2度提供するプロジェクト（ライブラリと利用者の両方が`UpdateProtocol`を持ち込むなど）は、`dagon gen`が両方の名前を示して受け付けません。

レイヤーごとの登録は次のとおりです。

- **ブートローダ（self-bootのマイコン）。** プロジェクトの`dagon.toml`がブートローダの拡張を挙げます（`[boot] extensions = ["uds-transport",
  …]`）。`dagon gen`がチェーンの型を持つ`boot_wiring.rs`を書き、crateを、4.4の保護マップが隔離する独立したセクション（`.ext.text`、`.ext.data`、`.ext.bss`）でブートローダのイメージにリンクします。チェーンは`const`で作ります（割り当てなし）。ハンドラは入り口のゲート（4.1）を通してブートの状態機械から同期で呼び、呼び出しごとに`max_run_us`の下に置きます。署名付きのブートローダのイメージが拡張を含むので、拡張を変えて書き込むことは、ほかのブートローダの変更と同じく、ROMの経路でのブートローダの書き込みです。
- **アプリ（embassy）。**`node::start` / `board::start`は`App`を受け取ります。自己検査の関数、`exts: Option<&'static mut (dyn Extension +
  Send)>`（`static`に置いたアプリの`extensions![..]`。embassyのタスクはジェネリックにできず、応答のタスクがチェーンを持つので、ジェネリックではなく参照です）、`confirm`（`ByHost`か`Own`: 起動がアプリの試行を確定します）です。起動は、リンクが応答を始める前に、ボードの`BoardBringUp`のポート（`app.bring_up`。無いボードは何も出しません）を実行し、次に自分の確定をします。ボードの定義の拡張（ボードの定義の`extension()`。ESP32-S3では`NodePeripherals::
  board_extension`）が、チェーンのボードの場所に入ります。エージェントは起動とその応答のタスクのものにとどまり、渡す公開の関数はありません。マイコンの定義のコードがレイヤーの`Published`を作って持ち、合成し、アプリに`Reader`を返します。エージェントの状態機械がすべてのイベントをチェーンに届け、チェーンが3つの点で判断のメソッドに尋ねます。マイコンの隔離のレベルが許すところでは（4.5）、応答のタスクはブート時と同じサンドボックスの下で、チェーンのハンドラを非特権で動かします。それ以外では、時間の上限を測って報告しながら、普通のコードとして動かします。
- **リレー。** リレー自身のエージェントは、同じチェーンを持つアプリです。リレーの状態機械のイベントと`ChildLink`のポートはそこを通ります。
- **ホスト。**`dagon-host`は、CLIがプロジェクトの`dagon.toml`（`[host] extensions`）から作ったチェーンを`Box<dyn Extension>`として受け取ります。データで定義する種類（エージェントの定義、アクションの定義、プログラムやデバイスで指定するシリアルやCANのアダプタのような転送の定義）と、`dagon_host::Transport`、`Signer`、`BuildRunner`を実装するRustのcrateで、crateはCLIのビルドのワークスペースの依存として読み込みます。ホストは利用者の権限で動き、そのサンドボックスはOSです。

アプリのエージェントは、合成されたチェーンだけを受け取ります。`Agent::new(config, store, chain)`で、`chain`は`compose`の結果です。別のフックのトレイトはありません。イベントの観測と、`accept_update`、`allow_reboot_now`、`self_check`への応答は`Extension`のメソッドです。マイコンの定義のコードの`start`は、アプリの`fn() -> bool`の自己検査も引き続き受け付け、1メソッドの拡張（`SelfCheck`）に包んで、チェーンの利用者の部分の、アプリの拡張の後に置きます。これは棄権しないので、先に置くと拡張の答えを隠してしまうからです。

### 2.4 待てる公開の状態

非同期の表示やログのタスクが、ハンドラの中で動くことはありません。チェーンの先頭の公開元が、各イベントを次のものにコピーします。

- `Latest`: `Latest { state, last }`を持つ`embassy_sync::watch::Watch`（複数の受信側、最後の値）です。すべてのイベントから畳み込んだレイヤーのスナップショット（アプリの`{ phase, progress, percent, version, trial }`、リレーの子機）と、最後のイベントの記録です。
- `Recent<N>`: 最後のN個のイベントの固定のリングで、`EventRecord`という、`state_machine!`がイベントごとに生成する所有された有界の形です。`{ layer: u8, kind: u8, flags: u8, dropped: u8, seq: u32, payload:
  [u8; 16] }`。フィールドは、カタログが示す短い形でコピーします（長さ、id、ダイジェストの先頭。ノードのタグやボードのidのような名前は、ドメインの接頭辞と名前のSHA-256の最初の8バイトとして記録し、ホストが知っている名前を通して対応づけます）。作る側が戻った後で参照を残すことはありません。あふれると、いちばん古いものを捨てます。その後の記録が`dropped`で欠落を報告し（それぞれ最大255で、残りは次へ繰り越します）、作る側は決して待ちません。`seq`は一周し、連番の演算で比べます。

どちらも1つの`Published<S, N>`にあり、レイヤーのcrate（マイコンの定義、ホスト）が作って持ち、排他の参照で`compose`に渡します。そのため、チェーンが唯一の書き手です。表示、ログ、アプリのタスクは、読むだけの`Reader`（受信側、最新の状態、最近のリング）を受け取ります。リレーの表示の状態は、リレーのスナップショットを持つ同じ型です。ブートローダに公開元はありません。そのイベントはブート記録と拡張のハンドラに届きます。

### 2.5 ホスト: イベントのストリーム、アクション、crateの発見

`dagon serve`は、すべてのホストのイベントを`/v1/events`に次の形で出します。

```json
{"type":"event","format":1,"layer":"host","kind":"node.result","at":"<RFC 3339 UTC>",
 "job":17,"payload":{"node":"n1","outcome":"done","version":"1.1.0"}}
```

既存の`job.*`と`device.*`のメッセージは変わりません（serve-api 1。`event`のメッセージは追加です）。受信側の取り決め: ストリームや`--events`の出力の受信側は、知らないメッセージの型、イベントの種類、ペイロードのフィールドを読み飛ばし、読めない`format`だけを受け付けません。これにより、`EVENT_FORMAT`を進めずに種類を足せます。CLIは`--events`で同じイベントを出力します（それぞれJSON 1行）。

アクションはエージェントの定義と同じく、ホストのデータで定義する拡張の種類です。何をいつ実行するかを示すTOMLファイルで、DAGONが実装する転送の種類を使います。commandの転送（環境変数の許可リスト、新しい作業フォルダ、プロセスグループ、時間と出力の上限）は、エージェントの定義とアクションが共に使うCLIの1つのモジュールです。

```toml
# ~/.dagon/actions/notify.toml  (never a project folder: it names a program)
format = 1
id = "notify-on-result"
on = ["host.node.result", "host.deploy.finished"]   # validated against CATALOG
when = { outcome = "failed" }                        # optional: equality on payload fields
[run]
kind = "command"
program = "notify-send"
args = ["DAGON", "{payload.node} {payload.outcome}"]
timeout_s = 30
```

`kind = "http"`は、イベントのJSONをURLにPOSTします（httpsだけ）。アクションはイベントを出した後で動き、操作の中では動きません。その結果は`host.action.ran`です（アクションは`host.action.*`を購読できません）。アクションは判断に答えません。`confirm_plan`に答えるのはTTY、ページ、`--yes`です。commandのアクションは、commandのエージェントと同じく利用者の権限で動きます。

serveがcrateを見つける場所: `dagon.toml [serve] packages`は、プロジェクトからの相対のglobの一覧です（生成されたプロジェクトでは`["nodes/*"]`）。ここに一致し`Cargo.toml`を持つディレクトリだけを挙げてビルドします。DAGONに組み込みの探索パスはありません。

## 3. 判断点

この表に無いものは通知です。ブートローダに判断点はありません。`DecisionPolicy`を提供する信頼された拡張（4.7）を除き、DAGONの外のものがブートローダの起動するものに影響することはありません。その拡張はそのときその部分のDAGONです。

| # | 判断 | レイヤー | 答えられる者（順に、棄権でない最初のもの） | 上限 | 既定 | 決して変えられないこと |
|---|---|---|---|---|---|---|
| 3.1 | `accept_update(&UpdateOffer) -> Accept` | app | ボード、ライブラリ、利用者 | 50 ms。DAGONのすべてのガードを通った後、消去の前 | 受け入れる | DAGONが拒んだものは受け入れられません（そのときは呼ばれません）。拒否はプロトコルの"state"のステータスで答えます |
| 3.2 | `allow_reboot_now(&RebootRequest) -> Delay` | app | ボード、ライブラリ、利用者 | `Now`か`After(ms)`、`ms ≤ REBOOT_DELAY_MAX`。マイコンの定義のウォッチドッグの周期（`[watchdog] trial_ms`、`confirmed_ms`）から作るビルド時の定数で、短いほうの半分です。DAGONはまず返答し、待つあいだガードの方針のもとでキックを続け、その後でリセットします。待ちのあいだのウォッチドッグによるリセットも同じリセットです | すぐ | 再起動は拒めず、遅れには上限があります |
| 3.3 | `self_check(&TrialInfo) -> Health` | app | ボード、ライブラリ、利用者 | 50 ms。`Pending`は"state"のステータスのままで、ホストに再試行させます | 健全 | 試行中でないスロットは確定できず、`confirmed`の遷移を飛ばせません |
| 3.4 | `confirm_plan(&Prepared) -> bool` | host | 対話する利用者（TTY、ページ）か`--yes`。アクションとホストの拡張はできません | 利用者が答えるまで | 尋ねる | 鍵の確認とイメージの検証は、計画を表示する前に行います |

上限は測ります。合成されたチェーンは、レイヤーのクロック（マイコンの定義のコードから来るDAGONのもので、ポートではありません）で、拡張の呼び出しをそれぞれ測ります。遅れた答えは棄権として無視し、チェーンは`app.extension_overran { id, point, us }`を出します（`id`は拡張のidで、記録ではその64ビットのFNV-1aのハッシュです）。サンドボックスのレイヤーでは、タイマが上限で呼び出しをプリエンプトします（4.3）。

## 4. 隔離と安全の取り決め

安全は、マイコンが許すところでは、信頼ではなくハードウェアで強制します。信頼されていない拡張はサンドボックスの中で非特権で動き、出口は、DAGONが使うたびに検証する特権のサービス呼び出しだけです。マイコンの保護ユニットは、拡張に自分のコード、データ、スタックと、マニフェストが宣言した能力のウィンドウを与え、ほかには何も与えません。DAGONのコードも、DAGONのメモリも、フラッシュコントローラも、状態とログのストレージも、ウォッチドッグも、どのバスマスタも、どのクロック、リセット、割り込みの経路のブロックも与えません。DAGONはプリエンプトして呼び出しをアンワインドするタイマで時間の上限を強制します。超過やフォールトがあっても、DAGONはブート（や応答）を続けます。信頼されていない拡張が何をしても、ブートの判断、署名、ロールバック、ウォッチドッグのルール、確定のルールは変わりません。

このモデルは、DAGONが対応していないマイコンのためにも書かれています。マイコンに固有のものはすべて**マイコンの定義の中のデータ**です。保護ハードウェアの種類とそのパラメータ、バスマスタと特権の制御ブロックとそのアドレスの範囲、フォールトとアンワインドの仕組み、ウォッチドッグ、保持メモリです。DAGONはマイコンごとではなく、保護ハードウェアの*種類*ごとに生成器と検査器を1つずつ持ちます（4.4）。既知の種類のマイコンを足すのは定義です。新しい種類はトレイトの後ろの新しい生成器のモジュール1つです。4.5の3つのマイコンの表は定義から生成した例で、規範ではありません。規範は定義のスキーマと種類のルールです。

### 4.1 サンドボックス

| 要素 | ルール |
|---|---|
| 特権 | 拡張のハンドラは、アーキテクチャの非特権モードで、自分のスタックで動きます（Cortex-M: `CONTROL.nPRIV = 1`、`SPSEL = 1`のThreadモード。RISC-V: Uモード。PMSを持つXtensa: World 1。TrustZoneを持つARMv8-M: Non-secure状態）。DAGONは特権（またはSecure）で動きます。非特権のコードは、割り込みをマスクできず、自分の特権を変えられず、特権モードのスタックポインタ、保護ユニット、割り込みコントローラ、システム制御ブロックに書けません（対応するどの種類でも、アーキテクチャで決まっています）。DAGONのコードやデータを実行も読み出しもできません（4.4）。 |
| 入口 | DAGONは入り口のゲートを通してハンドラを呼びます。特権のRAMに`CallFrame`を記録し（呼び出し先が保存するレジスタ、特権のスタックポインタ、続きのアドレス、拡張のid、呼び出しの持ち時間）、拡張の保護マップを読み込み（4.4）、非特権のスタックポインタを拡張のスタックのリージョンの先頭に設定し、サンドボックスのタイマを有効にし（4.3）、引数を引数レジスタに入れて、非特権モードでハンドラの入り口に落ちます（借用のペイロードはDAGONがまず拡張のデータのリージョンにコピーするので、ハンドラが受け取るのは自分のメモリへのポインタだけです）。 |
| 出口 | 出口は、サービス呼び出し`RETURN`（4.2）、サンドボックスのタイマ、フォールトだけです。3つとも4.3を通して`CallFrame`の続きへアンワインドし、拡張のスタックは決して通りません。 |
| メモリ | 保護マップ（4.4）が許すのは、ちょうど次のものです。拡張の`.ext.text`（読み、実行）、`.ext.data`/`.ext.bss`（読み、書き、実行禁止）、スタック（読み、書き、実行禁止、底にガード）、マニフェストが宣言した能力のウィンドウ。ほかはすべて、非特権のアクセスでフォールトします。DAGONのコードとデータ、ブート記録、ROM、すべてのバスマスタ、すべてのクロック、リセット、割り込みの経路、パッドの多重化のブロック、フラッシュコントローラ、状態とログのストレージ、ウォッチドッグ、ほかの拡張のメモリです。 |
| 能力のウィンドウ | ペリフェラルをウィンドウにできるのは、マイコンの定義がウィンドウにできると挙げているとき（`[[peripheral]] window_eligible = true`）だけです。メモリに書けず、割り込みやクロックを経路づけられず、DAGONと何も共有しない、自己完結したレジスタブロックです（4.6、11）。ほかのものにはすべて、同じ宣言のもとでDAGONが拡張の代わりに行うサービス（4.2）を通して届きます。 |
| 時間 | マニフェストにある呼び出しごとの持ち時間で、レイヤーが上限を設けます。サンドボックスのタイマが持ち時間でプリエンプトします（4.3）。ウォッチドッグへのキックはDAGONだけが、自分の予定で行います。 |
| 割り込み | 拡張は割り込みハンドラを持たず、割り込みの許可、禁止、経路づけができません。宣言されたペリフェラルの割り込みは、特権で有界のDAGONのハンドラが処理し、データを拡張ごとの特権のキューに移し、拡張がサービスでポーリングするフラグを立てます（4.2）。サンドボックスのタイマは設定できる最高の優先度を持ち、どの能力の割り込みもそれより低く、宣言した頻度より多く割り込む割り込み元はハンドラが無効にします（`extension_irq_storm`）。 |
| DMA | バスマスタのコントローラ、チャネル、ディスクリプタ、連結のレジスタが、ウィンドウの中に入ることはありません（11）。DMAの要る拡張の入出力は`dma_*`のサービスを通します。DAGONが拡張のバッファを検証し、特権のバウンスバッファを通してコピーし、コントローラを自分でプログラムし、キューで完了を報告します。キャッシュのあるマイコン（4.10）では、バウンスバッファをキャッシュされないメモリに置くか、サービスがクリーンと無効化を行います。 |
| 繰り返しのフォールト | 1回のブートやアプリの実行で`EXT_FAULT_LIMIT`（3）回フォールトか超過をしたハンドラは、その残りのあいだ無効にします（`extension_disabled`）。ロックアップヒューズ（4.3）は、前のブートでコアをロックアップさせた拡張を無効にします。 |
| 有効化の順序 | サンドボックスがあるのは`WatchdogArmed`からです。ウォッチドッグはHALの初期化のすぐ後に、ログを読む前、どの拡張やアクションも動く前に、マイコンの定義の`trial_ms`で有効にします。`plan`の後、判断が試行でなければ`confirmed_ms`に設定し、ジャンプの前に1度キックします。 |

### 4.2 サービスのABI

サービス呼び出しは、アーキテクチャのスーパーバイザコール命令で、サービスのidと引数を引数レジスタに入れ、結果を最初のレジスタに返します（Cortex-M: `svc #0`、r0–r3。RISC-V: `ecall`、a0–a3。Xtensa: `syscall`。TrustZoneを持つARMv8-M: Secureのゲートウェイのベニヤの呼び出しで、idをベニヤの引数にした同じABIです）。ハンドラは特権で動き、拡張からDAGONに入る唯一の想定された入り口です。DAGON自身のコードはこの命令を使わないので、特権モードからのサービス呼び出しはそれ自体がフォールトです。ABIはフラッシュ、状態ログ、ブート記録、ウォッチドッグ、DAGONのメモリを読み書きするサービスを公開しません。サービスの特権の実装は、DAGONのほかのコードと同じく信頼の基盤の一部で、その不具合はDAGONの不具合です。

検証は呼び出しのたびに、何かをする前に行います。

1. **呼び出し元。** いまの拡張のidは、DAGONが入口で作った`CallFrame`から取り、呼び出し元からは取りません。ハンドラの外（フレームが無い）のサービス呼び出しはフォールトです。
2. **サービスのid。** レイヤーのサービスの表（ポートのカタログから生成する`const`、2.1）にあること。知らないidはフォールトで、`extension_faulted { fault: BadService }`と記録します。
3. **能力。** 能力に作用するサービスはハンドルの番号を取ります。番号は、その拡張がそのサービスの種類で宣言した能力の数（`boot_wiring.rs`から）より小さいこと。能力自身の制限（デバイスの書けるレジスタ、ピンの許すレベル、ペリフェラルの宣言した割り込みの頻度）は、サービスが確かめます。
4. **ポインタと長さ。** すべてのポインタは呼び出した拡張のデータかスタックのリージョンの中にあり、すべての`(pointer, length)`の範囲は、オーバーフローを確かめる演算でその中にとどまり、アラインメントはサービスに合い、長さはサービスの上限を超えないこと。DAGONは**ポインタを渡した呼び出しのあいだに**特権のバッファを通してコピーし、その呼び出しが戻った後に拡張のポインタを持ち続けることはありません。後の呼び出しがデータを求めるなら、もう一度ポインタを渡し、もう一度検証します。
5. **持ち時間。** サービス呼び出しは呼び出し元の持ち時間の中で動きます。プリエンプトできない部分が残りの時間で終わらないサービスは、始める前に`Busy`で拒みます。

#### 特権の操作と取り消し

サービスは、DAGONが呼び出しごとに`CallFrame`の操作のスロットに持つ明示的な状態（`OpState`: サービス、能力、持っている資源、段階）を持つ**特権の操作**です。サンドボックスのタイマが、クリティカルセクションの途中で特権の操作を打ち切ることはありません。サービスは2つの部分で動きます。

- **プリエンプトできない部分**。有界で、サービスごとに宣言します（カタログの`np_max_us`。実装の最悪の場合に照らしてテストが確かめます）。バスのミューテックスを取るか放す、ペリフェラルやDMAのチャネルをプログラムするか止める、割り込み元をマスクするかクリアする、最大`COPY_MAX`バイトをコピーする、です。タイマの割り込みをマスクするのはこの部分だけで、種類のクリティカルセクションの基本要素（下記）を使います。プリエンプトできない部分が残りの持ち時間を超えるサービスは、始める前に拒むので（検証5）、持ち時間を超えるのは最大でも`np_max_us`です。
- **プリエンプトできる部分**。転送の終わりを待つところです。サービスはタイマをマスクしたままスピンしません。待ちは拡張に`Pending`として返すか（非同期のサービス）、タイマをマスクせず`OpState`に取り消すべきものを記録して行います。

呼び出しが異常に終わると（タイマかフォールト）、4.3のアンワインドが、DAGONに戻る前に`OpState`について**取り消しの手順**を実行します。この順で、各ステップは有界です。操作が触れたハードウェアがアイドルになるまで、何も解放しません。

1. 操作が始めたDMAのチャネルを止めて無効にし、コントローラがチャネルのアイドルを報告するまで待ちます（種類の記録の`dma_stop_max_us`が上限です。時間内に止まらないコントローラは`extension_faulted { fault: DmaStuck }`と記録し、チャネルは手順3がペリフェラルを初期化し直すまでリセットのまま保ちます）。
2. 能力の割り込み元を、割り込みコントローラとペリフェラルでマスクしてクリアします。
3. ポートの既定を通してペリフェラルを静止させます（転送を中止し、FIFOを空にし、ブロックをアイドルに戻します。DMAが止まらないときは、リセットのブロックでペリフェラルとチャネルをリセットします）。
4. 呼び出しのバウンスバッファと特権のキューの項目を回収します（DAGONが持ち、拡張で索引し、拡張のポインタでは索引しません）。
5. 持っていればバスのミューテックスを放します。拡張の転送がもうバスに触れられないからです（バスは手順3でポートの既定が残したままなので、DAGONの次の使用は既知の状態から始まります）。
6. `OpState`をクリアします。

同じ手順が通常の呼び出しの終わりにも、拡張が残したもの（始めて回収しなかったDMA）について動きます。そのため、呼び出しは必ず、拡張の資源が動いていない状態で終わります。したがって、呼び出しをまたぐ操作（非同期の入出力）がまたげるのは、**1回のハンドラの呼び出しの中**だけです。1回のハンドラの呼び出しで始めた`DMA_RX`は、その呼び出しが戻る前に回収するか取り消します。

プリエンプトできない部分は、種類の能力の記録（`critical_section`、4.4）に記録されたクリティカルセクションの基本要素で、サンドボックスのタイマをマスクします。ARMv6-Mでは`PRIMASK`（すべての割り込みなので、この部分の上限はレイヤーの割り込みの遅れの上限でもあり、`np_max_us`は小さくなります）。ARMv7-MとARMv8-Mでは、タイマとすべての能力の割り込みをマスクし、DAGONのフォールトは許したままの優先度のしきい値に設定した`BASEPRI`。RISC-Vでは`mstatus.MIE`のクリア（または`mie`のタイマのビット）。Xtensaでは、割り込みレベルをタイマより上げます。各サービスの上限`np_max_us`は、その種類での実装の測定した最悪の場合に照らしてテストで確かめます。

**リセット時の取り決め。** ロックアップ、ウォッチドッグによるリセット、電断は、上の手順を実行せずに呼び出しを終わらせます。そのため、すべてのリセットの後、ほかのものが使う前に、レイヤーの起動が、イメージのどの拡張のどの能力が触れた可能性のあるペリフェラル、DMAのチャネル、割り込み元もすべて初期化し直します。方法は種類の記録の`reset_cleanup`が示し（マイコンにリセットのブロックがあればそれを通す`peripheral-reset`、無ければ各ポートの既定を通す`reinit`）、ブロックの一覧はイメージのマニフェストから来ます。そのため、後始末は保護マップと一緒に生成され、最初のサービス呼び出しより前に、`boot.hal_ready`（ブートローダ）と`app.bring_up`の前（アプリ）で動きます。DAGON自身が使うペリフェラル（リンク、LED）は、どのリセットの後でもDAGON自身の立ち上げが初期化し直します。適合試験のビルドは、各サービスのプリエンプトできない部分の中からウォッチドッグによるリセットを起こし、次のブートで同じペリフェラルとチャネルの最初の使用が成功することを確かめます。

#### サービス

完全な一覧はポートのカタログから生成します。種類は次のとおりです。

| 種類 | サービス | プリエンプトできない部分 | `OpState`の状態 | 取り消し |
|---|---|---|---|---|
| Return | `RETURN(result)` | なし | なし | なし |
| Events | `LOG(ptr, len)` | シンクのバッファへ ≤ `COPY_MAX`のコピー | なし | なし |
| Pins | `GPIO_SET(cap, level)`、`GPIO_GET(cap)` | レジスタの書き込みか読み出し1回 | なし | なし |
| Buses | `BUS_WRITE(cap, reg, ptr, len)`、`BUS_READ(cap, reg, ptr, len)` | ミューテックスを取り、転送を始める。**この呼び出しの中で**、デバイスの`max_transfer`以下を拡張のバッファとコピーする | ミューテックスの保持、転送中 | 手順2、3、5 |
| Transport | `IO_TX(cap, ptr, len)`、`IO_RX(cap, ptr, len) -> n`、`IO_POLL(cap) -> flags` | 拡張のバッファと特権のキューのあいだのコピー。割り込み元の許可か禁止 | キューのほかになし | 手順2 |
| DMA | `DMA_TX_START(cap, ptr, len) -> op`、`DMA_RX_START(cap, len) -> op`、`DMA_STATUS(op) -> Pending \| Done(n) \| Failed`、`DMA_COLLECT(op, ptr, len) -> n`、`DMA_CANCEL(op)` | `START`: 拡張のバッファをバウンスバッファにコピーし（TX）、チャネルをプログラムする。`COLLECT`: バウンスバッファから ≤ `n`バイトを、この呼び出しに渡され**検証し直した**拡張のバッファにコピーする。`CANCEL`: 手順1–4 | チャネル、バウンスバッファ、終わったバイト数 | 手順1–4。回収していないRXは捨てます |
| Time | `NOW_US()`、`BUDGET_LEFT_US()` | なし | なし | なし |
| Memory | `SCRATCH(len) -> ptr` | なし（拡張自身のリージョンのバンプポインタ） | なし | バンプポインタは呼び出しの終わりにリセットします |

したがって、`DMA_RX`が完了時に拡張のメモリに書くことはありません。コントローラはDAGONのバウンスバッファに書き、拡張は`DMA_STATUS`（または能力のキューのフラグ）で完了を知り、`DMA_COLLECT`が、拡張の指定しDAGONがそのとき検証したバッファにコピーします。拡張のポインタが、渡した呼び出しより長く生きることはありません。

#### 取り消しの境界での適合試験

取り消しの列が空でないすべてのサービスについて、マイコンの定義のコードの適合試験のビルドは、各境界（ミューテックスの前、ミューテックスの保持中、転送の開始後、完了の後で回収の前、回収中、回収の後）でタイマが発火するようにして呼び出しを実行し、次のことを確かめます。DAGONの次の同じバスやチャネルの使用が成功すること。呼び出しの終わりの後で拡張のメモリに1バイトも届かないこと。バウンスバッファが回収されること（次の呼び出しの再利用に古いデータが見えないこと）。能力の割り込みがマスクされ、次の呼び出しだけが再び許可すること。持ち時間の超過が最大でも`np_max_us`であること。実行可能なモデル（5.5）は、ブートのシナリオで同じ境界を実行します。

### 4.3 プリエンプション、フォールト、アンワインド

サンドボックスのタイマ（`[isolation.capabilities] timer`: Cortex-MではSysTick、RISC-Vでは`mtimecmp`かプラットフォームのタイマ、Xtensaではタイマグループのアラーム）は、各呼び出しの前に、その持ち時間で、設定できる最高の優先度（どの能力の割り込みよりも、フォールトでないDAGONのどのハンドラよりも上）で有効にします。サービスのプリエンプトできない部分は、種類のクリティカルセクションの基本要素で最大`np_max_us`のあいだこれをマスクします（4.2）。タイマのハンドラ、フォールトのハンドラ、`RETURN`のサービスは、どれも同じように呼び出しを終えます。手順はどのアーキテクチャでも同じで、関わるレジスタは種類のアンワインドのモジュール（`[isolation.capabilities] unwind`: `cortex-m-exception`、`riscv-trap`、`xtensa-world`、`armv8m-tz`）から来ます。

1. **スタックされたフレームが使えるか決めます。** Cortex-MのARMv7-MとARMv8-M: `MSTKERR`/`STKERR`が立っていれば使えません（入口のスタッキングがフォールトしました）。ARMv6-M: プロセスのスタックポインタが拡張のスタックのリージョンの外か、アラインされていなければ使えません。RISC-V: トラップはスタックしないので（レジスタはそのままで、`mepc`/`mcause`/`mtval`はCSRです）、フレームは動いているレジスタファイルで、常に使えます。Xtensa: World 1のスタックポインタがリージョンの外なら使えません。手順は使えないフレームを決して読まず、それ以外でもフレームに依存しません。続きは、拡張が届かない特権のRAMにある`CallFrame`から来ます。
2. **帰属を決めます。** 呼び出しの拡張のidは`CallFrame`にあります。フォールトの種類と、アーキテクチャが報告するところではフォールトしたデータのアドレスを足します（ARMv7-MとARMv8-M: `CFSR`と、`MMARVALID`が立っているときだけ`MMFAR`、それ以外はアドレスなし。RISC-V: `mcause`と`mtval`。Xtensa: `EXCCAUSE`と`EXCVADDR`）。ARMv6-Mにはフォールトのステータスやアドレスのレジスタがありません（すべてのフォールトがHardFaultです）。そのため、記録はフレームが使えるときはスタックされた`PC`とその属するリージョンを、それ以外は`PC: unknown`を持ちます。`CallFrame`が開いているときは、フォールトは常に拡張に帰属させます。
3. **取り消します。**`OpState`について4.2の取り消しの手順を実行します（DMAのアイドル、割り込みのマスク、ペリフェラルの静止、バッファの回収、その後でミューテックスの解放）。
4. **アンワインドします。** ハンドラ自身の（特権の）スタックの上で行います。`CallFrame`から呼び出し先が保存するレジスタを戻し、特権のスタックポインタを保存したものに設定し、DAGONの特権の保護マップ（バックグラウンドのマップで、拡張のリージョンはすべて無効）を読み込み、戻る文脈の特権モードを戻し、打ち切った呼び出しの遅延保存された浮動小数点の文脈を捨てます（Cortex-M4F/M7: `FPCCR.LSPACT`をクリアし、基本フレームで戻ります。ブートローダはFPUを無効にして動くので、拡張のFP命令は`NOCP`のフォールトです。拡張は、どのレイヤーと種類でもソフトフロートでコンパイルします）。
5. **戻ります。** 例外からDAGONの続きへ戻ります（Cortex-M: メインスタック上の合成した基本の例外フレームと、特権のThreadモードの`EXC_RETURN`。RISC-V: `mepc`を続きに、`mstatus.MPP`をMモードに設定して`mret`。Xtensa: 続きでWorld 0へ戻ります。ARMv8-M TrustZone: Secureのスタックを通してSecureの続きへ）。続きは呼び出しの結果（`Returned(result) | Overran |
   Faulted(kind) | Disabled`）を`CallFrame`から読んで先に進むDAGONのコードです。戻らなかったポートの呼び出しはポートのエラー値を返し、戻らなかった通知はそこで終わりです。
6. **記録します。** 手順2の帰属とともに、`extension_ran | extension_overran | extension_faulted`。

タイマとフォールトが同時のとき: どちらのハンドラも、`CallFrame`の状態で守った1つのアンワインドの処理を通ります。後から動いたものはフレームが閉じているのを見て、アンワインドせずに戻ります。

#### ロックアップとヒューズ

拡張がスタックポインタをリージョンの外に動かしたとき（スタックの演算は非特権です。システム命令でスタックポインタのレジスタに書くのは、対応するどの種類でも特権だけです）の、タイマやフォールトの例外の入口でのフォールト: ARMv7-M/ARMv8-Mでは、プロセッサはスタッキングのエラーを示してハンドラに入り、上の手順があてはまります。RISC-Vではハードウェアは何もスタックしないので、手順があてはまります。ARMv6-Mでは、昇格したHardFaultが同じ不正なポインタにスタックし、コアはロックアップします。Xtensaでは、不正なスタックでのWorld切り替えの例外は特権のスタックで処理します。ロックアップはウォッチドッグが終わらせ（ブート時はどの拡張も動く前に有効化、4.1。アプリではアプリのガード）、4.2のリセット時の取り決めが、呼び出しが触れたものを初期化し直します。

ロックアップがリセットのたびに繰り返さないように、DAGONは保持メモリに**ロックアップヒューズ**を持ちます。前の実行が異常に終わったときにサンドボックスの呼び出しがどのイメージのどの拡張を動かしていたかを示す記録で、すべてのレイヤーの起動が読み、その拡張を無効にして動くために使います。ヒューズは保持メモリの状態で、その正しさはあらゆる時点のリセット、識別の衝突、イメージと拡張の新しいバージョンの書き込みのもとでのバイト単位の動きに依存します。そのため、ブート記録と同じく、文章では定めず、要件（5.5）と、それを通った形を形式とする実行可能なモデルで定めます。設計がここで決めることは次のとおりです。

- すべてのレイヤー（ブートローダ、アプリ、リレー）のすべてのサンドボックスの呼び出しを覆います。
- 確定したアプリの拡張がコアをロックアップさせたら、その拡張を無効にして戻り、応答を続けます。`commit`の後のブートローダのロックアップが失わせるのはそのブートの試行だけで、ヒューズによって、それは3回ではなく1回の試行になります。
- ヒューズを読み書きするのはレイヤーの起動だけです。ブート記録ではなく、`plan`、`commit`、`self_check`、`confirm`の入力でもありません。
- トリップしたヒューズは、VERSION（0x07）と`dagon status`（"extension <id> disabled after a lockup"）で報告し、それが示す識別が動いているイメージにもう無くなったときにクリアします。クリアする要求はありません。プロトコルにもCLIにも無いためです（5.5）。

DAGON自身のフォールトハンドラの中のフォールトは、診断コード付きのDAGONの停止です。

### 4.4 保護の種類、定義のスキーマ、生成されるマップ

マイコンの定義は保護ハードウェアをデータとして宣言します。

```toml
[isolation]                   # what the MCU and its package give DAGON
level = "mpu"                 # the level DAGON enforces: none | mpu | pms | tz (4.5)
kind = "armv7m-mpu"           # armv6m-mpu | armv7m-mpu | armv8m-mpu | armv8m-tz | riscv-pmp | riscv-epmp | esp-pms | none
boot_extensions = true        # the MCU has a DAGON boot stage
ext_ram_max = 24576
root_protection = ["rdp1", "rdp2", "wrp"]   # the MCU's root-image protection levels (4.7): rdp1, wrp reversible; rdp2, esp-secure-boot, esp-flash-encryption permanent
cache = "none"                # none | i-cache | id-cache (instruction and data) (4.10)
tcm = []                      # tightly coupled memories, each { at, size } (4.10)

[isolation.capabilities]      # the kind's capability record: what the unit implements, read back from the hardware and checked (below)
mpu_regions = 8               # armv6m/armv7m/armv8m-mpu: the unit's region count (MPU_TYPE.DREGION)
min_region = 32               # bytes (256 on armv6m-mpu implementations such as the RP2040)
subregions_from = 256         # 0 if none
# armv8m-tz instead: sau_regions, s_mpu_regions, ns_mpu_regions, idau = [{ base, size, attribution }]
# riscv-pmp instead: pmp_entries (0 | 16 | 64), grain (the smallest NAPOT size the implementation honours), na4 = true|false, tor = true|false
# esp-pms instead: worlds, gdma_channels, and optionally memory_regions, peripheral_slots
critical_section = "basepri"  # primask | basepri | mstatus-mie | xtensa-level: how the non-preemptible part masks the sandbox timer (4.2)
reset_cleanup = "peripheral-reset"   # peripheral-reset | reinit: how the reset-time contract of 4.2 is met
background_privileged = true  # the unit can give privileged code a default map
unwind = "cortex-m-exception" # cortex-m-exception | riscv-trap | xtensa-world | armv8m-tz
timer = "systick"             # the sandbox timer
fault_address = true          # the unit reports the faulting data address
# np_max_us, dma_stop_max_us: left out = the catalog's bounds (50, 20); a definition may only lower them

[[peripheral]]                # every register block the MCU has
id = "can1"
base = 0x4000_6400
size = 0x400
window_eligible = true        # self-contained, no memory writes, no routing, not shared with DAGON

[[peripheral]]
id = "dma1"
base = 0x4002_6000
size = 0x400
window_eligible = false
bus_master = true             # it can write memory
privileged_why = "dma"        # dma | flash | watchdog | clock | reset | irq-routing | pinmux | power | rom | core-control | protection | shared

[[core]]                      # every processor core; DAGON runs on the first
id = "cpu0"
```

書き方: 定義の値は`a-z 0-9 -`のケバブケースの語です（そのため`id-cache`です）。プロジェクトの定義の、単純な名前でない文字列は、PCの外に出る前に置き換えるからです（[マイコンとボードの定義](/reference/definitions/)）。

能力の記録は2度確かめます。`dagon gen`は定義の数値に基づいて計画し、種類の許す集合（下記）の外の数値を持つ定義を受け付けません。ブート時と適合試験のビルドでは、マイコンの定義のコードがユニット自身のレジスタ（Cortex-Mの`MPU_TYPE`、SAUのタイプレジスタ、RISC-Vの書ける`pmpcfg`のエントリの数と読み返せる最小のNAPOTの大きさ、PMSのバージョンのレジスタ）を読み、ハードウェアが記録を裏付けないレベルを有効にしません（`boot.isolation_mismatch`、レベルは`none`と報告します）。

検査器は、`kind`が`none`でないすべての定義に次のものを求めます。`flash`、`watchdog`、`clock`、`reset`、`irq-routing`のそれぞれについて`privileged_why`を持つ`[[peripheral]]`が少なくとも1つ。`bus_master = true`の項目はすべて`window_eligible = false`。`[[core]]`が複数あるマイコンでは`core-control`のブロック。保護の設定のレジスタブロックを持つ種類（`esp-pms`、`riscv-epmp`）では`protection`のブロック。どれかを欠く定義は、欠けた区分の名前を示して受け付けません。DMAコントローラの場所を定義が示さないマイコンに、`none`より上のレベルは与えられません。

マップの生成器と検査器は`kind`についてジェネリックです。種類ごとに1つのモジュールが1つのトレイトの後ろにあり、`dagon gen`とテストが使います。

```rust
pub trait ProtectionKind {
    fn kind(&self) -> Kind;                                                      // the definition's `kind` it serves
    fn plan(&self, def: &McuDef, space: &Space, exts: &[ExtLayout]) -> Result<Map, Refusal>;   // regions / entries / worlds inside the image's flash body and RAM (`space`)
    fn check(&self, def: &McuDef, map: &Map) -> Vec<Violation>;                  // alignment, overlap, attributes and memory type, privileged blocks and memory outside every window, fixed regions present
    fn unwind(&self) -> Unwind;                                                  // the definition's `unwind` value for the kind
    fn guarantees(&self) -> Guarantees;                                          // what 4.5 prints for the level
    fn read_back(&self, def: &McuDef) -> Vec<ReadBack>;                          // registers the MCU package reads at boot and in conformance, with the values the record states
}
```

`plan`は、`.ext.text`、`.ext.data`/`.ext.bss`、スタックを、種類のアラインメントのルールが許すアドレスに置き（リンカスクリプトは同じ結果から生成します）、宣言されたウィンドウを割り当て、エントリが収まらないときははっきりしたメッセージで拒みます（`extension
uds-can needs 5 windows; 3 fit after code, data and stack on a
8-region armv6m-mpu`）。`check`は、マイコンの定義のコードのテストで、生成された表と定義に照らして動きます。適合試験のビルドは、デバイス上の拡張から各特権のブロックへの書き込みを実行し、フォールトのイベントを期待します。既知の種類のマイコンを足すときにコードには触れません。種類を足すときはモジュールを1つ足します。

種類ごとに、ハードウェアが与えるものと生成器の扱いは次のとおりです。

| 種類 | ユニット | 能力の記録と形 | フォールトの報告 | 保証すること | できないこと |
|---|---|---|---|---|---|
| `armv6m-mpu` | ARMv6-M PMSAv6 MPU | `mpu_regions`（実装の選択で、`MPU_TYPE`から読み返します。RP2040では8）、2のべき乗の大きさ、大きさに揃えたベース、実装の`min_region`（RP2040では256 B）、256 Bからの8サブリージョン、`PRIVDEFENA`。`critical_section = primask`（このアーキテクチャに`BASEPRI`はないので、プリエンプトできない部分はすべての割り込みをマスクし、`np_max_us`がレイヤーの割り込みの遅れの上限です） | すべてのフォールトがHardFault。ステータスやアドレスのレジスタはありません。帰属は`CallFrame`とスタックされたPC（4.3） | 有効なリージョンの外のすべてへの非特権のアクセスの拒否。リージョンごとのXN | 例外のスタッキング中のフォールトでコアがロックアップします（ロックアップヒューズ、4.3）。ウィンドウは`min_region`以上の揃ったブロックでなければなりません |
| `armv7m-mpu` | ARMv7-M PMSAv7 MPU | 実装の`mpu_regions` ∈ {8, 16}（Cortex-M3/M4: 8。Cortex-M7: 8か16、読み返します）、2のべき乗、大きさに揃えたベース、最小32 B、256 Bからのサブリージョン、`PRIVDEFENA`。`critical_section = basepri` | MemManageと`CFSR`、`MMARVALID`のとき`MMFAR`。スタッキングのエラーは示され、ロックアップしません | 上と同じで、データのフォールトのアドレスが付きます | ウィンドウは2のべき乗のブロックです。キャッシュのあるコアでは4.10のルールがあてはまります |
| `armv8m-mpu` | ARMv8-M PMSAv8 MPU | 実装の`mpu_regions` ∈ {0, 4, 8, 12, 16}（読み返します）、32 Bの粒度のベースとリミットの組（2のべき乗のルールもサブリージョンもありません）、`PRIVDEFENA` | MemManageと、有効なとき`MMFAR` | `armv7m-mpu`と同じで、ウィンドウは32 Bに揃った任意の範囲です | リージョン0は`none`です |
| `armv8m-tz` | Security Extensionを持つARMv8-M | 独立した3つの記録: `sau_regions` ∈ {0, 4, 8}、`s_mpu_regions`と`ns_mpu_regions`はそれぞれ ∈ {0, 4, 8, 12, 16}、すべて読み返します。そしてマイコンの固定の帰属の`idau`の表と、定義が挙げる`mpc`/`ppc`のユニット。DAGONはSecure、拡張はNS-MPUの下でNon-secureかつ非特権です。サービスのABIはSecureのゲートウェイです（NSCのリージョンの`SG`のベニヤ） | SecureとNon-secureのフォールト、`SecureFault` | もっとも強いものです。Secureのメモリは、Non-secure状態で特権であっても拡張には見えません。バスマスタはインターコネクトがSecure/Non-secureに帰属させ、定義が各マスタの帰属を挙げます | IDAU/MPC/PPCの設定はマイコン固有のデータです。インターコネクトが帰属させられないマスタはSecureだけにとどめます。帰属はIDAUだけでも（完全な`idau`の表を持つ`sau_regions = 0`は有効な`tz`の定義です）、IDAUとSAUでもかまいません。レベルが`none`になるのは、どちらも拡張のリージョンを帰属させられないときだけです |
| `riscv-pmp` | RISC-V PMP | 実装の`pmp_entries` ∈ {0, 16, 64}（どの`pmpcfg`のエントリに書けるかを調べて読み返します）、アドレスモードNA4（4 B、`na4`のとき）、NAPOT（8 Bから、実装の`grain`を読み返します）、TOR（粒度の上の任意の範囲、`tor`のとき）。拡張はUモード、DAGONはMモード。`critical_section = mstatus-mie` | `mcause`、`mtval`（アドレス）、ハードウェアのスタッキングなし | Uモードに対する、与えていないすべての拒否。エントリごとのXN | ePMPが無いと、MモードはPMPで制限されません。エントリは少なく、TORは境界ごとに項目を消費します。エントリ0は`none`です |
| `riscv-epmp` | RISC-V ePMP（Smepmp） | PMPと同じで、`mseccfg`のマシンモードのロックダウンと共有リージョンが加わります | PMPと同じ | PMPと同じで、DAGONはMモードが拡張のメモリから実行することを禁止できます | |
| `esp-pms` | EspressifのPermission Control + World Controller | 実装の`worlds`、`memory_regions`、`peripheral_slots`、`gdma_channels`（PMSのバージョンとリージョンのレジスタから読み返します）。`critical_section = xtensa-level`。内部メモリのリージョン、MMUを通した外部メモリ、各ペリフェラル、各GDMAチャネルについてのWorldごと（World 0はDAGON、World 1は拡張）の権限。コアごとに適用し、ロックできます | `EXCCAUSE`/`EXCVADDR`、World切り替えの例外 | World 1に対する、与えていないすべての拒否。すべてのコアで、GDMAを含みます | PMS/WCLのレジスタはDAGONのマイコンの定義のコードがプログラムする必要があります（HALのドライバはありません） |
| `none` | 使えるユニットが無い | | | 時間の測定、イベントの記録 | メモリ、フラッシュ、ウォッチドッグ、ハング（4.5） |

定義が主張できるレベルは種類から決まります。MPUとPMPの種類は`mpu`、`esp-pms`は`pms`、`armv8m-tz`は`tz`、それ以外は`none`です。

### 4.5 マイコンごとの隔離のレベルと3つの例

`dagon-mcu.toml`は、そのマイコンでDAGONが強制するレベルを宣言します。VERSIONがそれを報告するので（5.4）、ホストはノードの隣に表示します。

| レベル | 保証 | 保証しないこと（文書、VERSION、UIで示します） |
|---|---|---|
| `mpu` | 4.1–4.4のすべて: 非特権の実行、DAGONのコードとデータの拒否、サービス呼び出しの検証、能力のウィンドウ、特権のDMAと割り込み、タイマのプリエンプションとアンワインド、フォールトからの回復、ロックアップヒューズ | 宣言された能力の物理的な効果（4.6） |
| `pms` | パーミッションコントローラとWorldの切り替えで同じことを、すべてのコアで、チャネルごとのGDMAの拒否とともに | 同じ |
| `tz` | Security Extensionで同じことを。さらに、DAGONのメモリはどのNon-secureのソフトウェアにも見えません | 同じ |
| `none` | 時間の上限を測って報告しますが（`extension_overran`）、プリエンプトしません。能力のハンドルは宣言どおりに渡します。イベントは記録します。ハードウェアの拒否はありません。拡張は、不具合や意図によって、フラッシュに書く、ウォッチドッグやDAGONのメモリに触れる、ウォッチドッグがチップをリセットするまでハングすることができます（ブート時は、`commit`の後に起きたときだけ、試行を失います） | メモリとフラッシュの拒否、時間の上限、誤動作する拡張に対する判断、署名、ロールバック、ウォッチドッグのルール。`dagon status`とページは、そのノードに"extensions unguarded"と表示し、`dagon doctor`は、プロジェクトがそこに信頼されていない拡張をリンクすると警告します |

DAGONが定義を配布している3つのマイコンを、スキーマの例として挙げます（規範となるマップは、`targets/mcu/*/dagon-mcu.toml`の定義から`dagon gen`が生成するものです。下の事実はベンダーのリファレンスマニュアルから取り、立ち上げ時にユニットのタイプレジスタから読み返します）。

**STM32F401RE**: `kind = "armv7m-mpu"`、`mpu_regions = 8`（`MPU_TYPE`から読み返します）、`min_region = 32`、`subregions_from = 256`、`fault_address = true`、`unwind =
"cortex-m-exception"`、ブート時とアプリで`level = "mpu"`（ST PM0214 4.2節MPU、ブロックのアドレスはST RM0368 2節のメモリマップ）。ウィンドウ2つの拡張1つについて生成されるマップは次のとおりです。

| リージョン | ベース | 大きさ | 属性 | 目的 |
|---|---|---|---|---|
| 0 | イメージの本体の中の`.ext.text`、大きさに揃える | 2 KiB … 32 KiB | 非特権RO、X | 拡張のコードと定数 |
| 1 | RAMの`.ext.data` + `.ext.bss`、揃える | 1 KiB … 16 KiB | 非特権RW、XN | 拡張のデータ |
| 2 | 拡張のスタック、揃える | 1 KiB … 8 KiB | 非特権RW、XN。いちばん下のサブリージョンは無効 | ガード付きのスタック |
| 3–4 | 能力のウィンドウ: それぞれ1 KiBのAPB/AHBのブロック1つ | 1 KiB | 非特権RW、XN | 宣言されたペリフェラル（ウィンドウにできるもの） |
| 5–6 | ウィンドウを増やすために空き | | | |
| 7 | `ram_bytes`が2のべき乗でないときの2つ目のデータのリージョンのために予約 | | | |

特権（`window_eligible = false`）: フラッシュインターフェース`0x4002_3C00`（`flash`）、IWDG `0x4000_3000`とWWDG（`watchdog`）、RTCとバックアップ`0x4000_2800`（`power`）、RCC `0x4002_3800`（`clock`、`reset`）、PWR、SYSCFGとEXTI `0x4001_3800`/`0x4001_3C00`（`irq-routing`、`pinmux`）、DMA1 `0x4002_6000`とDMA2 `0x4002_6400`（`dma`、バスマスタ）、USB OTG FS `0x5000_0000`（DAGONのリンクと共有）、CRC、システムメモリ`0x1FFF_0000`（`rom`）、オプションバイト、PPB。

**RP2040**: `kind = "armv6m-mpu"`、`mpu_regions = 8`（`MPU_TYPE`から読み返します）、`min_region = 256`、`subregions_from = 256`、`fault_address = false`、`unwind =
"cortex-m-exception"`、2つの`[[core]]`、HardFaultの帰属とロックアップヒューズを伴い、ブート時とアプリで`level = "mpu"`（RP2040データシート2.4節のCortex-M0+の機能とMPUのタイプレジスタ、Arm ARMv6-M Architecture Reference Manualの例外モデル）。生成されるマップは次のとおりです。

| リージョン | ベース | 大きさ | 属性 | 目的 |
|---|---|---|---|---|
| 0 | イメージのスロットの本体の中の、XIPの領域の`.ext.text`、大きさに揃える | 1 KiB … 32 KiB | 非特権RO、X | 拡張のコードと定数 |
| 1 | SRAMの`.ext.data` + `.ext.bss`、揃える | 256 B … 16 KiB | 非特権RW、XN | 拡張のデータ |
| 2 | 拡張のスタック、揃える | 1 KiB … 8 KiB | 非特権RW、XN。いちばん下のサブリージョンは無効 | ガード付きのスタック |
| 3–5 | 能力のウィンドウ: それぞれ16 KiBのAPBのブロック1つ（UART、SPI、I2C、PWM、ADC、PIO） | 16 KiB | 非特権RW、XN | 宣言されたペリフェラル（ウィンドウにできるもの） |
| 6 | 2つ目のデータのリージョンのために予約 | | | |
| 7 | ウィンドウの中の部分ブロックの拒否のために予約 | | アクセス不可 | |

特権: ブートROM `0x0000_0000`（`rom`。そのフラッシュのルーチンにはROMを実行しないと届かず、ROMは全体が拒否されます）、XIPの制御`0x1400_0000`とSSI `0x1800_0000`（`flash`）、DMA `0x5000_0000`（`dma`、バスマスタ）、USB `0x5010_0000`（自分のDPRAMへのバスマスタで、DAGONのリンク）、WATCHDOG `0x4005_8000`（`watchdog`）、CLOCKS `0x4000_8000`（`clock`）、RESETS `0x4000_C000`（`reset`）、PSM `0x4001_0000`とSIO `0xD000_0000`（`core-control`: 2つ目のコアをDAGONは起動せず、起動できるブロックは特権なので、1コアのマップで足ります。GPIOの出力はSIOにあり、サービスです）、IO_BANK0/QSPIとPADS `0x4001_4000`–`0x4001_C000`（`pinmux`）、XOSC/PLL/ROSC、VREG（`power`）、TIMER、RTC、PPB。

**ESP32-S3**: `kind = "esp-pms"`、2つの`[[core]]`、`boot_extensions =
false`（ベンダーのstage 2で、DAGONのブート段階はありません）、`fault_address = true`、`unwind = "xtensa-world"`。アプリで`level = "pms"`です。HALはドライバを公開していないので、マイコンの定義のコードがPMSとWCLのレジスタをプログラムします（Espressif ESP32-S3 TRMの"World Controller"と"Permission Control"の章）。`pms`で生成されるマップは次のとおりです。

| 資源 | World 1（拡張） | 補足 |
|---|---|---|
| 内部SRAM（またはMMUを通したIROM）の`.ext.text`のリージョン | 読み、実行 | |
| `.ext.data`、スタックのリージョン | 読み、書き、実行不可 | スタックの下の拒否されたリージョンがガードです |
| 宣言されたペリフェラル | 読み、書き | 定義の中のウィンドウにできるものの一覧 |
| ほかのすべて: DAGONのコードとデータ、ROM、SPI0/SPI1（キャッシュとフラッシュ、`flash`）、RTCとTIMGのウォッチドッグ（`watchdog`）、RTC_CNTL（`power`、`reset`）、システムと割り込みマトリクスのレジスタ（`irq-routing`、`clock`）、GDMA（`dma`、バスマスタ）、USB OTG（バスマスタ）、AES/SHA/RSAのエンジン（GDMAを通すバスマスタ）、両コアの制御レジスタ（`core-control`）、PMSとWCLのレジスタ（`protection`） | アクセス不可 | GDMAのパーミッション制御は、どのチャネルからのWorld 1のメモリアクセスも拒否します。マップは両コアに適用し、World 1に入る前にロックします |

### 4.6 サンドボックスが保証できないこと

どのレベルでも保証しないのは、宣言された能力が物理的な世界でできることです。マニフェストが宣言したピンは、配線が許せばボードの電源を切れます。宣言したデバイスの書けるレジスタへのバスの書き込みは、そのデバイスの状態を変えられます。それぞれの役目は次のとおりです。

- マニフェストが能力を宣言します。
- `dagon gen`が、プロジェクトのすべての拡張の宣言された能力を表示します。
- ボードの定義がバスの能力をデバイスとレジスタの集合に、ピンを許すレベルに絞ります。
- 配線はボードの作者の責任です。

ペリフェラルをウィンドウにできるかどうかは、メモリに書けず、割り込みやクロックを経路づけられず、DAGONと状態を共有しないという、マイコンの定義の表明です（11）。バスマスタをウィンドウにできると印を付けた定義を、検査器は受け付けません。

保証が覆うのはDAGONの公開APIとサンドボックスです。特権でリンクされるコード（DAGON自身、信頼された拡張）は、構造上その外です。

### 4.7 信頼された拡張と認証のチェーン

信頼された拡張だけのポート（2.1）を提供する拡張は、安全のルールを作っている部分を置き換えます。受け付けるのは、次のときだけです。

1. マニフェストが`trusted = true`と宣言し、提供する信頼されたポートを挙げること。
2. プロジェクトの`dagon.toml`が、ノードの`[extensions.nodes.<id>] trusted`の下に、idとcrateのソースのハッシュで挙げること。ハッシュはcrateのフォルダのすべてのファイルをパス（`/`区切り）の順に並べ、ファイルごとにパス、ゼロのバイト、長さ（リトルエンディアンのu64）、バイト列（CRLFはLFとして読む）を続けたもののSHA-256です。`include!`と`#[path]`でコンパイラがどれでも読めるので、`target/`と隠しフォルダを含め、除外するファイルはありません。そのため、拡張のフォルダの中でcargoを実行すると、ハッシュが変わります。ハッシュがソースのものなのは、`dagon gen`の実行時にリンクされたバイト列が存在しないからです。

   `dagon gen`はハッシュを確かめ（違うときは見つけたものを示します）、次にホストでブート用crateの依存グラフを確かめます。cargoがすべてのフィーチャ、すべてのターゲットで解決し（`Cargo.lock`ができた後は`--locked`）、これはどの1回のビルドよりも広い集合です。組み込まれるすべてのcrate（通常とビルドの依存）は、DAGONのもの、宣言された信頼された拡張のフォルダの中のもの（つまりそのハッシュの中）、またはブート用crateの`Cargo.lock`にチェックサムがあるレジストリのcrateでなければなりません。

   確認を通ると、genは`graph.stamp`を書きます。ブート用crateの`Cargo.toml`、`Cargo.lock`、各信頼された拡張のソースのハッシュのSHA-256です。ブートローダのビルドはこのスタンプを計算し直し、無いときや違うときはビルドしません。そのため、いまの入力についてグラフを確かめていないブート用crateがビルドされることはありません。グラフをビルドスクリプトではなくホストで確かめるのは、ビルドスクリプトからは外側のビルドのフィーチャも、オフラインではcargoが解決するすべてのcrateも見えないからです。

   覆わないもの: 拡張が依存するレジストリのcrate（チェックサムで固定しますが、レビューはしません）と、拡張が自分のフォルダの外から取り込むソース（外へ出るパスの`include!`や`#[path]`）です。これらは特権で動くコードとしてレビューします。
3. それを含むイメージがプロジェクトの鍵で署名され、署名が拡張のバイト列を覆い、イメージが動く前にデバイスがその署名を検証すること（下記）。
4. `dagon gen`、`dagon status`、`dagon doctor`、ページが、各ノードの信頼された拡張をidで表示し、VERSIONがそのidとソースのハッシュの最初のバイトを表示用に（0x08）、ハッシュ全体を識別用に（0x09）報告すること。

拡張はポートを提供せずに信頼されることもできます。マニフェストが`trusted = true`と宣言し、イベントを観測するだけの観測者です。上の項目2から4のもとで受け付け、判断点には答えず、ハンドラの呼び出しはそれぞれ、ほかの拡張と同じくマニフェストの`max_run_us`に照らして測ります（超えた呼び出しは`<layer>.extension_overran`と報告し、止めません）。理由は、プロジェクト自身のブート時のコード（ブートをディスプレイに表示する、自分のハードウェアを動かす）が、サンドボックスが拡張に禁じるもの、たとえばDAGONのマップが開かないバスやペリフェラルをまさに必要とすることが多いからです。信頼されたと宣言すると、すべてのプロジェクトのサンドボックスを弱める代わりに、そのプロジェクトでDAGONの一部になります。

信頼された拡張は特権で、DAGON自身のリージョンの中で動きます。そのプロジェクトではDAGONのその部分です。4.1–4.6のどれも、それに対しては成り立ちません。文書は信頼された拡張に触れるところではどこでもそう示します。

チェーンのすべての検証器（`ImageVerifier`と`Digest`のポートで、既定がこのルールです）が使う認証のルールは、次の1つです。

- **署名するバイト列。** イメージのヘッダの署名は、ヘッダのオフセット0から署名の欄の始まりまでのバイトを覆います。そこには、形式番号、イメージの種類、マイコンの識別、ターゲットのタグ、スロットのバインディング、バージョン、本体の長さ、本体のSHA-256が入ります。本体はそのダイジェストで認証され、本体はすべての`.ext.*`のセクションを含むので、拡張のバイト列も覆われます。
- **鍵の選択。** 検証器が受け付けるのは、ヘッダが示すidの鍵だけで、そのidがビルド時に検証するイメージに埋め込まれたプロジェクトの鍵のときだけです（`DAGON_PUBKEY`。公開のテスト鍵は、それを選んだビルドだけ）。デバイスに鍵の一覧はなく、予備の鍵もありません。
- **だれが何を検証するか。** エージェントは`written`の前にイメージを検証し、ブートローダは試行のブートで`chosen`の前に検証します（ダイジェストはすべてのブートで確かめます）。ブートローダのイメージは、エージェントがこのルールで`written`の前と`pending_set`の前にもう一度検証します（SLOT_PENDINGの記録がそのヘッダのダイジェストを示します）。

  定義が`[layout] stage0_verifies`を設定するstage0のあるself-bootのマイコンでは、stage0はダイジェストに加えて、組み込まれたプロジェクトの鍵でブートローダのヘッダの署名を確かめます。省くと、stage0はダイジェストだけを確かめます。ブートローダを起動するのは、ヘッダのダイジェストがブートローダの記録の示すもので、本体がそのヘッダのダイジェストと一致するときだけです。記録があるのはエージェントが署名を検証したコアだけなので、このチェーンが成り立つのは、状態ログを書くのがDAGONのエージェントとブートローダだけであるあいだです（拡張からはサンドボックス、4、が守ります）。署名の鍵なしでフラッシュに書けるコード（信頼されたコードの不具合、またはフラッシュに触れられる攻撃者）は、署名の無いコアをstage0に起動させられます。検証器は、下の余裕の範囲では、どちらのstage0の領域にも収まりません。

  ログの選択が確かめられないとき（壊れたスロット、またはコアの無いスロットを示す記録）は、stage0は起動するものが無くなる前に、ほかのスロットを単独で試します。

  stage0の無いself-bootのマイコンでは、ブートローダがルートで、書き込む前にホストが、書き込みの経路で書き込むときはエージェントが検証します。
- **stage0の上限。** マイコンの定義の`[layout] stage0.size`はstage0の全体です（ベクタテーブル、ログの読み手、検査、試行のセル）。イメージのmemory.xはちょうどその領域を与えるので、stage0がそれを超えるとリンクが失敗します。RP2040: boot2（ブートROMの256バイトの第2段階）を含めて16 KiB。STM32F401RE: 16 KiB、コアがリセットベクタを取るセクタ0のすべて（それを持つ最小の消去単位なので、どの書き込みもstage0を消去しません）。どのビルドも領域の1/16を空けなければなりません（`scripts/stage0_size.py`）。ボードの無いビルドと、定義がそのマイコンを示すボードごとに1つです。ボードはブートのLEDと停止のパターンをstage0に入れるからです。stage0に現場で書き込むことはありませんが、新しいノードごとにそのビルドのツールチェーンでビルドします。そのため、余裕の無いイメージは、ビルドできない最初の書き込みになります。どちらのstage0もコンパクトなSHA-256のラウンドを使います。展開したラウンドはRP2040の領域に収まらないためです。もっと必要なstage0は、最初の公開リリースの前により大きな領域を取ります。それ以前の形式に互換の約束はありません（6）。
- **試行のウォッチドッグの下で消去しない。** stage0はフラッシュに書きません。ブートローダは記録（BOOT_ATTEMPT、ROLLBACK、ブートローダのCONFIRMEDかROLLBACK）を、ログにすでにある場所にだけ書きます（`Log::append_in_place`）。エージェントは、イメージを待機にするときに、その試行のための場所を予約し（`Log::reserve`、`trial_room`）、電断で記録が途中で切れた後は、アプリのエージェントが起動時にそれを戻します（`SlotStore::keep_room`）。消去はアプリの中だけで、定義の確認が十分に長いと証明したウォッチドッグの周期の下で行います。`[watchdog]`の両方の周期は`[execution]
  erase_unit_max_ms`の2倍以上です。
- **試行のセル。** 試行のセルが書いたものを保持しないノード（`bootcore::cell_holds`、アプリのストアとブートローダが確かめます）は、ブートローダを受け取りません（XFERをunsupportedとして拒みます）。それが壊れていると気づいた待機中のコアは自分のロールバックを記録します。そのため、壊れたコアを回数なしで再試行することはありません。

- **ルートイメージ**（あればstage0、無ければブートローダ）は、準備のときにマイコンのROMローダで書きます。ROMローダはDAGONのものを何も検証しません。ROMローダを動かせる者はルートを置き換えられ、それとともにすべてのルールを置き換えられます。そのため、このチェーンが成り立つのは、DAGONが提供するすべての書き込みの経路（リンク、UDSのような置き換えた転送、親機のリレー）に対してで、ROMを通した物理的な書き直しに対してではありません。マイコンがルートイメージの保護を持つところでは、定義がそのレベルを挙げ（`[isolation]
  root_protection`: STM32では、`rdp1`は全消去で元に戻せ、`rdp2`は元に戻せず、セクタの書き込み保護`wrp`もあります。RP2040には何もなく、ブートROMは常にBOOTSELに従います。ESP32-S3にはセキュアブートとフラッシュの暗号化のeFuseがあります）、`dagon provision`がレベルの設定を提案してどれが元に戻せるかを示し、VERSIONが正確なレベルと保護されたセクタを報告します（0x07）。そのためホストは、ルートが置き換えられるノード、元に戻せる形で保護されたノード、元に戻せない形で保護されたノードを示します。

### 4.8 アプリ、リレー、ホスト

サンドボックスのアプリの拡張（レベル`mpu`/`pms`/`tz`）は、ハンドラについて4.1–4.4の保証を持ちます。レベル`none`のアプリの拡張が持つのは、APIの形の保証だけです。

| ルール | 強制するもの |
|---|---|
| ストアにアクセスしない | 拡張は、自分のリージョンにコピーされた読み取り専用のペイロードを受け取ります。アプリケーションは`ImageView`を得ます（2.3）。スロットのストアの変更メソッドには、イメージのライフサイクルの状態機械を通してだけ届きます。 |
| 検証と確定の順序 | 状態機械の遷移だけが状態を変えます。`self_check`に尋ねるのは、「試行中」のガードの後の`confirmed`の遷移の中だけです。`accept_update`はDAGONのガードの後だけです。拡張の答えは検証された`enum`（`RETURN`）で、DAGONが実行するクロージャではありません。プロトコルの拡張はトリガーを作り、セッションの状態機械がガードを実行します。 |
| 時間 | 判断の上限は3にあります。通知にも同じ上限があり、ハードウェアのレベルでは割り込み、`none`では測ります。 |
| ウォッチドッグ | どのサービスも触れません。 |
| 再起動 | 3.2: 遅れに上限があり、拒めません。 |

リレーの拡張はアプリの拡張です。ホストの拡張は利用者の権限で利用者のOSの中で動きます。データで定義するもの（アクション、エージェント、プログラムで指定する転送）は、共有のcommandの転送の上限のもとでプロセスの外で動きます。ホストの拡張は判断に答えません（3.4）。

### 4.9 適合試験のケース

マイコンの定義のコードの適合試験のビルドは、デバイス上の拡張から次のものを実行し、記録された結果を期待します。

- 定義の各特権のブロックへの書き込み（フォールト）
- DAGONのコードの読み出し（フォールト）
- DAGONのコードへのジャンプ（フォールト）
- 偽造したサービスのid（フォールト）
- リージョンの外のサービスのポインタとオーバーフローする長さ（拒否）
- サービス呼び出しの前の不正な非特権のスタックポインタ（種類の文書にある結果: スタッキングのエラーとアンワインド、またはロックアップ、ウォッチドッグによるリセット、ヒューズのセットと次のブートでの拡張の無効化）
- 持ち時間を超えるループ（超過）
- 宣言されたペリフェラルからの割り込みの嵐（割り込み元の無効化）
- ウィンドウにできないウィンドウを通したDMAの要求（生成時に拒否）
- 入れ子のフォールト（アンワインド1回）
- 同じ呼び出しのタイマとフォールト（アンワインド1回）
- マイコンにある別のコアの起動の試み（フォールト）
- `pms`のマイコンではWorld 1からのGDMAのアクセス（拒否）
- サービスについて: 4.2の取り消しの境界のケースと、リセット時の取り決めのケース（各プリエンプトできない部分の中のウォッチドッグによるリセットの後、最初の使用が成功する）
- ヒューズについて: そのモデルの要件（5.5）のシナリオをデバイスで実行します

ケースは定義から生成するので（特権のブロックごと、コアごと、バスマスタごと、サービスの取り消しの境界ごとに1つ）、新しいマイコンはその定義からケースを得ます。

各ケースは実行可能なモデル（5.5）のシナリオでもあり、ブートが拡張なしのときと同じ判断に達することを確かめます。

### 4.10 同じモデルでのほかのアーキテクチャ

モデルのマイコン固有の部分はすべて定義のデータで、それぞれトレイトの後ろに小さな閉じた仕組みの集合があります。

| 部分 | 定義のデータ | 仕組み（それぞれモジュール1つ） |
|---|---|---|
| 保護マップ | `[isolation] kind`とパラメータ、`[[peripheral]]`、`[[core]]` | `ProtectionKind`（4.4） |
| アンワインド | `[isolation.capabilities] unwind` | `cortex-m-exception`、`riscv-trap`、`xtensa-world`、`armv8m-tz` |
| サンドボックスのタイマ | `[isolation.capabilities] timer` | `systick`、`riscv-mtime`、`timer-group` |
| ウォッチドッグ | `[watchdog] kind, trial_ms, confirmed_ms` | 種類ごとの`WatchdogDriver`のポートの既定（`iwdg`、`rp-watchdog`、`rtc-wdt`など） |
| ブート記録の配置 | `[boot.record] kind, at, survives, rings, entropy`（entropy: 起動のたびに違う起動時のワードの出所。ブート記録には必須） | `noinit-ram`、`retained-registers`（5.5） |
| ロックアップヒューズ | `[boot.record] fuse`、`[watchdog] flag`（なし、またはリセットを越えて残り、起動が確認するウォッチドッグによるリセットのフラグの、レジスタかマイコンの定義のコードの出所） | ヒューズが使えるのはフラグがあるときだけです。それ以外は読み込みが使えないと報告します（5.5） |
| ルートの保護 | `[isolation] root_protection` | マイコンのファミリごと、`dagon provision`が適用します |
| キャッシュとTCM | `[isolation] cache, tcm` | ウィンドウの属性とバウンスバッファの配置（下記） |

最初の3つのマイコンには無いものの、同じスキーマに収まる種類は次のとおりです。

- **TrustZone-Mを持つARMv8-M**（Cortex-M23/M33/M55/M85）: 種類の表（4.4）の`armv8m-tz`の行が、その能力と保証のすべてです。表のほかにこのファミリに固有なのは、バスマスタがインターコネクトの帰属のユニットでSecureかNon-secureになることで、定義がマスタごとに挙げます（`bus_master_attribution = "secure" | "configurable"`）。拡張の入出力で使う設定可能なマスタは、DAGONのDMAのサービスをSecureとして通します。
- **RISC-V PMP/ePMP**（ESP32-C/Hシリーズ、RP2350のHazard3、ほか多数）: 種類の表の`riscv-pmp`と`riscv-epmp`の行が、項目、アドレスモード、保証を示します。表のほかに固有なのは、ESP32-C/Hの部品がペリフェラルとGDMAのためのEspressifのPMSも持つことで、その定義はメモリの`riscv-pmp`と`esp-pms`のペリフェラルとDMAの制御を組み合わせます（2つを合わせるモジュールの種類`riscv-pmp+esp-pms`）。
- **Cortex-M7とほかのキャッシュのあるコア**: 種類の表の`armv7m-mpu`の行です。定義は`cache = "id-cache"`とTCMの範囲を示します。生成器は能力のウィンドウをDevice（キャッシュなし）、コード、データ、スタックをNormalと印を付け、DMAのバウンスバッファをキャッシュされないリージョン（DTCMの範囲か、キャッシュなしの属性のMPUのリージョン）に置きます。DMAのサービスは、バッファがキャッシュされるときは送信の前にクリーンし、受信の後に無効化します。`.ext.text`はフラッシュかITCMに置け、アンワインドは変わりません。バウンスバッファやTCMのリージョンを置かない生成器は、データキャッシュやTCMを持つ定義を、無視せず、`plan`で欄の名前を示して拒みます。
- **DAGONが1つのコアを使うデュアルコアのマイコン**: もう一方のコアは`[[core]]`の下に挙げ、その起動とコア間のブロックは`core-control`です。マップは、種類がコアごとに設定するところ（`esp-pms`）ではすべてのコアに適用します。ユニットがコアごとで2つ目のコアを起動しないところでは、検査器が起動のブロックを特権にすることを求めます。

どの種類も保証しないもの: 宣言された能力の物理的な効果（4.6）、特権のコードのDAGON自身の不具合、ROMローダが書き直せるもの（4.7）。

## 5. ボードの値、アクションプログラム、共有の資源、VERSION、モデルで決める形式

### 5.1 ボードの値は定義から、マイコンのコードからは決して来ません

`dagon-board.toml`は`[boot]`のテーブルを持ちます。`led`（ピンの名前かGPIOの番号）、`led_active = "high" | "low"`、`halt = { pattern = "blink" |
"code" | "off", period_ms }`、任意の`power_hold = { pin, level }`、`[[boot.action]]`の項目（5.2）です。そして、拡張の能力を照らすテーブル`[[pin]]`、`[[bus]]`、`[[peripheral]]`、`[[device]] { bus, addr, writable = [regs] }`も持ちます。定義の読み手は知らないキーを受け付けないので、これらのテーブルを使う定義は、それらを持つ`definitions`の形式を示します（6）。`dagon gen`はブートローダのビルドのために`boot_wiring.rs`を書きます（`LED`、`HALT`、`PROGRAM`、`RECORD`、拡張のチェーンの型とその保護マップ）。ボードの定義は、ブートローダのビルドが同じように読みます。マイコンのcrateはこれらの定数を受け取り、自分ではピン、パターン、環境変数の既定を持ちません。

`dagon-mcu.toml`は、ボードではなくマイコンが決めるものを持ちます。`[watchdog] trial_ms, confirmed_ms`、`[isolation]`（4.4）、`boot_capabilities`（5.2）、`[boot.record]`（5.5）です。

### 5.2 ブートのアクションプログラム: いちばん単純な拡張の種類

LEDのパターンや電源保持の線だけが要り、コードが要らないボードのために、ボードの定義はDAGONが解釈する宣言的で有界のプログラムを持ちます。チェーンのほかのものと同じ拡張で（ボードの定義の位置）、アクションに使えるイベントを観測し、同じルールで動きます。`WatchdogArmed`以降でだけ、宣言されたピン、バス、デバイスでだけです。

```toml
[boot]
led = "GPIO25"
led_active = "high"
halt = { pattern = "blink", period_ms = 100 }
power_hold = { pin = "GPIO14", level = "high" }

[[boot.action]]
on = "watchdog_armed"              # any boot event marked action-capable in CATALOG (never entry or hal_ready)
ops = [
  { op = "gpio", pin = "GPIO14", level = "high" },   # the power hold line
  { op = "delay", ms = 2 },
  { op = "led", state = "on" },
]

[[boot.action]]
on = "before_jump"
ops = [{ op = "led", state = "off" }]
```

操作の集合（`boot-program`形式1）は次のとおりです。

| 操作 | 欄 | コスト | 補足 |
|---|---|---|---|
| `led` | `state: on \| off \| toggle` | ~0 | `[boot] led`のピン |
| `gpio` | `pin`、`level: high \| low \| input` | ~0 | ピンはボードのピンの表にあり、`boot = true`の印が必要です。設定してそのままにします。宣言された`power_hold`のピンでは、保持のレベルだけを受け付けます |
| `delay` | `ms ≤ 20` | `ms` | イベントごとの合計 ≤ 50 ms |
| `bus_write` | `device`（`[[device]]`の項目）、`reg`（その`writable`の一覧にあるもの）、`bytes ≤ 8` | タイムアウト5 ms | マイコンの定義が`boot_capabilities = ["i2c"]`を宣言するときだけ。任意のアドレスやレジスタは検証が拒みます |

上限（`dagon gen`が検証し、インタプリタが確かめ直します）: イベントごとに`ops ≤
32`、イベントごとに`delay total ≤ 50 ms`（バスの操作はタイムアウトで数えます）。ループ、分岐、読み出し、入力の待ち、フラッシュや状態へのアクセス、クロックやウォッチドッグの制御はありません。知らない操作に出会ったブートローダは、プログラム全体を無効として扱います（`boot.program_skipped`）。

### 5.3 共有の資源

複数の者が必要とするボードの資源は、持ち物ではなく共有です。

- ボードの定義が立ち上げと復旧に必要とし、アプリが自分のデバイスに必要とするバスは、`pub static SYS_I2C:
  Shared<I2c<'static, Blocking>>`（`dagon_events::Shared<T>`。`try_lock`で取る`embassy_sync::mutex::Mutex`）として公開し、`with(|bus| ..)`で使います。これは貸し出しで、クリティカルセクションではありません。貸し出しのあいだ割り込みは許可のままです。長さに上限の無い操作を割り込みをマスクして動かさず（実行モデル）、バスのドライバが自分の割り込みを待つことがあるからです。入れ子や並行の`with`は、待たずにビジーを返します。DAGONの立ち上げと復旧は`with`で取り、アプリはいつでも同じようにします。サンドボックスの拡張は、宣言したデバイスでバスのポートの呼び出しを通して届きます。各デバイスはボードの定義の`[[device]]`で、書けるレジスタと`max_transfer`（デバイスのデータシートから）を持ちます。ポートはほかのレジスタやそれより長い転送を拒みます。ボードの定義はデバイスの定数とレジスタのレベルの関数を公開するので、アプリは何もコピーしません。
- ボードの立ち上げは、`app.bring_up`でのボードの定義の拡張です（`BoardBringUp`のポート）。利用者のステップは、同じイベントでのチェーンの後の拡張か、`Latest.phase ==
  Serving`を待つタスクです。
- ボードの定義が持つのはボードで、その上に積むものではありません。積むペリフェラルのドライバは、それを使うアプリとともにあり、ボードの定義はバスのピンを単なるペリフェラルとして渡します。

### 5.4 VERSIONの問い合わせ

コマンド`Cmd::Version`で、リンクの取り決めの一部です。実装していないノード（復旧用のエージェント、最小のイメージ）は、知らないコマンドのルールで`E_UNSUPPORTED`を返し、ホストはそれを"versions
unknown"と表示します。リクエスト: 要約の集合なら空、ブート記録のページなら1つのセレクタ`[0x21,
offset u16 LE, len u8]`。レスポンス: TLVで、`u8 count`と、その後に`u8 id, u8 len, bytes`。読み手は知らないidを読み飛ばします。デバイスが決められない値は省き、番兵の値は使いません。各TLVの値は最大255バイトで、レスポンス全体がいちばん小さいリンクのペイロードに収まります（10）。

| id | 値 |
|---|---|
| 0x01 | ブートローダのバージョン（`Version`、LE u32） |
| 0x02 | インストールしたブートイメージの識別: そのSHA-256の最初の8バイト |
| 0x03 | エージェント（`dagon-agent`）のバージョン |
| 0x04 | マイコンの定義のバージョン |
| 0x05 | ボードの定義のバージョン |
| 0x06 | マイコンの識別（5.5）: イメージのヘッダとコンパイル済みの構成が持つ値。len 2ではイメージのヘッダの`mcu_id`、u16 LE |
| 0x07 | 隔離（4.5）: `level u8`、`kind u8`、`boot_extensions u8`、`root_protection`（定義が列挙する種類と正確なレベル。例えば`rdp1`/`rdp2`、書き込み保護されたセクタ、レベルが元に戻せるか）、`fuse`（Clearか、モデルの符号化の無効にした拡張の識別を伴うTripped、5.5） |
| 0x08 | 拡張、表示用: リンクされた拡張ごとに`id`（最大32バイト）、`trusted u8`、ソースのハッシュの最初の8バイト（4.7）。TLVを繰り返し、拡張ごとに1つ |
| 0x09 | 拡張、識別用: 信頼された拡張ごとに32バイトのソースのハッシュ全体（4.7）。TLVを繰り返します |
| 0x0A | stage0のバージョン: stage0が領域に持つバージョンの印（`dagon_format::stage0`。`Version`、LE u32）。印の無いstage0とvendor-stage2のマイコンでは省きます |
| 0x0B | ブートローダのスロットA、B: `active u8`（動いているスロット）、次にスロットごとに`state u8`（STATUSのスロットの状態）と`version`（LE u32） |
| 0x0C | アプリのスロットA、B、factory。0x0Bの形です |
| 0x0D | エージェントが話すリンクのバージョン（`PROTO_VERSION`、u8） |
| 0x10 | 形式: `(format id u8, number u8)`の組: wire、image-header、state-log、event、definitions、boot-program、boot-record、extension-manifest |
| 0x20 | ブート記録の要約（5.5）: 記録にどのブートがあるかと、いちばん新しいものの判断と理由。レイアウトはモデルが決めます |
| 0x21 | 生のブート記録の領域の1ページ: `offset u16`と、その後に`len`バイト（`len ≤ RECORD_PAGE`、10）。ホストは領域をページごとに読み、モデルの読み手で解読します |

動いているブートローダは、アプリのコンパイル時の相手ではなく、フラッシュから識別します。

- **ブートローダのスロットとインラインのヘッダを持つself-boot**: アプリはブートローダのログを読んでどのブートローダのスロットが起動したかを知り、次にそのスロットのヘッダ（0x01）を読み、イメージをハッシュします（0x02）。
- **トレーラのヘッダを持つself-boot**: アプリはトレーラ（0x01）を読み、本体をハッシュします（0x02）。
- **vendor-stage2**: インストールされたブートローダにDAGONのヘッダはありません。アプリはベンダーのブートローダのオフセットでフラッシュから読み、ベンダーのイメージのセグメントの表をたどって長さを決め、ハッシュし（0x02）、0x01を省きます。ホストは0x02を、準備のときに記録した識別（`host.provision.recorded`、プロジェクトに保存）と、マイコンの定義のベンダーの表に照らし、バージョンか"unknown vendor stage (hash …)"を表示します。

`dagon status`は、ノードごとに1行で、これらの値、隔離のレベル（"guarded" / "unguarded extensions"）、信頼された拡張、最後のブートの要約を出力します。`dagon doctor`は形式番号を`compat.toml`と比べ、CLIの読まない番号と、レベル`none`のノードにリンクされた信頼されていない拡張について警告します。

### 5.5 ブート記録、マイコンの識別、コンパイル済みの構成: 要件と実行可能なモデル

このモデルの4つの形式は、リセット、電断、部分的な書き込み、バージョンのずれのもとでのバイト単位の動きに正しさが依存する、保持や永続のデータに触れます。保持メモリのブート記録、その隣のロックアップヒューズ、イメージのヘッダ、識別の返答、VERSIONが持つマイコンの識別、そしてコンパイル済みの構成のノードのレイアウトと置き換えです。そのような形式が故障のどんな組み合わせにも耐えることを、文章では示せません。実行可能なモデルなら示せます。そのため、これらの形式はこの文書では定めません。ランダムな故障の注入を持つホスト側のRustのモデルとして設計し、性質を通った形が各形式の書かれた仕様になり、モデルのテストを証明として公開します。モデルはサンドボックスの観測できる取り決め（ブートのどのステップでも超過、フォールト、能力の誤用をする拡張）も、ブートの状態機械の続きに対して試します。

この節は、それらの形式が満たすべきことを示します。モデルを受け入れるのは、下のすべての性質が故障の注入のもとで成り立つときだけです。通った形式はモデルの性質の一覧と各主張の限界とともに、[retained-formats.md](/explanation/retained-formats/)で定めます。

#### ブート記録

- **観測できること。** すべてのブート段階（stage0、ブートローダ）のすべてのブートの遷移を記録し、アプリのエージェントが、起動時に破損していない記録済みのイベントをすべて、カタログのイベントとして、`replayed: true`と、ブート、書き手、位置を区別する識別子を付けてリプレイします。停止した、ハングした、アプリに達しなかったブートは、その後で最初に動くアプリがリプレイします。イベントの欠落（容量）もそれ自体を記録してリプレイします。
- **途中で切れた書き込みへの安全。** どのバイトで中断された書き込みも、以前に有効だったスロットの上書きを含め、失うのは最大で書いているイベントで、読み手が破損していないと受け付ける別のイベントを生みません。欄をその場で変えることはなく、読み手が受け付けるものが2回の書き込みの産物であることはありません。
- **書き手の独立。** 書き手は書き込みの位置を、マイコンの定義の定数と自分が検証したデータからだけ導きます。ほかの書き手の中身を解釈しないので、段階はもう一方の前でも後でも、どの順でも新しいバージョンにでき、不正な保持メモリが書き込みを記録の外に向けることはありません。識別子はどちらの順で段階を新しいバージョンにしても一意で、その前に何も書いていない最初の段階の場合を含みます。
- **順序。** ブートはシーケンスの一周をまたいで古い順にリプレイします。1つの書き手がシーケンスの空間の半分より長く黙っていた場合も含みます。読み手は新しいブートを古いブートより先に報告しません。順序づけられない組は、不確かと印を付けます。両方のイベントが手元にあるあいだ、識別子が繰り返すことはありません。
- **判断の入力にならないこと。**`plan`、`commit`、`self_check`、`confirm`がこの記録を読むことはありません（4.1）。モデルのAPIは、ブート段階に`append`だけを、エージェントに`events`だけを与えます。拡張は届きません（拒否されたリージョン）。
- **バージョン付け。** 保持のレイアウトは、読み手が読めないレイアウトを拒むために要るものを持ちます（6）。古いか新しいレイアウトの保持のバイトを、読み手のレイアウトの有効なイベントとして受け付けることはありません。
- **大きさ。** 領域は最大`RECORD_MAX`バイトで、`RECORD_PAGE(link)`バイトのページで読みます（10）。場所と書き手ごとの大きさは、マイコンの定義の`[boot.record]`で宣言し、1つの経路（マイコンのcrateのビルドスクリプト）ですべてのイメージの`memory.x`に届きます。イメージが一致することをテストが確かめます。
- **リプレイの意味。** 少なくとも1回です。記録には何をリプレイしたかがなく、受信側は識別子で重複を除き、エージェントは1回の実行で同じ識別子を2度リプレイしません。
- **状態ログからの独立。**`app.trial_started`と`app.rolled_back`は、記録に関わらず状態ログから来ます。

#### ロックアップヒューズ

- **所有者。** ヒューズを書くのは、サンドボックスの呼び出しを動かそうとしているレイヤー（ブートローダ、アプリ、リレー）だけで、読むのはすべてのレイヤーの起動です。ほかのコードは読み書きせず、ブートの判断、確定のルール、自己検査の入力にはなりません。
- **識別。** ヒューズはイメージ（スロットとイメージのヘッダのダイジェスト）、拡張（イメージのチェーンの位置と、成果物のハッシュ全体）、レイヤーを示し、どのイメージのどの拡張のほかのビルドとも一致しないほど完全です。モデルは衝突が起きないことを構造として試験し、符号化が拡張やスロットの数に上限を設けることはありません。
- **誤ったトリップが無いこと。** 有効化、呼び出し、戻り、クリアのどの時点のリセットも、戻った呼び出しを示すヒューズを作りません。ロックアップした呼び出しの中のリセットは、常にその呼び出しを示すヒューズを作ります（リセットを越えて残るかはマイコンの定義の宣言のとおりで、電断はヒューズを失うことがありますが、偽造はしません）。
- **引き渡し。** アプリやリレーの呼び出しが有効にしたヒューズは、ブートローダの起動をそのまま通り抜け（ブートローダはそれを記録し、ほかのレイヤーのヒューズをクリアせず、それに基づいて動きません）、アプリの起動が読み、示された拡張をその実行のあいだ無効にします。ブートローダの呼び出しが有効にしたヒューズは、ブートローダの次の起動がそれに基づいて動きます。
- **古いものと新しいバージョン。** 識別がいまインストールされているイメージのどの拡張とも一致しないヒューズ（イメージか拡張が置き換わった）は、古いものとして記録し、それに基づいて動くはずだったレイヤーがクリアします。ほかのレイヤーはクリアしません。成果物のハッシュが変わった拡張は新しい識別で、もう一度動きます。
- **クリア。** DAGONには、ヒューズを消す要求はありません。プロトコルにもCLIにも無いので、識別が存在するヒューズはトリップしたままです。retained-formats.mdの3.6は、形式のモデルのクリアです。
- **バージョン付き。** ヒューズは、読み手が読めないレイアウトを拒むために要るものを持ちます（6）。別のレイアウトのバイトに基づいて動くことはありません。
- **大きさと配置。** マイコンの定義のブート記録の隣で宣言し（`[boot.record] fuse`）、同じリンカの経路ですべてのイメージに届きます（イメージの一致を確かめるテストが1つ）。
- **ウォッチドッグのフラグ。** ヒューズには電源投入以外のすべてのリセットを越えて残り、起動が確認したときだけ消えるウォッチドッグによるリセットのフラグが要ります。マイコンの定義は、マイコンがそれを持つかどうかと出所（ハードウェアのレジスタか、マイコンの定義のコードが用意するもの）を宣言します。無いと、そのマイコンではヒューズを使えず、定義の読み込みがそれを示します。

#### マイコンの識別

- マイコンは手で割り当てた番号ではなく、定義の`id`から導いた値で識別します。マイコンを足すのは定義で、表の編集ではなく、数値のマイコンの表はどこにもありません（テストがソースを調べます）。
- 識別は一意です。定義の読み込みは識別を持つどの欄の幅でも衝突する2つの定義を、両方のidを示して受け付けません。読み込んだどの定義も一致しない識別のデバイスは不明と表示し、別のものに一致させることはありません。
- イメージのヘッダ、識別の返答、VERSIONは、ホストと親機にとってあいまいでないとモデルが示す形でこれを持ちます。場所ごとの幅とハッシュ関数は、モデルの出力です。

#### コンパイル済みの構成

- **状態機械。**`Absent → Written`（最初のコピー）と`Written → Written`（置き換え）で、どちらもターゲットTOPOLOGYのイメージのライフサイクルの`written`を通り、どちらも`app.written`と`relay.children_set`を出します。
- **行き来。** ホストが送ったコピーは、親機で同じ構成（ノードのid、リンク、アドレス、マイコンの識別）として読み返せます。`host.topology.pushed`はそのダイジェストを持ち、親機の`relay.children_set`が繰り返します。
- **上限。** 親機のTOPOLOGYの領域より大きい構成は、1バイトも送る前にホストが拒み（`E_RANGE`には達しません）、それでも送られたら親機が拒みます。
- **不正や未対応。** 親機が読まないレイアウトの番号のコピーや、自身の完全性の検査に失敗するコピーは、無いものとして読み、`relay.children_set { source: Fallback }`で報告し、プロジェクトの予備の一覧を使います。一部だけ適用することはありません。
- **中断された置き換え。** どのバイトで、1度でも繰り返しでも中断された置き換えも、親機が読むのは、ちょうど前のコピーか、ちょうど新しいコピーです。モデルは中断点ごとにどちらかを示し、結果は次の書き込みまでの以後の読み出しで同じです。
- **古いコピーの正確な結果。** 前のコピーを残した中断された置き換えの後、前のコピーはバイト単位で元のままです。完了した置き換えの後は、前のコピーには届きません。

#### 文章で定めるもの

この文書のほかのバイト単位の構造は、ビルド時に生成してデバイスに届く前に検証するもの（ブートのアクションプログラムの表、`boot_wiring.rs`の定数、拡張の保護マップ）、永続を持たないリクエストとレスポンスの符号化（VERSIONのTLVの枠、serveのJSONのメッセージ）、このモデルが変えない既存の取り決め（イメージのヘッダの署名とダイジェストの欄、状態ログ、ベンダーの選択データ、リリースのバンドル）のどれかです。どれも2度書くことも、故障の後で読み返すこともないので、モデルは要りません。モデルのハーネスが、VERSIONのページの読み出しをすべてのリンクの大きさで試すことはあります。

## 6. バージョン付けのルール

番号と部品は[バージョンとリリース](/explanation/versioning/)に従い、`compat.toml`に登録します。このモデルに固有のルールは次のとおりです。

| 取り決め | 番号が動くとき | 読み手のルール |
|---|---|---|
| `event`（`EVENT_FORMAT`） | 種類の名前を変える、消す、またはペイロードの意味を変えるとき。種類やペイロードのフィールドを足しても変わりません | 読み手は知らない種類とフィールドを読み飛ばし、読めない番号を受け付けません |
| `extension-manifest` | マニフェストのキーの意味が変わる、または読み手が拒むキーを足すとき | `dagon gen`は読めない番号のマニフェストを、両方の番号を示して受け付けません |
| `boot-program` | 操作を足す、消す、またはその欄が変わるとき | 知らない操作に出会ったブートローダはプログラム全体を無効として扱い、`boot.program_skipped`を出します。`dagon gen`は、番号がそれを読まないブートローダのためにプログラムを生成しません |
| `boot-record`（とその隣のロックアップヒューズ） | 古い読み手が読み違える形で保持のレイアウトが変わるとき（モデルの互換性のテストが決めます、5.5）。書き手のイベントの符号化は、その`event`の番号とともに、書き手ごとに独立して変わります | 番号を読まない読み手は記録を読めないと報告し、VERSION 0x21で生のページは返します。ある書き手の`event`の形式を読まない読み手は、その書き手のイベントを数え、中身を読み飛ばします |
| `image-header`、`topology-bin` | マイコンの識別の欄やノードのレイアウトが変わるとき（5.5） | ブートローダ、エージェント、親機は、`compat.toml`が挙げる番号を受け付け、ほかを番号で拒みます |
| `action-defs` | アクションのファイルのキーが互換性なく変わるとき | CLIの読まない番号のアクションのファイルは、エージェントの定義と同じく、読み込まなかったと報告します |
| `definitions` | 読み手が拒むキーを足す（読み手は知らないキーを拒みます）、またはキーの意味が変わるとき | 定義は必要な番号を示します。CLIは対応するすべての番号を読み、拒むときは両方の番号を示します |
| `wire` | コマンドのリクエストや返答の意味が変わるとき。コマンドを足しても変わりません（知らないコマンドのルールが`E_UNSUPPORTED`を返します）。デバイスが求められていないフレームを送ることはありません（リンクはリレーを通したリクエストとレスポンスです） | コマンドを実装していないノードは`E_UNSUPPORTED`を返し、ホストはそれを不明と表示します |
| `serve-api` | 追加である`event`のメッセージでは変わりません | 受信側は知らないメッセージの型を読み飛ばします |

最初の公開リリースより前の形式に互換の約束はありません。最初の公開リリースに入る形式が、各取り決めの番号1で、それより前の開発中の形式を読み書きするものはありません。そのリリース以降は、上のルールがあてはまります。

部品の種類: crateの公開APIの互換性のない変更は、その部品の破壊的な変更です。既定を持つ新しいイベント、ポート、操作、TLVのid、定義のキーは機能の追加です。ほかは一般のルールに従います。ポートのトレイトがメソッドを増やすのは既定があるときだけです。ポートのメソッドを消す、または変えるのは、レイヤーの部品の破壊的な変更です。

廃止: このモデルが置き換える公開のトレイトや関数（ストアのアクセサ、起動の引数）は、その部品の1回のマイナーリリースのあいだ、置き換え先を示す`#[deprecated]`を付けて残し、次の破壊的なリリースで消します。`CATALOG`の項目は1つの形式番号の中で消しません。もう出さない種類は、次の`event`の番号まで`retired`の印を付けてカタログに残します。

## 7. DAGONつきアプリの付け外し

ブートローダが求めるのは、このマイコン（その識別で、5.5）、ターゲットのタグ、スロットの署名付きのヘッダを持つスロット、ダイジェストが一致する本体、本体の先頭のベクタテーブル、そしてブートローダが有効にしたウォッチドッグをキックするアプリです（試行では: 試行の周期の中で確定するか、ロールバックされます）。

DAGONをリンクしないアプリも、ROMの経路でインストールでき、署名の後で動きます。確定はしないので、`dagon provision`が確定としてインストールしない限り、`max_attempts`の後でロールバックします。

DAGONを付けるには: マイコンとボードの定義を足し、拡張のチェーン（`Nop`も有効です）で`start`を呼び、返される`Reader`で公開のイベントを読み、ガードを通してキックし続けます。

外すには: `start`を呼ぶのをやめます。ブートローダは引き続き検証して試行を数え、自分の拡張を動かし、ブート記録を書きます。ホストには識別の返答が見えず、`dagon status`がそう示します。

宣言した信頼された拡張（4.7）を除き、どの拡張、イベント、定義、ブートのアクションも、ブートローダが確かめるものを変えません。

## 8. 例

### 8.1 起動画面と書き込みの進み具合のディスプレイ表示（アプリ）

```rust
#[embassy_executor::task]
async fn display_task(mut lcd: Lcd, events: AppEvents) {
    // AppEvents = Reader<dagon_agent::snapshot::AppSnapshot, RECENT>
    let mut rx = events.receiver().unwrap();
    loop {
        let s = rx.changed().await.state;
        match s.phase {
            Phase::Starting | Phase::Idle | Phase::Trial => lcd.splash(s.version, s.trial),
            Phase::Transferring => lcd.bar(s.percent), // Option<u8>
            Phase::Verifying => lcd.text("verify"),
            Phase::Written | Phase::Pending => lcd.text("written"),
            Phase::RebootPending => lcd.text("reboot"),
            Phase::Confirmed => lcd.text("ok"),
            Phase::RolledBack | Phase::Failed => lcd.text("back"),
            _ => {}
        }
    }
}
// main: static EXTS: StaticCell<MyApp> = StaticCell::new();
//       let app = App { exts: Some(EXTS.init(MyApp)), ..App::new(self_check) };
//       let events = node::start::<Board, N>(spawner, dagon, &GUARD, tag, app, UPLINK)?;
//       spawner.spawn(display_task(lcd, events))
```

### 8.2 ブート時の電源保持（ボードの定義、コードなし）

```toml
[boot]
led = "GPIO25"
led_active = "high"
halt = { pattern = "code", period_ms = 250 }
power_hold = { pin = "GPIO14", level = "high" }

[[boot.action]]
on = "watchdog_armed"
ops = [{ op = "gpio", pin = "GPIO14", level = "high" }, { op = "led", state = "on" }]

[[boot.action]]
on = "no_image"
ops = [{ op = "led", state = "off" }]
```

宣言されたI2Cデバイスの後ろにある電源管理で、マイコンの定義が`i2c`のブートの能力を宣言し、ボードがそのデバイスを宣言している場合は、次のように書きます。

```toml
[[device]]
id = "pmic"
bus = "i2c0"
addr = 0x6E
writable = [0x07]

[[boot.action]]
on = "watchdog_armed"
ops = [{ op = "bus_write", device = "pmic", reg = 0x07, bytes = [0x20] }]
```

### 8.3 ブートを含むすべてのイベントのログ（アプリかリレー）

```rust
pub struct DefmtLog;
impl Extension for DefmtLog {
    fn id(&self) -> &'static str { "defmt-log" }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx) { defmt::info!("{}", defmt::Debug2Format(e)); }
}
// … extensions![DefmtLog, MyApp]
// The log shows the generated order of 1.3: app.starting, app.store_opened,
// the replayed boot (boot.entry … boot.before_jump, replayed: true),
// app.boot_replayed, app.trial_started, app.watchdog_taken, app.bring_up, app.link_up …
```

または非同期で、応答のタスクの時間に触れずに: `Reader`から`events.recent()`を取り出すタスク（所有された`EventRecord`、2.4）。

### 8.4 書き込みの転送とプロトコルの置き換え: CANの上のUDS

車載のノードはシリアルリンクの上のDAGONのリンクではなく、ISO-TP（ISO 15765-2）を使うCANの上でUDS（ISO 14229）で通信する診断テスタが書き込みます。1つの拡張が2つのポートを提供し、ブート時とアプリで動きます。

マニフェストは次のとおりです。

```toml
format = 1
id = "uds-can"
layers = ["boot", "app"]
provides = ["UpdateTransport", "UpdateProtocol", "Diagnostics"]
observes = ["app.transfer_*", "app.verify_*", "boot.slot_chosen"]
decides = []
trusted = false
[capabilities]
peripherals = ["can1"]       # declared in the board definition's peripheral table
timer = true
[limits]
max_run_us = 500
stack_bytes = 2048
ram_bytes = 8192
```

ブート段階。ブートローダは拡張を`.ext.*`にリンクし、生成されたマップ（4.4）が、マイコンの定義でウィンドウにできるCAN1のレジスタブロックと、自分のメモリを与えます。フラッシュコントローラ、状態ログ、ウォッチドッグ、DAGONのRAMは拒否します。`WatchdogArmed`から、ブートの状態機械は、ステップのあいだに限られた時間枠（プロジェクトの`[boot] service_window_ms`。試行の周期が上限です）のあいだ`UpdateTransport`のポートをポーリングします。`DiagnosticSessionControl(programmingSession)`を送るテスタはブートローダを復旧の経路に保ち（`boot.recovery { path: Service }`）、拡張の`UpdateProtocol`が対応づけます。`RequestDownload` → `update_offered`、`TransferData` → `header_chunk`/`data_chunk`、`RequestTransferExit` → `transfer_end`、`RoutineControl(switchSlot)` → `switch`、`ECUReset` → `reboot`です。セッションの状態機械は、DAGONのリンクとまったく同じく、トリガーごとにDAGONのすべてのガードと検証器のポートを実行します。そのため、テスタが署名を飛ばしたり、動いているスロットに書いたりすることはできません。拡張の`Diagnostics`のポートは、宣言した識別子について`ReadDataByIdentifier`に答えます（VERSIONが持つのと同じ値からのバージョンと隔離のレベル）。各ハンドラの呼び出しは`max_run_us`の下で動きます。止まったCANコントローラは超過のイベントで、ブートは時間枠なしで判断に進みます。

アプリの段階。同じcrateが、アプリのチェーンの中で、同じポートをエージェントに提供します。応答のタスクが`UpdateTransport`からフレームを読み（ISO-TPの組み立ては拡張の中）、`UpdateProtocol`がUDSのサービスをセッションのトリガーに変え、エージェントの返答が同じ道を戻ります。`accept_update`と`self_check`は、ボードの定義、ライブラリ、アプリのものです（ここでは`decides = []`）。レベル`mpu`では、ハンドラはCANのウィンドウだけをペリフェラルとして非特権で動きます。レベル`none`では同じコードが動き、`dagon status`がそのノードを"extensions unguarded"と表示します。

拡張が、どのレベルでもAPIを通してできないこと: スロットのストアやログに届くこと（ハンドルがありません）、スロットを確定すること（宣言する判断ではありません）、ブートローダが起動するものを変えること。`mpu`/`pms`で、コードが何をしてもできないこと: フラッシュに書くこと、ウォッチドッグに触れること、DAGONのメモリを読むこと、ブートをハングさせること。

### 8.5 デプロイの結果に対するホストのアクション

2.5のような`~/.dagon/actions/on-result.toml`です。その後`dagon apply`が、ノードごとに`host.node.result`を、プログラムが終わった後に`host.action.ran`を出します。同じイベントが`/v1/events`と、`--events`でstdoutに現れます。

### 8.6 起動時の確定（テスト環境用）

```rust
struct Bench;
impl Extension for Bench {
    fn id(&self) -> &'static str { "bench" }
    fn self_check(&mut self, _: &TrialInfo, _: &Ctx) -> Option<Health> { Some(Health::Healthy) }
}
// main: let app = App { exts: Some(EXTS.init(Bench)), confirm: Confirm::Own, ..App::new(always) };
// The start, after the board's bring-up and while the image is on trial, calls
// agent.confirm_own(): app.self_check, app.confirmed (and app.demoted for the other slot).
```

## 9. 採らなかった案

| 案 | 採らなかった理由 |
|---|---|
| DAGONが解釈する閉じた操作の集合だけをブート段階の拡張にする | ニーズは限りがなく（UDSのような診断の転送、追加のセキュリティ処理、ディスプレイ、別のストレージ）、新しいニーズのたびにDAGONの変更になります。コードの要らない用途のためのいちばん単純な拡張の種類として残し、一般のニーズは、ハードウェアが強制するサンドボックスの中の、ポートの後ろのコードにします。 |
| lint、依存の許可リスト、適合のスキャンを防壁にした、ブート時の任意のコード | それらが調べるのはソースで、動くものではありません。コールバックは、ウォッチドッグがチップをリセットするまでループでき、どのレジスタやフラッシュにも書けます。防壁はマイコンの保護ユニットとプリエンプトするタイマ（4.1）で、どちらも無いマイコンでは、定義が`none`と宣言し、製品がそう示します。 |
| 拡張が答える、ブート時の拒否権 | `Boot`をロールバックに変える信頼されていない答えは、故障した拡張が有効なイメージをいつまでも動かさないことを許します。ブートの方針はDAGONの`DecisionPolicy`のポートで、置き換えるのは宣言され、署名され、表示される信頼された拡張（4.7）です。 |
| スロットのストアの変更メソッドを公開し、エージェントをその利用者の1つにする | 永続のイメージの状態が、イベントも確定のルールもなしに変わりえます。イメージのライフサイクルの状態機械が唯一の書き手です。 |
| 新しいタイミングごとにメソッドを足す固定のフックのトレイト | 合成がなく、ディスプレイのためのペイロードのストリームもなく、部分の置き換えもなく、予想しなかったタイミングのたびに一覧を直すことになります。 |
| 手で保守するカタログに照らす、手で保守するイベントの列挙型 | 2つの一覧が一致しても、遷移が何も出さないことがありえます。イベントを出さずには状態を変えられない基本要素だけが、カタログを派生物にします。 |
| リンカのセクションの登録簿（`linkme`） | 項目の順が定まらず、ライブラリによる見えない登録があり、`dyn`のディスパッチのコストと大きさがかかります。 |
| デバイスの`alloc`を使う`dyn Extension`の一覧 | ホストより下にallocはありません。静的なスライスは登録簿の問題を再び持ち込みます。 |
| すべてをチャネルにする | 判断とポートには同期の答えが要ります。キューが正しい道具なのは公開の状態だけです。 |
| 待てるリングに借用のペイロードを残す | 静的なリングは作る側より長く参照を持てません。生成される所有された短い形（2.4）は有界で、表示やログのタスクに十分です。 |
| デバイスからホストへの求められていないイベントのフレーム | リンクは、親機がホップごとに中継するリクエストとレスポンスです。だれも求めていないフレームには、経路も、再試行の連番も、リレーの無通信の時間のモデルの中の場所もありません。`EVENT`のフレームのフラグは予約で、使いません。ホストはデバイスのすべての遷移を、返答、VERSION、ブート記録から導きます。 |
| ビルド時の環境変数でのボードの値 | 定義に無い、形を変えたマイコンの知識です。ボードの定義の`[boot]`に置き換えました。 |
| アプリがコンパイル時に相手にしたブートローダのバージョンを報告する | インストールされているものを識別しません。VERSIONはインストールされたイメージをフラッシュから読みます。 |
| 再起動の遅れの固定の上限 | 1度決めた数は、試行のウォッチドッグの短いマイコンでは誤りです。上限はマイコンの定義の周期から導きます。 |
| CRCで覆ったヘッダをその場で書き換えるブート記録 | その場で書き換える回数、状態、CRCは、リセットに対してアトミックにできません。途中で切れた書き換えは、ヘッダが覆うすべてのイベントを無効にします。ほかの書き手のセクションをヘッダで探す引き渡しは、書き手の形式を結びつけます。 |
| 保持メモリと識別の形式を文章で定める | リセット、部分的な書き込み、バージョンのずれのあらゆる組み合わせに正しさが依存する形式を正しいと示すのは、文章ではなく、ランダムな故障の注入を持つ実行可能なモデルです（5.5）。 |

## 10. 大きさの上限とその出所

どの上限にも唯一の出所があり、それに収まるべき場所をすべて挙げ、テストが各組を確かめます。

| 上限 | 値 | 唯一の出所 | 収まるべきもの |
|---|---|---|---|
| 種類ごとのリンクのペイロード | USB 1088、UART 1024、I2C 256バイト | `dagon_proto::max_payload` | すべてのリクエストと返答、`RECORD_PAGE`、VERSIONのレスポンス、表示の文字列、適合試験の詳細 |
| いちばん小さいリンクのペイロード | `max_payload::MIN` = 上の種類の最小（導いたもので、書いたものではありません） | `dagon-proto` | VERSIONのレスポンス全体（デバイスが答えるすべてのTLVの集合。いちばん大きい0x08の集合、0x20、0x21の全ページで確かめます） |
| TLVの値 | ≤ 255バイト（`len u8`） | VERSIONの符号化（`dagon-proto`のペイロード） | 5.4のすべてのid、0x21のページ |
| VERSION 0x21の固定の費用 | `VERSION_PAGE_OVERHEAD = 6` = ステータス(1) + TLVの数(1) + id (1) + len (1) + offset (2) | 欄の幅の合計と同じことを`const`のアサーションで確かめる`dagon-proto`の定数 | `RECORD_PAGE` |
| リンクごとの`RECORD_PAGE` | `RECORD_PAGE(link) = min(255 − 2, max_payload(link) − VERSION_PAGE_OVERHEAD)`: I2C 250、UART 253、USB 253。`RECORD_PAGE_MIN = 250` | リンクの種類ごとに`const`のアサーションを持つ`dagon-proto`の`const fn` | VERSION 0x21。ホストのページの読み手が求めるのは最大`RECORD_PAGE_MIN` |
| ブート記録の領域 | `bytes ≤ RECORD_MAX = 4096`（0x21の`offset u16`が全体を指せます）。内部の上限はモデルの定数です（5.5） | `dagon_format::bootrecord::RECORD_MAX`、マイコンの定義の`[boot.record]` | 定義の検証、すべてのイメージの`memory.x`、エージェントの読み手 |
| サービスのプリエンプトできない部分 | サービスと種類ごとの`np_max_us`、`COPY_MAX`、各`[[device]]`の`max_transfer`、`dma_stop_max_us` | ポートのカタログと種類の能力の記録（4.4）。実装の測定した最悪の場合に照らして確かめます | サービスの実装、タイマのマスク、4.2の検証5 |
| 拡張の持ち時間 | `max_run_us ≤ BOOT_EXT_MAX_US`、`≤ APP_EXT_MAX_US`。1回のブートで`Σ (calls × budget)` ≤ `trial_ms / 4` | マイコンの定義の`[watchdog]`からの`dagon-events`の`const fn`、`boot_wiring.rs` | `dagon gen`の検証、サンドボックスのタイマ、マニフェスト |
| 拡張のメモリ | `stack_bytes + ram_bytes ≤`マイコンの定義の`[isolation] ext_ram_max`。エントリはコード、データ、スタックの後の種類の上限以下 | マイコンの定義、`boot_wiring.rs`の保護マップ | `dagon gen`の検証、`memory.x` |
| 拡張の数とid | id ≤ 32バイト。0x08は拡張ごとに繰り返し、VERSIONの返答全体がいちばん小さいリンクに収まること | マニフェストの形式、`max_payload::MIN` | `dagon gen`は、VERSIONの返答が収まらないプロジェクトを受け付けません |
| ブートのアクションプログラム | イベントごとに`ops ≤ 32`、`delay total ≤ 50 ms`、`bus_write`ごとに`bytes ≤ 8`、`delay ≤ 20 ms` | `dagon_format::bootprogram`の定数 | `dagon gen`の検証、ブートローダのインタプリタ、`boot_wiring.rs`の表の型 |
| liveのイベントのペイロード | ブートのイベントは ≤ 16バイト（値で）。より大きいアプリやホストのペイロードは参照で | `dagon-events`の大きさのアサーション | `Published`のスナップショット、短い形 |
| `EventRecord` | 24バイト: `layer`、`kind`、`flags`、`dropped`、`seq u32`、`payload [u8; 16]` | `state_machine!`の`compact`属性（生成されたテストが、すべての種類が持つことを確かめます） | `Recent<N>`。ブート記録の短い形は、ペイロードの最初の3バイトを再利用します |
| カタログ | 数値の上限なし。`kind`はレイヤーごとの`u8` | `state_machine!` | 記録の`kind u8`（生成されたテストが、レイヤーの256種類で失敗します）、serveのストリーム |
| 最近のリング | レイヤーのcrateが選ぶ`N` | `Published<S, N>` | レイヤーのcrateの`Published`。あふれると`dropped`を数えます |
| 判断の上限 | 50 ms、`REBOOT_DELAY_MAX = min(trial_ms, confirmed_ms) / 2` | `dagon-events`の定数、マイコンの定義の`[watchdog]` | 応答のタスクのタイマ、`boot_wiring.rs` |

## 11. バスマスタと特権の制御ブロック: 定義のスキーマ

すべてのマイコンの定義は、マイコンのすべてのレジスタブロック（`[[peripheral]]`、4.4）を`window_eligible`、`bus_master`、`privileged_why`とともに挙げます。検査器は、種類が`none`でなく、必要な区分のブロックを欠く定義や、バスマスタをウィンドウにできると印を付けた定義を受け付けません。生成されるマップ（4.4）と適合試験のケース（4.9）はこの一覧から派生し、下のものをほかの場所で手で保守することはありません。ここの3つの表は、配布する3つの定義の一覧で、スキーマが定義の作者に求める判断の例です。ブロックはメモリに書けるか、設計はそれをどう特権に保つか、です。

**STM32F401RE**

| ブロック | メモリに書くか | 特権に保つもの |
|---|---|---|
| DMA1、DMA2（`0x4002_6000`、`0x4002_6400`） | はい（バスマスタ） | `bus_master = true`、`privileged_why = "dma"`。ウィンドウには決してしません。拡張のDMAは、バウンスバッファを使う`DMA_*`のサービスを通すときだけです |
| USB OTG FS（`0x5000_0000`） | この部品ではシステムRAMへのDMAはありません（FIFOベース）が、DAGONのリンクです | `shared`。ウィンドウには決してしません。エージェントが持ちます |
| フラッシュインターフェース（`0x4002_3C00`） | いいえ（フラッシュに書き込みます） | `flash` |
| IWDG、WWDG | いいえ | `watchdog` |
| RCC、PWR、RTC/バックアップ | いいえ | `clock`、`reset`、`power` |
| SYSCFG、EXTI、NVIC/SCB（PPB） | いいえ | `irq-routing`、`pinmux`。PPBはアーキテクチャで特権です |
| CRC | いいえ | `shared`（DAGONが使います） |
| システムメモリ（ROMローダ） | いいえ | `rom`: 非特権モードから実行できません（バックグラウンドのマップ） |
| ボードでDAGONが使わないUSART/SPI/I2C/CAN/TIM/ADC/GPIOのブロック | いいえ | 自己完結しているところは`window_eligible = true`。GPIOはサービスです（DAGONのLEDと共有のバンク） |

**RP2040**

| ブロック | メモリに書くか | 特権に保つもの |
|---|---|---|
| DMA（`0x5000_0000`） | はい | `dma`、バスマスタ。サービスだけ |
| USB（`0x5010_0000`） | 自分のDPRAMにだけで、システムRAMには書きません。DAGONのリンクです | `shared`。ウィンドウには決してしません |
| XIPの制御、SSI（`0x1400_0000`、`0x1800_0000`） | いいえ（SSIを通してフラッシュに書き込みます） | `flash` |
| ブートROM（`0x0000_0000`） | そのフラッシュのルーチンがSSIとDMAを動かします | `rom`: 非特権モードからのROMの実行は全体を拒否します。ルーチンのアドレスは防壁に関係ありません |
| WATCHDOG | いいえ | `watchdog` |
| CLOCKS、RESETS、XOSC、PLL、ROSC、VREG | いいえ | `clock`、`reset`、`power` |
| PSM、SIOのFIFOとスピンロック | コア1の起動とコア間 | `core-control`。2つ目のコアは起動しません |
| SIOのGPIOのout/oe | いいえ | `shared`（DAGONのLED）。GPIOはサービスです |
| IO_BANK0、IO_QSPI、PADS | いいえ | `pinmux` |
| PIO0、PIO1 | いいえ（FIFO。PIOのプログラムはシステムメモリにアクセスできません） | ボードがDAGONのリンクに使わないときは`window_eligible = true`。それ以外は`shared` |
| UART0/1、SPI0/1、I2C0/1、PWM、ADC、TIMER | いいえ | DAGONのものでないところはウィンドウにできます。TIMERは`shared`です |
| コア1 | コア0のMPUに制約されない2つ目のCPU | `[[core]]`。起動しません。その起動のブロックは特権です |

**ESP32-S3**

| ブロック | メモリに書くか | 特権に保つもの |
|---|---|---|
| GDMA | はい。SPI2/3、I2S、UHCI、LCD_CAM、ADC、AES、SHAの代わりに | `dma`、バスマスタ。GDMAのパーミッション制御がチャネルごとにWorld 1を拒否します。サービスだけ |
| USB OTG | はい（自分のDMA） | `shared`、バスマスタ。ウィンドウには決してしません |
| USB Serial/JTAG | DMAなし。DAGONのリンクです | `shared` |
| SPI0/SPI1（キャッシュとフラッシュ）、MMU | いいえ（フラッシュに書き込み、メモリを割り当てます） | `flash`、`protection` |
| AES、SHA、RSA、HMAC、Digital Signatureのエンジン | GDMAを通して（AES/SHA） | `shared`/バスマスタ。`CryptoProvider`は信頼されたポートです（2.1） |
| RTC WDT、TIMG WDT、スーパーウォッチドッグ | いいえ | `watchdog` |
| RTC_CNTL、システムレジスタ、割り込みマトリクス、IO MUX、GPIOマトリクス | いいえ | `power`、`reset`、`clock`、`irq-routing`、`pinmux` |
| PMSとWCLのレジスタ | いいえ | `protection`。World 1の前にロックします |
| Wi-FiとBluetoothのMAC | はい（バスマスタ） | バスマスタ。DAGONは有効にしません。特権です |
| コア1 | 2つ目のCPU | `[[core]]`。マップは両コアに適用します |
| DAGONが使わないUART1/2、DMAなしのSPI2/3、I2C、TWAI、PWM、RMT、PCNT | いいえ | 自己完結しているところはウィンドウにできます。TWAI（CAN）はUDSの転送の自然なウィンドウです |

## 12. 実行モデル

DAGON自身のコードで何がどこで動くか、DAGONのコードが割り込みをマスクしてよい長さ、フラッシュの操作が何を止めるかを、マイコンごとに示します。この節は規範です。マイコンの定義のコードはこれに従って書かれ、マイコンの定義はそのハードウェアの事実（`[execution]`、`[watchdog]`）を持ち、下に挙げる確認がコードとデータをこれに従わせます。

### 12.1 ルール

1. **仕事はハンドラではなくタスクで。** DAGONの応答のループ、リレー、ウォッチドッグへのキックは、エグゼキュータの上のタスクとして動きます。DAGON自身の割り込みハンドラは、タスクを起こすかバイトを動かすだけです（リンクのドライバ、エグゼキュータ自身の割り込み）。待つもの、消去するもの、フラッシュに書くものが、ハンドラの中で動くことはありません。
2. **マスク時間の上限。** DAGON自身のコードのクリティカルセクション（ウォッチドッグのガードの状態、ロックの状態）が割り込みをマスクするのは、最大でマイコンの定義の`[execution] masked_max_us`です。フラッシュの消去や書き込み、ほかのマイクロ秒の上限の無い操作を、その中で動かすことはありません。複数の利用者を持つフラッシュドライバは、操作のためにマスクせずに*取る*ロックで共有します（`dagon_mcu_common::shared::SharedFlash`、`try_lock`。取られていると気づいたハンドルはすぐにエラーを返し、待ちません）。clippyが強制します。`targets/mcu/clippy.toml`は、マイコンの定義とブートイメージ（stage0、ブートローダ）の中のすべてのマスクの基本要素（`critical_section::with`と`acquire`、`cortex_m::interrupt::free`と`disable`、クリティカルセクションの`blocking_mutex::Mutex::lock`）を、解決したパスで拒むので、別名やラッパーでは隠せません。マスクする関数（ウォッチドッグのガードの状態の変更、リレーの共有の表示の状態）は、理由とともに`#[allow(clippy::disallowed_methods)]`を持ち、テストが一覧と例外をレビューされたとおりに保ちます。
3. **フラッシュの操作はハードウェアの事実です。** 消去や書き込みが何を止めるかは、定義の`[execution] flash_op`です。
   - `fetch-stall`: コードが動くシングルバンクのフラッシュ。命令の取得とフラッシュの読み出しは、操作が終わるまで止まります。割り込みは許可のまま待ち、何も動きません（DAGONはRAMにハンドラを置きません）。
   - `xip-off-masked`: 操作はexecute-in-placeを切ってRAMから動きます。ハードウェアの求めるとおり、フラッシュドライバはその1回の操作のあいだ割り込みをマスクします（そしてもう一方のコアを止めます）。
   - `cache-off-masked`: 操作のあいだフラッシュのキャッシュは切れています。フラッシュドライバはその1回の操作のあいだ割り込みをマスクします（内部RAMの外のハンドラは動けません）。

   どの場合も、DAGONは1回の操作で最大1つの消去単位（`[execution]
   erase_unit`）を発行し、かかりうる時間はデータシートの最大値`erase_unit_max_ms`です。
4. **ウォッチドッグは1回の消去より長く持ちます。**`erase_unit_max_ms`は`[watchdog] confirmed_ms`より小さいです（定義の読み込み時に確かめます）。どのリクエストも応答のループの`ServeHost::feed`（RP2040のリレーのPCのループでは、ガードの`progress`）を通した、条件付きのキックで始まります。長い消去は消去単位に分け、あいだに条件付きのキックを挟みます。待機中のイメージの試行は独立した期間です。そのあいだキックするかどうかは、ガードの試行の方針が決めます（`app.watchdog_taken`が報告します）。
5. **止まっているあいだのリンク。** リクエストは新しいリクエストにリンクを閉じて処理します（I2Cのターゲットは自分のアドレスにNACKし、ストリームのリンクの相手は返答を待ちます）。CPUが止まっているあいだに届いたものは、失われるかハードウェアが保持します。要求側は同じ連番で再試行し、ノードは繰り返しのリクエストに、すでに作った返答で答えます。
6. **リセットは返答が出た後でだけ。** 再起動は返答を書いてフラッシュするまで待ち、その後で`allow_reboot_now`が求めた遅れのあいだ、応答のタスクをアイドルにして待ちます（ガードはキックを続けます）。送れなかった返答では何もリセットせず、要求側の再試行は同じ返答を得ます。
7. **事実はどこまで確かめたかを示します。** 定義のハードウェアの事実は、出所に照らして確かめた数値（定義のコメントが文書と表を示します）、`{ unverified = N }`（出所に照らしていない値）、または`"unmeasured"`（値が無い）です。ツールと文書は後の2つを確かめていないと報告します。値の要る確認はunverifiedの値を使います。リリースがどれかに頼ることはありません。

### 12.2 マイコンごと

表はマイコンの定義のコードがすることで、上限はそれぞれのマイコンの定義のものです。

**STM32F401RE**（`flash_op = "fetch-stall"`、`erase_unit`はセクタ1つ）

| 仕事 | 動く場所 | 優先度 |
|---|---|---|
| I2Cのターゲットのリンクの応答（エージェント、CONF_RUN）、フラッシュの消去と書き込み | DAGONの割り込みエグゼキュータ（USART6のベクタ） | P6、threadモードより上 |
| リンクのI2CとDMAの転送 | I2Cドライバの割り込み | HALの既定 |
| タイマ（`embassy-time`） | そのタイマの割り込み | HALの既定 |
| ウォッチドッグへのキック | アプリのキックのタスク、ガードが条件を確かめます | threadモード |
| サンドボックスのタイマ（4.3） | SysTick | どの能力の割り込みよりも上 |

消去のあいだ、セクタが終わるまで何も実行しません。IWDGはリクエストのキックから数えます。自分のI2Cアドレスはリクエスト全体のあいだNACKするので、親のポーリングはCPUなしでペリフェラルがNACKし、親は自分の期限まで再試行します。

**RP2040**（`flash_op = "xip-off-masked"`、`erase_unit`はセクタ1つ）

| 仕事 | 動く場所 | 優先度 |
|---|---|---|
| PCへのリンクとエージェント、子機へのリンク（リレー） | DAGONの割り込みエグゼキュータ（SWI_IRQ_0） | P1 |
| USBデバイス | USBの割り込み | HALの既定 |
| フラッシュの消去と書き込み | PCのタスク、スロットのストアの`SharedFlash`のハンドルを通して | 操作ごとにセクタかページ1つ、ドライバがマスク |
| ウォッチドッグへのキック | キックのタスク、ガードが条件を確かめます | threadモード |

フラッシュの操作はそれぞれ、セクタの消去1回かページの書き込み1回のあいだ割り込みをマスクします。スロットのストアは、セクタのあいだにガードに進み具合を報告します。そのあいだUSBコントローラはホストにNAKし、ホストは再試行します。構成のコピーは書くときと同じフラッシュドライバを通して読みます。

**ESP32-S3**（`flash_op = "cache-off-masked"`、`erase_unit`はブロック1つ）

| 仕事 | 動く場所 | 優先度 |
|---|---|---|
| UARTとUSB Serial/JTAGの上りのリンク、エージェント、フラッシュの消去と書き込み | DAGONの割り込みエグゼキュータ（FROM_CPU_INTR1） | Priority2、threadモードより上 |
| ウォッチドッグへのキック（確定の後、動作の確認を条件に） | リンクのタスク | Priority2 |
| アプリのタスク | threadモード | いちばん低い |

フラッシュの操作はそれぞれ、ブロックの消去1回のあいだ割り込みをマスクします。スロットのストアは、ブロックのあいだにウォッチドッグをキックします。UARTの受信FIFOは収まる分を保持します。止まったことで切れたフレームは20 msの間隔のルールで捨て、要求側が再試行します。そのあいだUSB Serial/JTAGはホストにNAKします。

### 12.3 上限の決め方

マイコンごとの上限は、ボードでの次の値から決めます。DAGON自身のコードのいちばん長いクリティカルセクション（`masked_max_us`: 最悪の値に余裕を足したもの）、ボードのフラッシュの部品の最悪の消去時間（と、定義が示すそのデータシートの値）、1回の消去の終わりに残るウォッチドッグの余裕、その後のリンクの回復（親の再試行に答えること）。
