---
title: "ライフサイクルイベントと拡張"
description: "ブートローダ、エージェント、リレー、ホストに共通の 1 つのモデル: 状態機械、イベント、ポート、保護です。"
---

この文書は、DAGON のライフサイクルイベントと拡張のモデルの設計です。すべてのレイヤーのすべての状態遷移を観測できるイベントにする方法。ボードの定義、ライブラリ、アプリケーションが、ブート段階を含むすべてのレイヤーで、イベントを観測し、少数の判断点に答え、ポートを通して DAGON の一部を置き換えることで DAGON を拡張する方法。マイコンの保護ハードウェアが、拡張を、書き込みを安全にしているものに触れさせない方法。ボードの値とブート時のアクションが定義から来る方法。そしてモデルのバージョンの付け方を示します。実装と、DAGON を拡張するすべての人のための参照です。

目標は、DAGON と利用者のコードを完全に分けることです。利用者（ボードの定義、ライブラリ、アプリケーション）は、DAGON の公開 API、イベント、拡張のポート、定義で、すべての必要を満たします。DAGON の中を編集も、コピーも、フォークもしません。DAGON は、どのサンプル、ボード、ペリフェラルについての知識も持ちません。このモデルは、決まった一覧ではなく、前もって分からない必要（車載の診断の転送、追加のセキュリティ処理、ディスプレイ、別のストレージ）のために書かれています。

言葉: *ブートローダ*は、DAGON がアプリの前に動かすイメージです（self-boot のマイコンでは stage0 とブートローダ。vendor-stage2 のマイコンではベンダーの 2nd stage）。*stage0* は、リセットの後で最初に動く小さなプログラムで、起動するブートローダのスロットを選びます。*エージェント*はアプリの中の `dagon-agent` です。*リレー*は親機の PC へのリンクと子機へのリンクです。*ホスト*は、CLI と `dagon serve` が動かす `dagon-host` です。*拡張*は、DAGON の外にあって、DAGON が拡張点のどこかで動かすコードかデータです。親機/子機は、英語の hub/node にあたります。*キック*は、ウォッチドッグのリセット（カウンタを戻して時間切れを防ぐこと）です。ウォッチドッグが時間切れでチップをリセットすることは、*ウォッチドッグによるリセット*と書きます。

## 1. 状態機械とそのイベント

DAGON の状態の変化は、どれも宣言された状態機械の遷移を通して起き、遷移はどれもイベントを出します。状態機械は `state_machine!` でそれぞれ 1 度だけ宣言します（1.7）。規範となるカタログは生成されたものです。[イベント](/reference/events/)は状態機械の `CATALOG` の定数（種類、ペイロード、属性、観測の経路）から作り、CI でそれらに照らして確かめます。この節のイベントの表は、状態機械が作るものの例です。生成されたカタログに無い項目は持たず、2 つが違うときは生成されたカタログが正しいものです。1.8 は、同じ但し書きのもとで、イベントの区分ごとの観測の仕方を示します。

### 1.1 イメージのライフサイクル（スロットごと、永続）

イメージのスロットの状態は、リセットをまたいでスロットのストアに残ります（kind S: 状態ログ。kind V: otadata のようなベンダーの選択データ）。2 つのレイヤーに遷移を持つ 1 つの状態機械です。エージェントはリンク越しに変え、ブートローダはブート時に変えます。この状態機械の外で永続のイメージの状態を変える公開の経路はありません。

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
| `erase` | app | XFER_BEGIN がすべてのガードと `accept_update` を通った。`Empty`、`Written`、`Pending`、`Reverted` から（動いているスロットや確定したスロットからは不可）。`Pending` からなのは、プロトコルが、待機にしたがまだ起動していないイメージを新しいイメージで置き換えられるからです。消したヘッダはもう SLOT_PENDING の記録と一致しないので、そのスロットは空と読まれます | スロットの本体とヘッダの場所を消去 | `app.transfer_started` |
| `written` | app | XFER_END: 本体のダイジェストがヘッダのものと同じで、ヘッダを書いて検証し直した | ヘッダが所定の場所にある | `app.written { slot, version }` |
| `pending_set` | app | SET_PENDING: ヘッダがあり、署名と互換性、本体のダイジェストが正しい。動いているスロットは不可。`Written`、`Reverted`、`Pending`（SET_PENDING を繰り返すと、もう一度追記します）、`Confirmed`（もう一方のスロットが試行のあいだに、確定したスロットをもう一度待機にして、そちらへ戻します）から | SLOT_PENDING の記録 / ベンダーの選択 | `app.pending_set { slot, version }` |
| `pending_moved` | app | このスロットが `Pending` か `Trial` のときに、もう一方のスロットへの SET_PENDING があり、このスロットが確定したものでない | 同じ SLOT_PENDING の記録（ログは待機のスロットを 1 つ示します） | `app.pending_moved { from, to }` |
| `pending_withdrawn` | app | このスロットが確定したもので、もう一度待機にされ、別のスロットが待機になるか、その試行がロールバックされた。ログの `confirmed` がまだこのスロットを示すので、`Confirmed` に戻ります | 同じ SLOT_PENDING か ROLLBACK の記録 | `app.pending_withdrawn { slot, version }` |
| `attempt` | boot | 待機のスロットを検査した（署名、ダイジェスト）、`attempts < max` | BOOT_ATTEMPT の記録 / ベンダーの "new" → "pending verify" | `boot.attempt_recorded { slot, attempt, max }` |
| `confirmed` | app | 試行中の CONFIRM、`self_check` が健全 | CONFIRMED の記録 / ベンダーの "valid" | `app.confirmed { slot, version }` |
| `demoted` | app | もう一方のスロットの `confirmed`（ログの `confirmed` はもう一方のスロットを示します） | 同じ CONFIRMED の記録 | `app.demoted { slot, version }` |
| `rolled_back` | boot | 試行回数を使い切った、または待機のイメージが検査に失敗した | ROLLBACK の記録 / ベンダーの中止 | `boot.rollback { from, to, reason }` |
| `chosen` | boot | 起動するスロットが決まった（確定か、試行） | なし | `boot.slot_chosen { slot, trial, version }` |
| `no_image` | boot | 起動できるスロットが無い | なし | `boot.no_image { reason }` |

`erase` と `pending_set` の追加の遷移元の状態と `pending_withdrawn` があるのは、プロトコルがそれらのリクエストをすでに許しているからです。状態機械は、リンク上の動作を狭めず、それに従います。

1 つの永続の記録が 2 つのスロットを動かすことがあります（`confirmed` と `demoted`、`pending_set` と `pending_moved` か `pending_withdrawn`）。状態機械はスロットごとに 1 つの状態を持ち、1 つのトリガーが両方の遷移を実行し、両方のイベントをこの順に出します。テストは、ログから畳み込んだストア（`LogState`）の上でエージェントの `ImageStore` を、準備済みのデバイスから、エージェントとブートローダが書くもの（転送、切り替え、確定、ブートの試行、ロールバック）のあらゆる順序で動かし、各ステップの後ですべてのスロットの状態機械の状態が STATUS と同じであることを確かめます。そのため、遷移の無い畳み込みの結果は存在せず、エージェントは 2 つ目のスロットの分も含めて遷移を実行します。

状態機械はスロットのストアのポートを包みます。`ImageStore<S: SlotStore>` がストアを持ち、ポートの変更メソッド（`erase`、`write`、`write_header`、`set_pending`、`confirm`）は、状態機械だけが作る `Transition` を、その変更のイベントを出す遷移の中で受け取ります。`SlotStore` のトレイトは、ボードが実装するポートのままです。エージェントがストアを持ち、だれにも渡しません。ボードのほかの資源（フラッシュの読み手、ウォッチドッグ）は、マイコンの定義のコードが自分で持つハンドルです。それらのハンドルが届くもの（生のフラッシュドライバ、シミュレータの準備用の補助）は、この保証の外です。同じイメージでフラッシュに触れるものと同じく、ボードやテスト環境の信頼されたコードで、拡張からはサンドボックス（4）が守ります。ヘッダの書き込みが読み返せないときは、スロットをもう一度消去します。そのため、故障が状態機械の記録していないイメージを残すことはありません。エージェントは `images() -> &ImageView` を公開します（読み取り専用: ステータス、各スロットのライフサイクルの状態、自分のバージョン、試行中かどうか）。起動時に自分で確定するアプリ（テスト環境用）は、起動にそれを求めます（`App { confirm: Confirm::Own, .. }`）。起動はボードの立ち上げの後で `confirm_own()` を呼びます。これが `confirmed` の遷移です。まず `self_check` の判断、次にポートの確定、イベントは必ず出します。ブートのレイヤーの遷移は、アプリのステージの観測者にも見えるように、アプリが起動時にリプレイします（1.2、5.5）（`app.trial_started`、`app.rolled_back`）。

### 1.2 ブートの手順

self-boot のマイコン（stage0 とブートローダ）: `Entry → HalReady →
WatchdogArmed → LogRead → BeforeDecision → Planned → Committed →
Jumping | Halted`。`plan` は純粋です（フラッシュを読み、判断と追記する記録を計算します）。`commit` がイメージのライフサイクルの遷移 `attempt`、`rolled_back`、`chosen`、`no_image` を実行します。stage0 が持つのは `Entry`、`CoreChosen`、`Halted` だけです。状態ログのブートローダの記録と試行のセルから、ブートローダのスロットを選びます（`dagon_core::bootcore::choose`、retained-formats.md 7）。数えた起動が `trial_attempts` より少ないあいだは待機のコアを、それ以外は確定したコアを選びます。`core_slot_chosen` の前に、コアのヘッダが記録の示すものであることと、本体がヘッダのダイジェストと一致することを確かめ（4.7）、試行のコアの前にウォッチドッグを始め、拡張は動かしません。stage0 はフラッシュに書きません。起動したブートローダは、自分のブートの判断を実行する前にブートローダの試行を決着させます（`settle_log`）。待機のコアは、自分の検査（起動できるアプリがあり、そのアプリのヘッダがコアの鍵で検証できる）の後でだけ CONFIRMED（ターゲット BOOTCORE）を追記し、それ以外はリセットします。別のコアが待機のときに起動した古いコアは、ROLLBACK を追記します。ウォッチドッグのルールは 4.1 にあります。

ブートローダのイメージのライフサイクル（エージェントの `core_image` の状態機械）は、1.1 のアプリのイメージのライフサイクルに、ブートローダの経路が持つ遷移で従います。エージェントの `erase`、`written`、`pending_set`（ターゲット BOOTCORE の XFER と SET_PENDING で、空いているブートローダのスロットへ。動いているコアや確定したコアのスロットは不可）、新しいコア自身の CONFIRMED による `confirmed` と `demoted`、古いコアの ROLLBACK による `rolled_back`（アプリの起動時にリプレイします）です。アプリから見える `Trial` の状態はありません。どのアプリも動く前に、コアが試行を決着させるからです。ターゲットのバイトが `BOOTCORE` の STATUS は、ブートローダのスロット（バージョン、ヘッダのダイジェスト、動いているコアと確定したコア）を報告します。マイコンの定義がブートローダのスロットを宣言しないボードは、`Unsupported` を返します。ブートローダのスロットの数と配置は、マイコンの定義の `[[slot]] image = "bootcore"` の項目から来ます（持つマイコンではどれも 2 つです。スロットが 1 つのマイコンは 1 つを保ち、エージェントはスロットが無いときと同じく BOOTCORE を拒みます）。

ブート段階の拡張（2）とブートのアクションプログラム（5.2）は、`WatchdogArmed` 以降でだけ動きます。それより前のイベントはどれも記録してリプレイしますが、実行中の観測者はいません。そのため、ウォッチドッグがブートを守る前に DAGON の外のものは動きません。ブートのイベントはどれも、フラッシュの書き込みを伴わない保持メモリであるブート記録（5.5）にも追記され、アプリのエージェントが起動時に同じ `boot.*` のイベントとして `replayed: true` を付けてリプレイします。この記録が `plan`、`commit`、`self_check`、`confirm` の入力になることはありません。

| イベント | いつ | ペイロード | ブート時に live |
|---|---|---|---|
| `boot.entry` | リセット後の DAGON のコードの最初の命令、HAL は未初期化 | `stage`、`reset_cause` | いいえ（記録のみ） |
| `boot.hal_ready` | HAL とクロックの初期化の後 | `stage` | いいえ |
| `boot.watchdog_armed` | ウォッチドッグを始めた（4.1）。ここから拡張が動きます | `period_ms` | はい |
| `boot.log_read` | 状態ログを読んだ | `ok`、`pending`、`confirmed`、`attempts` | はい |
| `boot.before_decision` | `plan` を実行する直前。判断の前の最後のイベント | `pending`、`confirmed`、`attempts` | はい |
| `boot.decision_planned` | 判断が決まり、まだ何も書いていない | `Decision` | はい |
| `boot.core_slot_chosen` | stage0 がブートローダのスロットを選んだ | `slot`、`version` | いいえ |
| `boot.slot_chosen` | イメージのライフサイクルの `chosen` | `slot`、`trial`、`version`、`entry` | はい |
| `boot.attempt_recorded`、`boot.rollback`、`boot.no_image` | イメージのライフサイクル（1.1） | | はい |
| `boot.before_jump` | 制御が DAGON を離れる前の最後のイベント | `slot`、`trial` | はい |
| `boot.recovery` | アプリの代わりに復旧の経路に入る | `path` | はい |
| `boot.halt` | コアが完全に止まる | `code` | はい（ブートローダ） |
| `boot.extension_ran` / `boot.extension_faulted` / `boot.extension_overran` | 拡張のハンドラが戻った / フォールトした（保護、バス、命令、不正なサービス） / 時間を超えた | `id`、`us` / `id`、`fault`、`pc?`、`addr?` / `id`、`budget_us` | はい |
| `boot.extension_disabled` / `boot.extension_irq_storm` | 拡張をブートの残りのあいだ無効にした（フォールトの上限か、前のブートのロックアップヒューズ） / 宣言されたペリフェラルの割り込み元を無効にした | `id`、`why` / `id`、`peripheral` | はい |
| `boot.program_step` / `boot.program_skipped` | ブートのアクションを 1 つ実行した / プログラムを実行しなかった | `index`、`op`、`ok` / `why` | はい |
| `boot.step` | 診断のステップに達した | `n` | いいえ |

vendor-stage2 のマイコンには DAGON のブート段階がありません。アプリが起動時に選択データから `boot.slot_chosen`、`boot.rollback`、`boot.recovery` を `replayed:
true` で出し、ブート段階の拡張はそこには存在しません（4.5）。

### 1.3 エージェントのセッション

状態は `Idle`、`Header { slot, body_len, got }`、`Body { target, slot,
len, next }` です。プロトコルのリクエストはどれも遷移です（HELLO、STATUS、VERSION、進み具合は自己遷移）。プロトコル自体もポートです（2.1）。状態機械のトリガーはプロトコルに依存せず（`update_offered`、`header_chunk`、`data_chunk`、`transfer_end`、`switch`、`reboot`、`confirm`、`status`）、プロトコルのプラグイン（DAGON のリンク、UDS など）がメッセージをそこに対応づけます。

生成される起動時の順序: `app.starting` → `app.store_opened`（または `app.store_failed`）→ ブートのリプレイ（`replayed: true` の `boot.*`）→ `app.boot_replayed`（または `app.boot_record_missing`）→ `app.trial_started` / `app.rolled_back` → `app.watchdog_taken` → `app.bring_up` → `app.link_up` …

| イベント | いつ | ペイロード |
|---|---|---|
| `app.starting` | DAGON の起動関数に入った | `tag`（名前のダイジェストとして、2.4）、`role`、`mode` |
| `app.store_opened` / `app.store_failed` | 状態ログや選択データを読んだ / 読めない | `active`、`trial`、`version`、`attempts`、`last_reason` / `error` |
| `app.boot_replayed` / `app.boot_record_missing` | ブート記録の有効なイベントをリプレイした（見つかったブートごとに要約 1 つ、古い順） / 有効なイベントがまったく無い（5.5） | ブートの識別、`writers`、`entries`、`invalid_entries` / `why` |
| `app.trial_started` / `app.rolled_back` | 状態ログからリプレイした | `slot`、`version`、`attempt`、`max` / `from`、`to`、`reason` |
| `app.watchdog_taken` | ブートローダのウォッチドッグを引き継いだ | `period_ms`、`trial_policy` |
| `app.bring_up` | ボードの立ち上げを実行した | `board_id`（名前のダイジェストとして、2.4） |
| `app.extension_started` / `app.extension_faulted` / `app.extension_overran` | 拡張のハンドラが動いた / フォールトした / 超過した（4.2） | `id`、`point` / `id`、`point` / `id`、`point`、`us` |
| `app.link_up` / `app.link_down` | DAGON のリンクがフレームを送受信する（起動時か、切れた後の最初のフレーム） / 止まった（返答を送れない、またはリンクが閉じた） | `link`、`addr` |
| `app.request` / `app.replied` | リクエストが届いた（どのプロトコルでも） / その返答が出た | `protocol`、`cmd`、`seq`、`via` / `status`、`ms` |
| `app.hello` / `app.status_asked` / `app.version_asked` | 識別 / ステータス / VERSION に答えた | `via` |
| `app.update_offered` | 書き込みのリクエストが DAGON のすべてのガードを通った。**判断** `accept_update` | `target`、`slot`、`body_len` |
| `app.update_refused` | 拒んだ | `status`、`by: Dagon \| Extension` |
| `app.header_verified` / `app.header_rejected` | ヘッダの署名と互換性（検証器のポートを通して） | `slot`、`version`、`key_id` / `status` |
| `app.transfer_started`、`app.transfer_progress`、`app.transfer_aborted` | 消去が終わった / 各チャンクを書いた / 無通信のタイムアウト、ストアのエラー、新しいリクエスト | `target`、`slot`、`len` / `done`、`total` / `cause` |
| `app.verify_started`、`app.verify_ok`、`app.verify_failed` | 本体のハッシュと読み返し | `len` / `slot`、`version` / `status` |
| `app.written`、`app.pending_set`、`app.pending_moved`、`app.pending_refused`、`app.confirmed`、`app.demoted`、`app.confirm_refused` | イメージのライフサイクル（1.1） | |
| `app.reboot_requested` | 再起動を受け付けた。**判断** `allow_reboot_now` | `mode` |
| `app.before_reboot` | 返答が出た。リセットが続きます | |
| `app.self_check` | 試行中の確定。**判断** `self_check` | `slot`、`version` |
| `app.rom_entry` | 復旧への入り口を受け付けた | `method` |
| `app.conformance` | 適合試験に応じた | `test` |
| `app.liveness_overdue` | タスクの遅れでガードがキックをやめた | `task` |

### 1.4 リレー

リレーもアプリです（自分のエージェントが `app.*` を出します）。リレー自身の状態機械が持つのは、PC へのリンクの接続/切断、中継したリクエストや表示のリクエストの後の無通信の時間、ポーリングと中継した返答による子機ごとの `Unknown → Present | Absent`、進み具合の画面です。子機へのリンクはポートです（子機のプロトコルのプラグインは、DAGON のフレームだけでなく UDS も中継できます）。

| イベント | いつ | ペイロード |
|---|---|---|
| `relay.pc_up` / `relay.pc_down` / `relay.pc_quiet` | PC へのリンクを開いた / 閉じた / 無通信の時間が終わった | |
| `relay.relay_request` | 中継のリクエストを受けた | `link`、`addr`、`cmd` |
| `relay.child_seen` | 中継した識別やステータスの返答を解析した | `index`、`link`、`addr`、`Seen` |
| `relay.child_found` / `relay.child_lost` / `relay.child_polled` | 不在の後でポーリングに答えた / 最初の無応答 / どのポーリングでも | `index`、`link`、`addr`、`Seen` |
| `relay.child_update_progress` / `relay.child_update_result` | 中継した転送の返答 / 中継した切り替えや確定の返答、または最後の段階の表示のリクエスト | `index`、`done`、`total` / `index`、`outcome` |
| `relay.children_set` | 子機の一覧を読み込んだ（構成のコピーか、プロジェクトの予備） | `Subs` |
| `relay.child_control` | 子機の制御線を動かした / 生のバスアクセス | `line`、`level` / `addr`、`len` |
| `relay.show` / `relay.show_expired` | 表示のリクエストを受けた / 進み具合の画面を消した | `Progress` / |

### 1.5 ホストの操作

操作（`apply`、`rollback`、`recover`、`confirm`、`provision`、`push_topology`、`build`）ごとに 1 つの状態機械があり、ノードごとの副状態を持ちます。ほかにプロジェクトのセッションとエージェントとのつなぎがあります。ホストの転送、署名、ビルドの実行はポートです（2.1）。

| イベント | いつ | ペイロード |
|---|---|---|
| `host.build.started` / `host.build.finished` | ノードとスロットごと | `node`、`slot`、`ok`、`elapsed_ms`、`image_sha256` |
| `host.release.prepared` | バンドルを作った | `version`、`nodes[]`、`path` |
| `host.deploy.planned` | 一覧を表示した。**判断** `confirm_plan` | 一覧 |
| `host.deploy.started` / `host.deploy.finished` | | `release`、`nodes[]` / `ok`、`results[]` |
| `host.node.discovered` | 検出 | `target`、識別 |
| `host.node.step` | ノードの各段階（TRANSFER、VERIFY、REBOOT、CONFIRM、REJECTED、FAILED、ROM） | `node`、`target`、`phase`、`percent`、`version` |
| `host.node.reconnecting` | 再起動後のバージョンを待っている | `node`、`elapsed_ms`、`other_version?` |
| `host.node.result` | | `node`、`Done(v) \| Failed(reason, status?) \| NotStarted` |
| `host.confirm.result` | | `Confirmed \| NotOnTrial \| NeedsChoice` |
| `host.rollback.*`、`host.recover.*` | deploy と同じ形 | |
| `host.provision.started` / `host.provision.recorded` | ROM を通した最初のインストール。インストールしたブートイメージの識別を記録した | `node`、`boot_sha256` |
| `host.topology.pushed` | 表示用のコピーを親機に書いた | `sha256` |
| `host.project.opened` / `host.project.closed` | プロジェクトのセッション | `path` |
| `host.agent.linked` / `host.agent.unlinked` | 同意を記録した / 取り消した | `agent`、`way` |
| `host.version.read` | VERSION に答えた、または Unsupported。隔離のレベルと、ノードが報告する信頼された拡張を含みます | `target`、`VersionResp?` |
| `host.extension.loaded` / `host.extension.rejected` | ホストの拡張（データで定義、または crate）を読み込んだ / 拒んだ | `id`、`kind` / `why` |
| `host.job.*`、`host.device.*` | serve の作業とデバイスのメッセージ | |
| `host.action.ran` | データで定義したアクションが終わった | `id`、`ok`、`elapsed_ms` |

### 1.6 永続の状態とその状態機械

リセットや再起動を越えて残るものはすべて状態機械の後ろにあり、そこへの書き込みはどれも最後の列の意味でアトミックです。どの時点のリセットや電断でも、前の状態か新しい状態のどちらかが残り、読み手が受け付ける半端な状態は残りません。

| 永続の状態 | 場所 | 状態機械 | 書き込む遷移 | アトミックである方法 |
|---|---|---|---|---|
| アプリのイメージのスロット（ヘッダ、本体）とその選択（状態ログ kind S） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `erase`、`written`、`pending_set`、`pending_moved`、`attempt`、`confirmed`、`demoted`、`rolled_back` | 本体が先、ヘッダが最後（スロットにイメージがあるのは、ヘッダを読み返して検証できてからです）。状態ログは追記だけの記録で、それぞれ CRC を持ち、交互に使う 2 つの面にあります。途中で切れた記録は CRC に失敗して無視され、前の記録が有効なままです |
| ベンダーの選択（kind V） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `pending_set`、`attempt`、`confirmed`、`rolled_back` | ベンダーの 2 セクタの方式: 連番と CRC を持つ項目全体を別のセクタに書き、有効で最新の連番が勝ちます |
| ブートローダのスロットとその記録（ブートローダのログ、またはマイコンの定義が両方のログに同じ面を与えているところでは APP の記録と共有する状態ログ） | ノードのフラッシュ（stage0 のある self-boot） | ブートローダのイメージのライフサイクル（`core_image`、1.2） | エージェントの `erase`、`written`、`pending_set`。新しいコアの `confirmed` / `demoted`。古いコアの `rolled_back`。ホストの `provision`（`host.provision.recorded`）。stage0 の `chosen`（`boot.core_slot_chosen`、書き込みなし） | アプリのスロットと状態ログと同じです。stage0 の起動の回数は、フラッシュではなく試行のセルにあります（retained-formats.md 7） |
| 工場出荷の復旧用エージェント（vendor-stage2 のマイコン） | ノードのフラッシュ、factory のパーティション、ヘッダは分離 | イメージのライフサイクル（1.1）、ターゲット AGENT の `erase` と `written` | エージェント。確定したアプリが OTA スロットから動いているあいだだけ | 本体が先、ヘッダが最後。書いているあいだは、動いているアプリ（そのエージェントを含む）が戻り道です |
| インストールしたブートイメージの識別（vendor-stage2 のマイコン） | プロジェクトの準備の記録 | ホストの `provision` | `host.provision.recorded` | 一時ファイルに書き、古いものの上に名前を変えます |
| ブート記録（保持メモリ、5.5） | ノードの RAM か保持のレジスタ | ブート記録: 追記だけで、記録したイベントのほかに状態はありません | `boot.*` の遷移がそれぞれイベントを 1 つ追記します | 必要な性質: どのバイトで中断された書き込みも、失うのは最大でそのイベントで、別のイベントを有効と読めるようにはしません。これを満たす形式は実行可能なモデルが定めます（5.5） |
| ロックアップヒューズ（保持メモリ、5.5） | ノードの RAM か保持のレジスタ | ヒューズ: `Clear → Armed → Clear`（呼び出し）、`Armed → Tripped → Clear`（ロックアップ、その後のクリア） | サンドボックスの呼び出しの前後で有効化とクリア。次の起動でトリップ | 必要な性質: どの時点のリセットも、戻った呼び出しのヒューズを偽造せず、ロックアップした呼び出しのヒューズを失いません。これを満たす形式は実行可能なモデルが定めます（5.5） |
| 構成の表示用のコピー | 親機のフラッシュ | 構成のコピー: `Absent → Written → Written`（置き換え） | ターゲット TOPOLOGY の `written`（`app.written`、`host.topology.pushed`） | 必要な性質: どのバイトで中断された置き換えも、親機が読むのは前のコピーか新しいコピーで、混ざったものではありません。これを満たす `topology-bin` の形式と書き込みの順は実行可能なモデルが定めます（5.5） |
| エージェントの同意 | `~/.dagon/agent.json` | つなぎ: `Unlinked → Linked → Unlinked` | `host.agent.linked` / `host.agent.unlinked` | 一時ファイルと名前の変更 |
| プロジェクトのセッション（開いているプロジェクト、リリースの鍵） | serve のプロセス | プロジェクト: `Closed → Open` | `host.project.opened` / `host.project.closed` | 1 つのロックの下のプロセスのメモリ |
| 診断のステップ、拡張の時間、無通信の時間、見たものの表 | RAM だけ、プロセスの寿命 | 永続ではなく、イベントだけ | | |

このモデルの永続の構造に、その場で書き直すヘッダ、カウンタ、状態の欄を持つものはありません。永続の書き込みはどれも、1 単位でコミットする新しい記録か、名前の変更で置き換えるファイル全体です。このモデルが保持や永続のデータのために導入する形式（ブート記録、ロックアップヒューズ、マイコンの識別を持つコンパイル済みの構成）は、文章では定めません。ランダムな故障の注入を持つ実行可能なモデルとして設計し、それを通った形式が仕様です（5.5）。

### 1.7 遷移の基本要素

イベントの無い遷移は存在できません。状態機械の状態を変える唯一の方法が、イベントを届ける生成された遷移メソッドだからです。crate は、すべての状態機械を 1 つの `state_machine!` で宣言し、crate のルートで 1 度だけ呼び出します。

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

このマクロは状態機械ごとに次のものを生成します。生成された `Machine` 型の非公開の欄の後ろにある状態の列挙型（宣言した crate でだけ作れ、`Default` も `Clone` も無く、変更メソッドは遷移メソッドだけです。各メソッドはレイヤーの合成されたチェーン（2.3）を受け取り、イベントと次の状態を作り、イベントを届け、その後で状態を設定します。遷移元でない状態からは何も変えず、何も届けません。`from` の状態は名前で、任意でフィールドを束ね、値のパターンにはなりません）。遷移ごとに 1 つのペイロードの構造体（`#[non_exhaustive]` で、`Copy` でも `Clone` でもないので、宣言した crate だけが作れます）。各ペイロードの所有する短い形とその解読（2.4）。`CATALOG`（状態機械、状態、そして遷移ごとに from、to、トリガー、種類、コード、バイト数付きのペイロードのフィールド、`observe` の経路、`action` と `retired` のフラグ）。すべての状態からすべての遷移を動かして記録をリプレイする、状態機械ごとのテスト。crate ごとに `MACHINES` の static を定義し、カタログの登録簿（`dagon-catalog`）がそれを挙げます。`dagon-events` に依存する crate が欠けているあいだは、そこのテストが失敗します。どの遷移も届かない状態、`emits` や `observe` の経路の無い遷移、重複したコード、大きすぎるペイロード、crate の中の 2 つ目の `state_machine!`、その外の状態機械は、コンパイルエラーです。[イベント](/reference/events/)はカタログから生成し、テストが 2 つを同じに保ちます。

判断点（3）は `emits` の節にはなりません。新しい判断点は設計の変更で、まずここに、上限、既定、答える側、安全の論拠とともに書き、独立したトレイトのメソッドにします。

イベントを出すのは DAGON だけです。拡張は観測するだけで、作ることはありません。

### 1.8 だれが何を観測するか

どのイベントにも少なくとも 1 つの観測の経路があり、経路は生成されたカタログの属性です。そのため、生成された[イベント](/reference/events/)がこの表をイベントごとに持ちます。経路の無い種類があると、生成されたテストが失敗します。*Live* は同じイメージの拡張（2）です。*replay* は、アプリのエージェントが起動時にブート記録（5.5）か状態ログから出し直すものです。*host* は CLI、serve のストリーム、アクション（2.5）で、返答や VERSION が含むデバイス側のすべてのイベントも見ます。リプレイが届けるのは、カタログが `replay` と印を付け、`retired` でない種類だけです。ほかの記録は推測せず、受け付けません。

| イベント | レイヤー | Live | Replay | Host |
|---|---|---|---|---|
| `boot.entry`、`hal_ready`、`core_slot_chosen`、`step` | boot | いいえ（ウォッチドッグの前、または stage0） | 破損していない記録済みのイベントはすべて → `boot.*` `replayed: true`、少なくとも 1 回、識別子付き | VERSION の記録の TLV |
| `boot.watchdog_armed` とそれ以降のすべてのブートのイベント（`halt` と拡張やプログラムのイベントを含む） | boot | はい: ブート段階の拡張（2）、4 のサンドボックスの中 | 同じ | VERSION |
| `boot.halt`、および `before_jump` の前にハングしたブートや拡張でフォールトしたブートのイベント | boot | 停止やハングの時点まで | イベントは記録に残ります。最初に動くアプリが、残っているすべてのブートを古い順にリプレイします | VERSION（上書きされるまで読めます） |
| `boot.slot_chosen`、`attempt_recorded`、`rollback`、`no_image` | boot | はい | ブート記録、そして記録が無くても状態ログ（`app.trial_started`、`app.rolled_back`） | VERSION、STATUS |
| `app.*` | app | はい | `boot_replayed`、`trial_started`、`rolled_back` はそれ自体がリプレイです | 返答、リレーの `child_*` |
| `relay.*` | relay | はい（親機のアプリのチェーン） | | 中継した返答、表示の結果 |
| `host.*` | host | `on_event` のコールバック（CLI、serve）、ホストの拡張 | | ストリーム、`--events`、アクション |

## 2. 拡張のモデル

すべてのレイヤーで形は 1 つです。DAGON が置き換えのために公開する**ポート**。ポートを提供し、イベントを観測し、判断に答える**プラグイン**。既定では何もしない**イベント**。そして**1 つの合成の順序**（ボードの定義 → ライブラリ → 利用者）。実行の文脈は違いますが（ブートローダと、マイコンが許すところではアプリの非特権のサンドボックス。アプリの embassy のタスク。ホストのスレッドと JSON）、形は同じです。宣言的なアクションプログラム（5.2）は、コードの要らない用途のための、いちばん単純なプラグインの種類です。

### 2.1 ポート

ポートは、DAGON が置き換えられる 1 つの部分のために呼ぶトレイトで、DAGON 自身の実装が既定です。ポートを提供するプラグインは、リンクされたイメージのその部分を置き換えます。ポートはすべてのレイヤーにあります。

ポートのカタログは、コードの中の 1 つの表 `dagon_events::PORTS` で、ポートごとに 1 つの記録（`id`、存在するレイヤーの集合、レイヤーごとの既定、信頼の方針、必要なサービス、説明）です。ここから、この表、生成された[ポート](/reference/ports/)、マニフェストの検証（`provides` は拡張のレイヤーのポートを指定すること）、4.2 のサービスの表、`dagon status` のポートの列が派生します。テストがこの抜粋をカタログと同じに保ちます。ポートで*ない* DAGON の部分も、同じカタログに `port = false` と理由とともに挙げるので、「これは置き換えられるか」に 1 つのソースが答えます。

<!-- generated from dagon_events::PORTS -->
| ポート | レイヤー | 既定 | 信頼の方針 | サービス |
|---|---|---|---|---|
| `UpdateTransport` (リンクに出入りするバイト) | boot, app, relay, host | boot, app, relay: USB CDC、UART、I2C ターゲット、PIO UART; host: シリアル | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` (メッセージ ↔ セッションのトリガー: 識別、ステータス、転送、切り替え、再起動、確定) | boot, app, relay, host | DAGON のリンク | untrusted (ガードは飛ばせません。ガードを実行するのはセッションの状態機械で、プロトコルではありません) | return, time, memory |
| `ImageVerifier` (ヘッダの署名、鍵の検索) | boot, app | プロジェクトの鍵での Ed25519 | trusted only | – |
| `Digest` (本体のハッシュ) | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` (検証器とダイジェストが使う基本要素。ハードウェアの加速、HSM) | boot, app, host | ソフトウェア | trusted only | – |
| `DecisionPolicy` (試行の上限、どのスロットが勝つか) | boot | 定義の `max_attempts`、確定より待機を先に | trusted only | – |
| `WatchdogDriver` (ブートと試行を守るウォッチドッグ) | boot, app | マイコンのもの | trusted only | – |
| `SlotStore` (イメージの置き場所) | boot, app | マイコンの定義のフラッシュの配置 | trusted only | – |
| `LogStorage` (状態ログの置き場所) | boot, app | マイコンの定義のフラッシュの配置 | trusted only | – |
| `Recovery` (追加の復旧の経路: 転送のためのサービスの時間枠、ベンダーのダウンロードモード) | boot, app | マイコンとボードの定義のとおりの BOOTSEL/UF2、factory、ROM の入り口 | untrusted, additive (プラグインは経路を足すだけで、DAGON のものを消せません) | return, transport, time, memory |
| `Diagnostics` (追加の VERSION の TLV、追加のステータス、UDS の DID の表) | boot, app, relay | なし | untrusted | return, time, memory |
| `Display` (段階やブートの結果の見え方) | boot, app, relay | boot: アクションプログラムを通した、ボードの定義の `[boot] led` と `halt`; app, relay: なし | untrusted (ブート時は、宣言されたピンとバスでサンドボックスの中で動きます) | return, pins, buses, time, memory |
| `LogSink` (`Recent` のあふれと拡張のログの行き先) | app, relay, host | なし | untrusted | return, events, time, memory |
| `SelfCheck` (試行中のスロットを確定するときの健全さの答え) | app | 健全 | untrusted (判断点、3) | return, time, memory |
| `BoardBringUp` (アプリの起動時のボードの電源、ピン、バス) | app, relay | ボードの定義のもの | untrusted | return, pins, buses, time, memory |
| `ChildLink` (親機が子機に届く方法) | relay | I2C コントローラ、PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` (イメージとリリースの署名) | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `KeyStore` (署名の鍵の置き場所) | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `BuildRunner` (ノードのイメージのビルド) | host | cargo | untrusted | – |
| `Agent` (CLI とページがリクエストを渡す外部のエージェント) | host | データで定義（エージェントの定義） | untrusted | – |
| `Action` (ホストのイベントの後で実行するプログラムやリクエスト) | host | データで定義（アクションの定義） | untrusted | – |
<!-- end of generated table -->

ポートの既定は DAGON のコードです。信頼された拡張だけのポートの置き換えは特権で動き、4.7 のもとでだけ受け付けます。マニフェストの `provides` は、レイヤーの集合がマニフェストの `layers` のすべてのレイヤーを含むポートを指定します（8.4 の UDS の例は、`boot` と `app` に `UpdateTransport`、`UpdateProtocol`、`Diagnostics` を宣言し、カタログはこれを許します）。ほかのポートの置き換えは、マニフェストが宣言する能力で、レイヤーのサンドボックス（4）の中で動きます。

### 2.2 プラグインとマニフェスト

プラグインは、マニフェスト `dagon-extension.toml` を持つ Rust の crate です（いちばん単純な種類では、定義の中のアクションプログラムです）。

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

コードの形は、すべてのレイヤーで同じです。

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

`Ctx` は、マニフェストが宣言した能力のハンドル（GPIO、バス、タイマ、ペリフェラルのウィンドウ）を持ち、ほかには何も持ちません。ポートの実装は、同じ型への独立した `impl UpdateTransport for MyExt` です。`Ctx` に、フラッシュ、状態ログ、ウォッチドッグ、スロットのストア、DAGON のメモリへのハンドルはありません。サンドボックスのレイヤーでは、コードが何をしてもハードウェアがそれらを拒みます（4）。

拡張のインターフェースが信頼の境界です。拡張が受け取るのは `&Event`、`&Ctx`、判断の引数だけで、どれもチェーン、公開の状態、リプレイにはつながりません。チェーンの合成、そこへの配信、リプレイは、レイヤーの起動の API です。拡張のインターフェースの外で同じイメージに組み込まれたコード（DAGON の crate、アプリケーション自身の起動コード）は、1 つのアドレス空間のものと同じく信頼の基盤に入ります。信頼されていない拡張の隔離は、サンドボックスの役目です（4）。

### 2.3 合成

型付きのチェーンで、順序は型のレベルで、つまりビルド時に固定されます。

```rust
pub struct Chain<A, B> { /* private */ }   // Chain::new(a, b)
impl<A: Extension, B: Extension> Extension for Chain<A, B> { /* on_event: both; decisions: each in turn, first Some wins */ }
pub struct Nop;  // no extension
#[macro_export] macro_rules! extensions { ($a:expr $(, $b:expr)*) => { … nested Chain::new … } }
```

順序は、すべてのレイヤーで常に**公開元 → ボードの定義 → ライブラリ → 利用者**です。レイヤーの起動（ブートローダの生成された `main`、マイコンの定義のコードの `start`、ホストの `Host::new`）が、`compose`（公開の状態と判断点を持つレイヤー: app、relay）か `compose_unpublished`（ブートローダ、ホスト）で自分でチェーンを作ります。`Published` の状態（2.4）のための公開元、次にボードの定義のチェーン、次に利用者の `extensions![..]`（ライブラリが先で、次にアプリ自身）です。結果は中身の見えない `Composed` です。部分に届くことも並べ替えることもできず、拡張ではなく、レイヤーの起動だけが持ちます。利用者のコードが自分を先頭に置くことはできず、`extensions![..]` が公開元を含むことはありません。通知はすべての拡張に届きます。判断は、棄権でない最初の答えで止まります。ポートを提供できるプラグインは、イメージごとに最大 1 つです。チェーンがポートを 2 度提供するプロジェクト（ライブラリと利用者の両方が `UpdateProtocol` を持ち込むなど）は、`dagon gen` が両方の名前を示して受け付けません。

レイヤーごとの登録:

- **ブートローダ（self-boot のマイコン）。** プロジェクトの `dagon.toml` がブートローダの拡張を挙げます（`[boot] extensions = ["uds-transport",
  …]`）。`dagon gen` がチェーンの型を持つ `boot_wiring.rs` を書き、crate を、4.4 の保護マップが隔離する独立したセクション（`.ext.text`、`.ext.data`、`.ext.bss`）でブートローダのイメージにリンクします。チェーンは `const` で作ります（割り当てなし）。ハンドラは、入り口のゲート（4.1）を通してブートの状態機械から同期で呼び、呼び出しごとに `max_run_us` の下に置きます。署名付きのブートローダのイメージが拡張を含むので、拡張を変えて書き込むことは、ほかのブートローダの変更と同じく、ROM の経路でのブートローダの書き込みです。
- **アプリ（embassy）。** `node::start` / `board::start` は `App` を受け取ります。自己検査の関数、`exts: Option<&'static mut (dyn Extension +
  Send)>`（`static` に置いたアプリの `extensions![..]`。embassy のタスクはジェネリックにできず、応答のタスクがチェーンを持つので、ジェネリックではなく参照です）、`confirm`（`ByHost` か `Own`: 起動がアプリの試行を確定します）です。起動は、リンクが応答を始める前に、ボードの `BoardBringUp` のポート（`app.bring_up`。無いボードは何も出しません）を実行し、次に自分の確定をします。ボードの定義の拡張（ボードの定義の `extension()`。ESP32-S3 では `NodePeripherals::
  board_extension`）が、チェーンのボードの場所に入ります。エージェントは起動とその応答のタスクのものにとどまり、渡す公開の関数はありません。マイコンの定義のコードがレイヤーの `Published` を作って持ち、合成し、アプリに `Reader` を返します。エージェントの状態機械がすべてのイベントをチェーンに届け、チェーンが 3 つの点で判断のメソッドに尋ねます。マイコンの隔離のレベルが許すところでは（4.5）、応答のタスクは、ブート時と同じサンドボックスの下で、チェーンのハンドラを非特権で動かします。それ以外では、時間の上限を測って報告しながら、普通のコードとして動かします。
- **リレー。** リレー自身のエージェントは、同じチェーンを持つアプリです。リレーの状態機械のイベントと `ChildLink` のポートはそこを通ります。
- **ホスト。** `dagon-host` は、CLI がプロジェクトの `dagon.toml`（`[host] extensions`）から作ったチェーンを `Box<dyn Extension>` として受け取ります。データで定義する種類（エージェントの定義、アクションの定義、プログラムやデバイスで指定するシリアルや CAN のアダプタのような転送の定義）と、`dagon_host::Transport`、`Signer`、`BuildRunner` を実装する Rust の crate で、crate は CLI のビルドのワークスペースの依存として読み込みます。ホストは利用者の権限で動き、そのサンドボックスは OS です。

アプリのエージェントは、合成されたチェーンだけを受け取ります。`Agent::new(config, store, chain)` で、`chain` は `compose` の結果です。別のフックのトレイトはありません。イベントの観測と、`accept_update`、`allow_reboot_now`、`self_check` への応答は、`Extension` のメソッドです。マイコンの定義のコードの `start` は、アプリの `fn() -> bool` の自己検査も引き続き受け付け、1 メソッドの拡張（`SelfCheck`）に包んで、チェーンの利用者の部分の、アプリの拡張の後に置きます。これは棄権しないので、先に置くと拡張の答えを隠してしまうからです。

### 2.4 待てる公開の状態

非同期の表示やログのタスクが、ハンドラの中で動くことはありません。チェーンの先頭の公開元が、各イベントを次のものにコピーします。

- `Latest`: `Latest { state, last }` を持つ `embassy_sync::watch::Watch`（複数の受信側、最後の値）です。すべてのイベントから畳み込んだレイヤーのスナップショット（アプリの `{ phase, progress, version, trial }`、リレーの子機）と、最後のイベントの記録です。
- `Recent<N>`: 最後の N 個のイベントの固定のリングで、`EventRecord` という、`state_machine!` がイベントごとに生成する所有された有界の形です。`{ layer: u8, kind: u8, flags: u8, dropped: u8, seq: u32, payload:
  [u8; 16] }`。フィールドは、カタログが示す短い形でコピーします（長さ、id、ダイジェストの先頭。ノードのタグやボードの id のような名前は、ドメインの接頭辞と名前の SHA-256 の最初の 8 バイトとして記録し、ホストが知っている名前を通して対応づけます）。作る側が戻った後で参照を残すことはありません。あふれると、いちばん古いものを捨てます。その後の記録が `dropped` で欠落を報告し（それぞれ最大 255 で、残りは次へ繰り越します）、作る側は決して待ちません。`seq` は一周し、連番の演算で比べます。

どちらも 1 つの `Published<S, N>` にあり、レイヤーの crate（マイコンの定義、ホスト）が作って持ち、排他の参照で `compose` に渡します。そのため、チェーンが唯一の書き手です。表示、ログ、アプリのタスクは、読むだけの `Reader`（受信側、最新の状態、最近のリング）を受け取ります。リレーの表示の状態は、リレーのスナップショットを持つ同じ型です。ブートローダに公開元はありません。そのイベントはブート記録と拡張のハンドラに届きます。

### 2.5 ホスト: イベントのストリーム、アクション、crate の発見

`dagon serve` は、すべてのホストのイベントを `/v1/events` に次の形で出します。

```json
{"type":"event","format":1,"layer":"host","kind":"node.result","at":"<RFC 3339 UTC>",
 "job":17,"payload":{"node":"n1","outcome":"done","version":"1.1.0"}}
```

既存の `job.*` と `device.*` のメッセージは変わりません（serve-api 1。`event` のメッセージは追加です）。受信側の取り決め: ストリームや `--events` の出力の受信側は、知らないメッセージの型、イベントの種類、ペイロードのフィールドを読み飛ばし、読めない `format` だけを受け付けません。これにより、`EVENT_FORMAT` を進めずに種類を足せます。CLI は `--events` で同じイベントを出力します（それぞれ JSON 1 行）。

アクションは、エージェントの定義と同じく、ホストのデータで定義するプラグインの種類です。何をいつ実行するかを示す TOML ファイルで、DAGON が実装する転送の種類を使います。command の転送（環境変数の許可リスト、新しい作業フォルダ、プロセスグループ、時間と出力の上限）は、エージェントの定義とアクションが共に使う CLI の 1 つのモジュールです。

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

`kind = "http"` は、イベントの JSON を URL に POST します（https だけ）。アクションはイベントを出した後で動き、操作の中では動きません。その結果は `host.action.ran` です（アクションは `host.action.*` を購読できません）。アクションは判断に答えません。`confirm_plan` に答えるのは TTY、ページ、`--yes` です。command のアクションは、command のエージェントと同じく利用者の権限で動きます。

serve が crate を見つける場所: `dagon.toml [serve] packages` は、プロジェクトからの相対の glob の一覧です（生成されたプロジェクトでは `["nodes/*"]`）。ここに一致し `Cargo.toml` を持つディレクトリだけを挙げてビルドします。DAGON に組み込みの探索パスはありません。

## 3. 判断点

この表に無いものは通知です。ブートローダに判断点はありません。`DecisionPolicy` を提供する信頼されたプラグイン（4.7）を除き、DAGON の外のものがブートローダの起動するものに影響することはありません。そのプラグインは、そのときその部分の DAGON です。

| # | 判断 | レイヤー | 答えられる者（順に、棄権でない最初のもの） | 上限 | 既定 | 決して変えられないこと |
|---|---|---|---|---|---|---|
| 3.1 | `accept_update(&UpdateOffer) -> Accept` | app | ボード、ライブラリ、利用者 | 50 ms。DAGON のすべてのガードを通った後、消去の前 | 受け入れる | DAGON が拒んだものは受け入れられません（そのときは呼ばれません）。拒否はプロトコルの "state" のステータスで答えます |
| 3.2 | `allow_reboot_now(&RebootRequest) -> Delay` | app | ボード、ライブラリ、利用者 | `Now` か `After(ms)`、`ms ≤ REBOOT_DELAY_MAX`。マイコンの定義のウォッチドッグの周期（`[watchdog] trial_ms`、`confirmed_ms`）から作るビルド時の定数で、短いほうの半分です。DAGON はまず返答し、待つあいだガードの方針のもとでキックを続け、その後でリセットします。待ちのあいだのウォッチドッグによるリセットも同じリセットです | すぐ | 再起動は拒めず、遅れには上限があります |
| 3.3 | `self_check(&TrialInfo) -> Health` | app | ボード、ライブラリ、利用者 | 50 ms。`Pending` は "state" のステータスのままで、ホストに再試行させます | 健全 | 試行中でないスロットは確定できず、`confirmed` の遷移を飛ばせません |
| 3.4 | `confirm_plan(&Prepared) -> bool` | host | 対話する利用者（TTY、ページ）か `--yes`。アクションとホストの拡張はできません | 利用者が答えるまで | 尋ねる | 鍵の確認とイメージの検証は、計画を表示する前に行います |

上限は測ります。合成されたチェーンは、レイヤーのクロック（マイコンの定義のコードから来る DAGON のもので、ポートではありません）で、拡張の呼び出しをそれぞれ測ります。遅れた答えは棄権として無視し、チェーンは `app.extension_overran { id, point, us }` を出します（`id` は拡張の id で、記録ではその 64 ビットの FNV-1a のハッシュです）。サンドボックスのレイヤーでは、タイマが上限で呼び出しをプリエンプトします（4.3）。

## 4. 隔離と安全の取り決め

安全は、マイコンが許すところでは、信頼ではなくハードウェアで強制します。信頼されていない拡張はサンドボックスの中で非特権で動き、出口は、DAGON が使うたびに検証する特権のサービス呼び出しだけです。マイコンの保護ユニットは、拡張に自分のコード、データ、スタックと、マニフェストが宣言した能力のウィンドウを与え、ほかには何も与えません。DAGON のコードも、DAGON のメモリも、フラッシュコントローラも、状態とログのストレージも、ウォッチドッグも、どのバスマスタも、どのクロック、リセット、割り込みの経路のブロックも与えません。DAGON は、プリエンプトして呼び出しをアンワインドするタイマで時間の上限を強制します。超過やフォールトがあっても、DAGON はブート（や応答）を続けます。信頼されていない拡張が何をしても、ブートの判断、署名、ロールバック、ウォッチドッグのルール、確定のルールは変わりません。

このモデルは、DAGON が対応していないマイコンのためにも書かれています。マイコンに固有のものはすべて**マイコンの定義の中のデータ**です。保護ハードウェアの種類とそのパラメータ、バスマスタと特権の制御ブロックとそのアドレスの範囲、フォールトとアンワインドの仕組み、ウォッチドッグ、保持メモリです。DAGON は、マイコンごとではなく、保護ハードウェアの*種類*ごとに生成器と検査器を 1 つずつ持ちます（4.4）。既知の種類のマイコンを足すのは定義です。新しい種類は、トレイトの後ろの新しい生成器のモジュール 1 つです。4.5 の 3 つのマイコンの表は定義から生成した例で、規範ではありません。規範は、定義のスキーマと種類のルールです。

### 4.1 サンドボックス

| 要素 | ルール |
|---|---|
| 特権 | 拡張のハンドラは、アーキテクチャの非特権モードで、自分のスタックで動きます（Cortex-M: `CONTROL.nPRIV = 1`、`SPSEL = 1` の Thread モード。RISC-V: U モード。PMS を持つ Xtensa: World 1。TrustZone を持つ ARMv8-M: Non-secure 状態）。DAGON は特権（または Secure）で動きます。非特権のコードは、割り込みをマスクできず、自分の特権を変えられず、特権モードのスタックポインタ、保護ユニット、割り込みコントローラ、システム制御ブロックに書けません（対応するどの種類でも、アーキテクチャで決まっています）。DAGON のコードやデータを実行も読み出しもできません（4.4）。 |
| 入口 | DAGON は入り口のゲートを通してハンドラを呼びます。特権の RAM に `CallFrame` を記録し（呼び出し先が保存するレジスタ、特権のスタックポインタ、続きのアドレス、拡張の id、呼び出しの持ち時間）、拡張の保護マップを読み込み（4.4）、非特権のスタックポインタを拡張のスタックのリージョンの先頭に設定し、サンドボックスのタイマを有効にし（4.3）、引数を引数レジスタに入れて、非特権モードでハンドラの入り口に落ちます（借用のペイロードは DAGON がまず拡張のデータのリージョンにコピーするので、ハンドラが受け取るのは自分のメモリへのポインタだけです）。 |
| 出口 | 出口は、サービス呼び出し `RETURN`（4.2）、サンドボックスのタイマ、フォールトだけです。3 つとも 4.3 を通して `CallFrame` の続きへアンワインドし、拡張のスタックは決して通りません。 |
| メモリ | 保護マップ（4.4）が許すのは、ちょうど次のものです。拡張の `.ext.text`（読み、実行）、`.ext.data`/`.ext.bss`（読み、書き、実行禁止）、スタック（読み、書き、実行禁止、底にガード）、マニフェストが宣言した能力のウィンドウ。ほかはすべて、非特権のアクセスでフォールトします。DAGON のコードとデータ、ブート記録、ROM、すべてのバスマスタ、すべてのクロック、リセット、割り込みの経路、パッドの多重化のブロック、フラッシュコントローラ、状態とログのストレージ、ウォッチドッグ、ほかの拡張のメモリです。 |
| 能力のウィンドウ | ペリフェラルをウィンドウにできるのは、マイコンの定義がウィンドウにできると挙げているとき（`[[peripheral]] window_eligible = true`）だけです。メモリに書けず、割り込みやクロックを経路づけられず、DAGON と何も共有しない、自己完結したレジスタブロックです（4.6、11）。ほかのものにはすべて、同じ宣言のもとで DAGON が拡張の代わりに行うサービス（4.2）を通して届きます。 |
| 時間 | マニフェストにある呼び出しごとの持ち時間で、レイヤーが上限を設けます。サンドボックスのタイマが持ち時間でプリエンプトします（4.3）。ウォッチドッグへのキックは DAGON だけが、自分の予定で行います。 |
| 割り込み | 拡張は割り込みハンドラを持たず、割り込みの許可、禁止、経路づけができません。宣言されたペリフェラルの割り込みは、特権で有界の DAGON のハンドラが処理し、データを拡張ごとの特権のキューに移し、拡張がサービスでポーリングするフラグを立てます（4.2）。サンドボックスのタイマは設定できる最高の優先度を持ち、どの能力の割り込みもそれより低く、宣言した頻度より多く割り込む割り込み元はハンドラが無効にします（`extension_irq_storm`）。 |
| DMA | バスマスタのコントローラ、チャネル、ディスクリプタ、連結のレジスタが、ウィンドウの中に入ることはありません（11）。DMA の要る拡張の入出力は `dma_*` のサービスを通します。DAGON が拡張のバッファを検証し、特権のバウンスバッファを通してコピーし、コントローラを自分でプログラムし、キューで完了を報告します。キャッシュのあるマイコン（4.10）では、バウンスバッファをキャッシュされないメモリに置くか、サービスがクリーンと無効化を行います。 |
| 繰り返しのフォールト | 1 回のブートやアプリの実行で `EXT_FAULT_LIMIT`（3）回フォールトか超過をしたハンドラは、その残りのあいだ無効にします（`extension_disabled`）。ロックアップヒューズ（4.3）は、前のブートでコアをロックアップさせた拡張を無効にします。 |
| 有効化の順序 | サンドボックスがあるのは `WatchdogArmed` からです。ウォッチドッグは HAL の初期化のすぐ後に、ログを読む前、どの拡張やアクションも動く前に、マイコンの定義の `trial_ms` で有効にします。`plan` の後、判断が試行でなければ `confirmed_ms` に設定し、ジャンプの前に 1 度キックします。 |

### 4.2 サービスの ABI

サービス呼び出しは、アーキテクチャのスーパーバイザコール命令で、サービスの id と引数を引数レジスタに入れ、結果を最初のレジスタに返します（Cortex-M: `svc #0`、r0–r3。RISC-V: `ecall`、a0–a3。Xtensa: `syscall`。TrustZone を持つ ARMv8-M: Secure のゲートウェイのベニヤの呼び出しで、id をベニヤの引数にした同じ ABI です）。ハンドラは特権で動き、拡張から DAGON に入る唯一の想定された入り口です。DAGON 自身のコードはこの命令を使わないので、特権モードからのサービス呼び出しはそれ自体がフォールトです。ABI は、フラッシュ、状態ログ、ブート記録、ウォッチドッグ、DAGON のメモリを読み書きするサービスを公開しません。サービスの特権の実装は、DAGON のほかのコードと同じく信頼の基盤の一部で、その不具合は DAGON の不具合です。

検証は、呼び出しのたびに、何かをする前に行います。

1. **呼び出し元。** いまの拡張の id は、DAGON が入口で作った `CallFrame` から取り、呼び出し元からは取りません。ハンドラの外（フレームが無い）のサービス呼び出しはフォールトです。
2. **サービスの id。** レイヤーのサービスの表（ポートのカタログから生成する `const`、2.1）にあること。知らない id はフォールトで、`extension_faulted { fault: BadService }` と記録します。
3. **能力。** 能力に作用するサービスはハンドルの番号を取ります。番号は、その拡張がそのサービスの種類で宣言した能力の数（`boot_wiring.rs` から）より小さいこと。能力自身の制限（デバイスの書けるレジスタ、ピンの許すレベル、ペリフェラルの宣言した割り込みの頻度）は、サービスが確かめます。
4. **ポインタと長さ。** すべてのポインタは呼び出した拡張のデータかスタックのリージョンの中にあり、すべての `(pointer, length)` の範囲は、オーバーフローを確かめる演算でその中にとどまり、アラインメントはサービスに合い、長さはサービスの上限を超えないこと。DAGON は**ポインタを渡した呼び出しのあいだに**特権のバッファを通してコピーし、その呼び出しが戻った後に拡張のポインタを持ち続けることはありません。後の呼び出しがデータを求めるなら、もう一度ポインタを渡し、もう一度検証します。
5. **持ち時間。** サービス呼び出しは呼び出し元の持ち時間の中で動きます。プリエンプトできない部分が残りの時間で終わらないサービスは、始める前に `Busy` で拒みます。

#### 特権の操作と取り消し

サービスは、DAGON が呼び出しごとに `CallFrame` の操作のスロットに持つ明示的な状態（`OpState`: サービス、能力、持っている資源、段階）を持つ**特権の操作**です。サンドボックスのタイマが、クリティカルセクションの途中で特権の操作を打ち切ることはありません。サービスは 2 つの部分で動きます。

- **プリエンプトできない部分**。有界で、サービスごとに宣言します（カタログの `np_max_us`。実装の最悪の場合に照らしてテストが確かめます）。バスのミューテックスを取るか放す、ペリフェラルや DMA のチャネルをプログラムするか止める、割り込み元をマスクするかクリアする、最大 `COPY_MAX` バイトをコピーする、です。タイマの割り込みをマスクするのはこの部分だけで、種類のクリティカルセクションの基本要素（下記）を使います。プリエンプトできない部分が残りの持ち時間を超えるサービスは、始める前に拒むので（検証 5）、持ち時間を超えるのは最大でも `np_max_us` です。
- **プリエンプトできる部分**。転送の終わりを待つところです。サービスは、タイマをマスクしたままスピンしません。待ちは、拡張に `Pending` として返すか（非同期のサービス）、タイマをマスクせず `OpState` に取り消すべきものを記録して行います。

呼び出しが異常に終わると（タイマかフォールト）、4.3 のアンワインドが、DAGON に戻る前に `OpState` について**取り消しの手順**を実行します。この順で、各ステップは有界です。操作が触れたハードウェアがアイドルになるまで、何も解放しません。

1. 操作が始めた DMA のチャネルを止めて無効にし、コントローラがチャネルのアイドルを報告するまで待ちます（種類の記録の `dma_stop_max_us` が上限です。時間内に止まらないコントローラは `extension_faulted { fault: DmaStuck }` と記録し、チャネルは手順 3 がペリフェラルを初期化し直すまでリセットのまま保ちます）。
2. 能力の割り込み元を、割り込みコントローラとペリフェラルでマスクしてクリアします。
3. ポートの既定を通してペリフェラルを静止させます（転送を中止し、FIFO を空にし、ブロックをアイドルに戻します。DMA が止まらないときは、リセットのブロックでペリフェラルとチャネルをリセットします）。
4. 呼び出しのバウンスバッファと特権のキューの項目を回収します（DAGON が持ち、拡張で索引し、拡張のポインタでは索引しません）。
5. 持っていればバスのミューテックスを放します。拡張の転送がもうバスに触れられないからです（バスは手順 3 でポートの既定が残したままなので、DAGON の次の使用は既知の状態から始まります）。
6. `OpState` をクリアします。

同じ手順が、通常の呼び出しの終わりにも、拡張が残したもの（始めて回収しなかった DMA）について動きます。そのため、呼び出しは必ず、拡張の資源が動いていない状態で終わります。呼び出しをまたぐ操作（非同期の入出力）がまたぐのは、したがって**1 回のハンドラの呼び出しの中**だけです。1 回のハンドラの呼び出しで始めた `DMA_RX` は、その呼び出しが戻る前に回収するか取り消します。

プリエンプトできない部分は、種類の能力の記録（`critical_section`、4.4）に記録されたクリティカルセクションの基本要素で、サンドボックスのタイマをマスクします。ARMv6-M では `PRIMASK`（すべての割り込みなので、この部分の上限はレイヤーの割り込みの遅れの上限でもあり、`np_max_us` は小さくなります）。ARMv7-M と ARMv8-M では、タイマとすべての能力の割り込みをマスクし、DAGON のフォールトは許したままの優先度のしきい値に設定した `BASEPRI`。RISC-V では `mstatus.MIE` のクリア（または `mie` のタイマのビット）。Xtensa では、割り込みレベルをタイマより上げます。各サービスの上限 `np_max_us` は、その種類での実装の測定した最悪の場合に照らしてテストで確かめます。

**リセット時の取り決め。** ロックアップ、ウォッチドッグによるリセット、電断は、上の手順を実行せずに呼び出しを終わらせます。そのため、すべてのリセットの後、ほかのものが使う前に、レイヤーの起動が、イメージのどの拡張のどの能力が触れた可能性のあるペリフェラル、DMA のチャネル、割り込み元もすべて初期化し直します。方法は種類の記録の `reset_cleanup` が示し（マイコンにリセットのブロックがあればそれを通す `peripheral-reset`、無ければ各ポートの既定を通す `reinit`）、ブロックの一覧はイメージのマニフェストから来ます。そのため、後始末は保護マップと一緒に生成され、最初のサービス呼び出しより前に、`boot.hal_ready`（ブートローダ）と `app.bring_up` の前（アプリ）で動きます。DAGON 自身が使うペリフェラル（リンク、LED）は、どのリセットの後でも DAGON 自身の立ち上げが初期化し直します。適合試験のビルドは、各サービスのプリエンプトできない部分の中からウォッチドッグによるリセットを起こし、次のブートで同じペリフェラルとチャネルの最初の使用が成功することを確かめます。

#### サービス

完全な一覧はポートのカタログから生成します。種類は次のとおりです。

| 種類 | サービス | プリエンプトできない部分 | `OpState` の状態 | 取り消し |
|---|---|---|---|---|
| Return | `RETURN(result)` | なし | なし | なし |
| Events | `LOG(ptr, len)` | シンクのバッファへ ≤ `COPY_MAX` のコピー | なし | なし |
| Pins | `GPIO_SET(cap, level)`、`GPIO_GET(cap)` | レジスタの書き込みか読み出し 1 回 | なし | なし |
| Buses | `BUS_WRITE(cap, reg, ptr, len)`、`BUS_READ(cap, reg, ptr, len)` | ミューテックスを取り、転送を始める。**この呼び出しの中で**、デバイスの `max_transfer` 以下を拡張のバッファとコピーする | ミューテックスの保持、転送中 | 手順 2、3、5 |
| Transport | `IO_TX(cap, ptr, len)`、`IO_RX(cap, ptr, len) -> n`、`IO_POLL(cap) -> flags` | 拡張のバッファと特権のキューのあいだのコピー。割り込み元の許可か禁止 | キューのほかになし | 手順 2 |
| DMA | `DMA_TX_START(cap, ptr, len) -> op`、`DMA_RX_START(cap, len) -> op`、`DMA_STATUS(op) -> Pending \| Done(n) \| Failed`、`DMA_COLLECT(op, ptr, len) -> n`、`DMA_CANCEL(op)` | `START`: 拡張のバッファをバウンスバッファにコピーし（TX）、チャネルをプログラムする。`COLLECT`: バウンスバッファから ≤ `n` バイトを、この呼び出しに渡され**検証し直した**拡張のバッファにコピーする。`CANCEL`: 手順 1–4 | チャネル、バウンスバッファ、終わったバイト数 | 手順 1–4。回収していない RX は捨てます |
| Time | `NOW_US()`、`BUDGET_LEFT_US()` | なし | なし | なし |
| Memory | `SCRATCH(len) -> ptr` | なし（拡張自身のリージョンのバンプポインタ） | なし | バンプポインタは呼び出しの終わりにリセットします |

したがって、`DMA_RX` が完了時に拡張のメモリに書くことはありません。コントローラは DAGON のバウンスバッファに書き、拡張は `DMA_STATUS`（または能力のキューのフラグ）で完了を知り、`DMA_COLLECT` が、拡張の指定し DAGON がそのとき検証したバッファにコピーします。拡張のポインタが、渡した呼び出しより長く生きることはありません。

#### 取り消しの境界での適合試験

取り消しの列が空でないすべてのサービスについて、マイコンの定義のコードの適合試験のビルドは、各境界（ミューテックスの前、ミューテックスの保持中、転送の開始後、完了の後で回収の前、回収中、回収の後）でタイマが発火するようにして呼び出しを実行し、次のことを確かめます。DAGON の次の同じバスやチャネルの使用が成功すること。呼び出しの終わりの後で拡張のメモリに 1 バイトも届かないこと。バウンスバッファが回収されること（次の呼び出しの再利用に古いデータが見えないこと）。能力の割り込みがマスクされ、次の呼び出しだけが再び許可すること。持ち時間の超過が最大でも `np_max_us` であること。実行可能なモデル（5.5）は、ブートのシナリオで同じ境界を実行します。

### 4.3 プリエンプション、フォールト、アンワインド

サンドボックスのタイマ（`[isolation.capabilities] timer`: Cortex-M では SysTick、RISC-V では `mtimecmp` かプラットフォームのタイマ、Xtensa ではタイマグループのアラーム）は、各呼び出しの前に、その持ち時間で、設定できる最高の優先度（どの能力の割り込みよりも、フォールトでない DAGON のどのハンドラよりも上）で有効にします。サービスのプリエンプトできない部分は、種類のクリティカルセクションの基本要素で最大 `np_max_us` のあいだこれをマスクします（4.2）。タイマのハンドラ、フォールトのハンドラ、`RETURN` のサービスは、どれも同じように呼び出しを終えます。手順はどのアーキテクチャでも同じで、関わるレジスタは種類のアンワインドのモジュール（`[isolation.capabilities] unwind`: `cortex-m-exception`、`riscv-trap`、`xtensa-world`、`armv8m-tz`）から来ます。

1. **スタックされたフレームが使えるか決めます。** Cortex-M の ARMv7-M と ARMv8-M: `MSTKERR`/`STKERR` が立っていれば使えません（入口のスタッキングがフォールトしました）。ARMv6-M: プロセスのスタックポインタが拡張のスタックのリージョンの外か、アラインされていなければ使えません。RISC-V: トラップはスタックしないので（レジスタはそのままで、`mepc`/`mcause`/`mtval` は CSR です）、フレームは動いているレジスタファイルで、常に使えます。Xtensa: World 1 のスタックポインタがリージョンの外なら使えません。手順は、使えないフレームを決して読まず、それ以外でもフレームに依存しません。続きは、拡張が届かない特権の RAM にある `CallFrame` から来ます。
2. **帰属を決めます。** 呼び出しの拡張の id は `CallFrame` にあります。フォールトの種類と、アーキテクチャが報告するところではフォールトしたデータのアドレスを足します（ARMv7-M と ARMv8-M: `CFSR` と、`MMARVALID` が立っているときだけ `MMFAR`、それ以外はアドレスなし。RISC-V: `mcause` と `mtval`。Xtensa: `EXCCAUSE` と `EXCVADDR`）。ARMv6-M にはフォールトのステータスやアドレスのレジスタがありません（すべてのフォールトが HardFault です）。そのため、記録はフレームが使えるときはスタックされた `PC` とその属するリージョンを、それ以外は `PC: unknown` を持ちます。`CallFrame` が開いているときは、フォールトは常に拡張に帰属させます。
3. **取り消します。** `OpState` について 4.2 の取り消しの手順を実行します（DMA のアイドル、割り込みのマスク、ペリフェラルの静止、バッファの回収、その後でミューテックスの解放）。
4. **アンワインドします。** ハンドラ自身の（特権の）スタックの上で行います。`CallFrame` から呼び出し先が保存するレジスタを戻し、特権のスタックポインタを保存したものに設定し、DAGON の特権の保護マップ（バックグラウンドのマップで、拡張のリージョンはすべて無効）を読み込み、戻る文脈の特権モードを戻し、打ち切った呼び出しの遅延保存された浮動小数点の文脈を捨てます（Cortex-M4F/M7: `FPCCR.LSPACT` をクリアし、基本フレームで戻ります。ブートローダは FPU を無効にして動くので、拡張の FP 命令は `NOCP` のフォールトです。拡張は、どのレイヤーと種類でもソフトフロートでコンパイルします）。
5. **戻ります。** 例外から DAGON の続きへ戻ります（Cortex-M: メインスタック上の合成した基本の例外フレームと、特権の Thread モードの `EXC_RETURN`。RISC-V: `mepc` を続きに、`mstatus.MPP` を M モードに設定して `mret`。Xtensa: 続きで World 0 へ戻ります。ARMv8-M TrustZone: Secure のスタックを通して Secure の続きへ）。続きは、呼び出しの結果（`Returned(result) | Overran |
   Faulted(kind) | Disabled`）を `CallFrame` から読んで先に進む DAGON のコードです。戻らなかったポートの呼び出しはポートのエラー値を返し、戻らなかった通知はそこで終わりです。
6. **記録します。** 手順 2 の帰属とともに、`extension_ran | extension_overran | extension_faulted`。

タイマとフォールトが同時のとき: どちらのハンドラも、`CallFrame` の状態で守った 1 つのアンワインドの処理を通ります。後から動いたものはフレームが閉じているのを見て、アンワインドせずに戻ります。

#### ロックアップとヒューズ

拡張がスタックポインタをリージョンの外に動かしたとき（スタックの演算は非特権です。システム命令でスタックポインタのレジスタに書くのは、対応するどの種類でも特権だけです）の、タイマやフォールトの例外の入口でのフォールト: ARMv7-M/ARMv8-M では、プロセッサはスタッキングのエラーを示してハンドラに入り、上の手順があてはまります。RISC-V ではハードウェアは何もスタックしないので、手順があてはまります。ARMv6-M では、昇格した HardFault が同じ不正なポインタにスタックし、コアはロックアップします。Xtensa では、不正なスタックでの World 切り替えの例外は特権のスタックで処理します。ロックアップはウォッチドッグが終わらせ（ブート時はどの拡張も動く前に有効化、4.1。アプリではアプリのガード）、4.2 のリセット時の取り決めが、呼び出しが触れたものを初期化し直します。

ロックアップがリセットのたびに繰り返さないように、DAGON は保持メモリに**ロックアップヒューズ**を持ちます。前の実行が死んだときにサンドボックスの呼び出しがどのイメージのどの拡張を動かしていたかを示す記録で、すべてのレイヤーの起動が読み、その拡張を無効にして動くために使います。ヒューズは保持メモリの状態で、その正しさは、あらゆる時点のリセット、識別の衝突、イメージと拡張の新しいバージョンの書き込みのもとでのバイト単位の動きに依存します。そのため、ブート記録と同じく、文章では定めず、要件（5.5）と、それを通った形を形式とする実行可能なモデルで定めます。設計がここで決めることは次のとおりです。

- すべてのレイヤー（ブートローダ、アプリ、リレー）のすべてのサンドボックスの呼び出しを覆います。
- 確定したアプリの拡張がコアをロックアップさせたら、その拡張を無効にして戻り、応答を続けます。`commit` の後のブートローダのロックアップが失わせるのはそのブートの試行だけで、ヒューズによって、それは 3 回ではなく 1 回の試行になります。
- ヒューズを読み書きするのはレイヤーの起動だけです。ブート記録ではなく、`plan`、`commit`、`self_check`、`confirm` の入力でもありません。
- トリップしたヒューズは、VERSION（0x07）と `dagon status`（"extension <id> disabled after a lockup"）で報告し、それが示す識別が動いているイメージにもう無くなったときにクリアします。クリアする要求はありません。プロトコルにも CLI にも無いためです（5.5）。

DAGON 自身のフォールトハンドラの中のフォールトは、診断コード付きの DAGON の停止です。

### 4.4 保護の種類、定義のスキーマ、生成されるマップ

マイコンの定義は、保護ハードウェアをデータとして宣言します。

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

書き方: 定義の値は `a-z 0-9 -` のケバブケースの語です（そのため `id-cache` です）。プロジェクトの定義の、単純な名前でない文字列は、PC の外に出る前に置き換えるからです（[マイコンとボードの定義](/reference/definitions/)）。

能力の記録は 2 度確かめます。`dagon gen` は定義の数値に基づいて計画し、種類の許す集合（下記）の外の数値を持つ定義を受け付けません。ブート時と適合試験のビルドでは、マイコンの定義のコードがユニット自身のレジスタ（Cortex-M の `MPU_TYPE`、SAU のタイプレジスタ、RISC-V の書ける `pmpcfg` のエントリの数と読み返せる最小の NAPOT の大きさ、PMS のバージョンのレジスタ）を読み、ハードウェアが記録を裏付けないレベルを有効にしません（`boot.isolation_mismatch`、レベルは `none` と報告します）。

検査器は、`kind` が `none` でないすべての定義に次のものを求めます。`flash`、`watchdog`、`clock`、`reset`、`irq-routing` のそれぞれについて `privileged_why` を持つ `[[peripheral]]` が少なくとも 1 つ。`bus_master = true` の項目はすべて `window_eligible = false`。`[[core]]` が複数あるマイコンでは `core-control` のブロック。保護の設定のレジスタブロックを持つ種類（`esp-pms`、`riscv-epmp`）では `protection` のブロック。どれかを欠く定義は、欠けた区分の名前を示して受け付けません。DMA コントローラの場所を定義が示さないマイコンに、`none` より上のレベルは与えられません。

マップの生成器と検査器は `kind` についてジェネリックです。種類ごとに 1 つのモジュールが 1 つのトレイトの後ろにあり、`dagon gen` とテストが使います。

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

`plan` は、`.ext.text`、`.ext.data`/`.ext.bss`、スタックを、種類のアラインメントのルールが許すアドレスに置き（リンカスクリプトは同じ結果から生成します）、宣言されたウィンドウを割り当て、エントリが収まらないときははっきりしたメッセージで拒みます（`extension
uds-can needs 5 windows; 3 fit after code, data and stack on a
8-region armv6m-mpu`）。`check` は、マイコンの定義のコードのテストで、生成された表と定義に照らして動きます。適合試験のビルドは、デバイス上の拡張から各特権のブロックへの書き込みを実行し、フォールトのイベントを期待します。既知の種類のマイコンを足すときにコードには触れません。種類を足すときはモジュールを 1 つ足します。

種類ごとに、ハードウェアが与えるものと生成器の扱いは次のとおりです。

| 種類 | ユニット | 能力の記録と形 | フォールトの報告 | 保証すること | できないこと |
|---|---|---|---|---|---|
| `armv6m-mpu` | ARMv6-M PMSAv6 MPU | `mpu_regions`（実装の選択で、`MPU_TYPE` から読み返します。RP2040 では 8）、2 のべき乗の大きさ、大きさに揃えたベース、実装の `min_region`（RP2040 では 256 B）、256 B からの 8 サブリージョン、`PRIVDEFENA`。`critical_section = primask`（このアーキテクチャに `BASEPRI` はないので、プリエンプトできない部分はすべての割り込みをマスクし、`np_max_us` がレイヤーの割り込みの遅れの上限です） | すべてのフォールトが HardFault。ステータスやアドレスのレジスタはありません。帰属は `CallFrame` とスタックされた PC（4.3） | 有効なリージョンの外のすべてへの非特権のアクセスの拒否。リージョンごとの XN | 例外のスタッキング中のフォールトでコアがロックアップします（ロックアップヒューズ、4.3）。ウィンドウは `min_region` 以上の揃ったブロックでなければなりません |
| `armv7m-mpu` | ARMv7-M PMSAv7 MPU | 実装の `mpu_regions` ∈ {8, 16}（Cortex-M3/M4: 8。Cortex-M7: 8 か 16、読み返します）、2 のべき乗、大きさに揃えたベース、最小 32 B、256 B からのサブリージョン、`PRIVDEFENA`。`critical_section = basepri` | MemManage と `CFSR`、`MMARVALID` のとき `MMFAR`。スタッキングのエラーは示され、ロックアップしません | 上と同じで、データのフォールトのアドレスが付きます | ウィンドウは 2 のべき乗のブロックです。キャッシュのあるコアでは 4.10 のルールがあてはまります |
| `armv8m-mpu` | ARMv8-M PMSAv8 MPU | 実装の `mpu_regions` ∈ {0, 4, 8, 12, 16}（読み返します）、32 B の粒度のベースとリミットの組（2 のべき乗のルールもサブリージョンもありません）、`PRIVDEFENA` | MemManage と、有効なとき `MMFAR` | `armv7m-mpu` と同じで、ウィンドウは 32 B に揃った任意の範囲です | リージョン 0 は `none` です |
| `armv8m-tz` | Security Extension を持つ ARMv8-M | 独立した 3 つの記録: `sau_regions` ∈ {0, 4, 8}、`s_mpu_regions` と `ns_mpu_regions` はそれぞれ ∈ {0, 4, 8, 12, 16}、すべて読み返します。そしてマイコンの固定の帰属の `idau` の表と、定義が挙げる `mpc`/`ppc` のユニット。DAGON は Secure、拡張は NS-MPU の下で Non-secure かつ非特権です。サービスの ABI は Secure のゲートウェイです（NSC のリージョンの `SG` のベニヤ） | Secure と Non-secure のフォールト、`SecureFault` | もっとも強いものです。Secure のメモリは、Non-secure 状態で特権であっても拡張には見えません。バスマスタはインターコネクトが Secure/Non-secure に帰属させ、定義が各マスタの帰属を挙げます | IDAU/MPC/PPC の設定はマイコン固有のデータです。インターコネクトが帰属させられないマスタは Secure だけにとどめます。帰属は IDAU だけでも（完全な `idau` の表を持つ `sau_regions = 0` は有効な `tz` の定義です）、IDAU と SAU でもかまいません。レベルが `none` になるのは、どちらも拡張のリージョンを帰属させられないときだけです |
| `riscv-pmp` | RISC-V PMP | 実装の `pmp_entries` ∈ {0, 16, 64}（どの `pmpcfg` のエントリに書けるかを調べて読み返します）、アドレスモード NA4（4 B、`na4` のとき）、NAPOT（8 B から、実装の `grain` を読み返します）、TOR（粒度の上の任意の範囲、`tor` のとき）。拡張は U モード、DAGON は M モード。`critical_section = mstatus-mie` | `mcause`、`mtval`（アドレス）、ハードウェアのスタッキングなし | U モードに対する、与えていないすべての拒否。エントリごとの XN | ePMP が無いと、M モードは PMP で制限されません。エントリは少なく、TOR は境界ごとに項目を消費します。エントリ 0 は `none` です |
| `riscv-epmp` | RISC-V ePMP（Smepmp） | PMP と同じで、`mseccfg` のマシンモードのロックダウンと共有リージョンが加わります | PMP と同じ | PMP と同じで、DAGON は M モードが拡張のメモリから実行することを禁止できます | |
| `esp-pms` | Espressif の Permission Control + World Controller | 実装の `worlds`、`memory_regions`、`peripheral_slots`、`gdma_channels`（PMS のバージョンとリージョンのレジスタから読み返します）。`critical_section = xtensa-level`。内部メモリのリージョン、MMU を通した外部メモリ、各ペリフェラル、各 GDMA チャネルについての World ごと（World 0 は DAGON、World 1 は拡張）の権限。コアごとに適用し、ロックできます | `EXCCAUSE`/`EXCVADDR`、World 切り替えの例外 | World 1 に対する、与えていないすべての拒否。すべてのコアで、GDMA を含みます | PMS/WCL のレジスタは DAGON のマイコンの定義のコードがプログラムする必要があります（HAL のドライバはありません）。それまでは `none` です |
| `none` | 使えるユニットが無い | | | 時間の測定、イベントの記録 | メモリ、フラッシュ、ウォッチドッグ、ハング（4.5） |

定義が主張できるレベルは種類から決まります。MPU と PMP の種類は `mpu`、`esp-pms` は `pms`、`armv8m-tz` は `tz`、それ以外、またはマイコンの定義のコードが種類を実装するまでは `none` です。

### 4.5 マイコンごとの隔離のレベルと 3 つの例

`dagon-mcu.toml` は、そのマイコンで DAGON が強制するレベルを宣言します。VERSION がそれを報告するので（5.4）、ホストはノードの隣に表示します。

| レベル | 保証 | 保証しないこと（文書、VERSION、UI で示します） |
|---|---|---|
| `mpu` | 4.1–4.4 のすべて: 非特権の実行、DAGON のコードとデータの拒否、サービス呼び出しの検証、能力のウィンドウ、特権の DMA と割り込み、タイマのプリエンプションとアンワインド、フォールトからの回復、ロックアップヒューズ | 宣言された能力の物理的な効果（4.6） |
| `pms` | パーミッションコントローラと World の切り替えで同じことを、すべてのコアで、チャネルごとの GDMA の拒否とともに | 同じ |
| `tz` | Security Extension で同じことを。さらに、DAGON のメモリはどの Non-secure のソフトウェアにも見えません | 同じ |
| `none` | 時間の上限を測って報告しますが（`extension_overran`）、プリエンプトしません。能力のハンドルは宣言どおりに渡します。イベントは記録します。ハードウェアの拒否はありません。拡張は、不具合や意図によって、フラッシュに書く、ウォッチドッグや DAGON のメモリに触れる、ウォッチドッグがチップをリセットするまでハングすることができます（ブート時は、`commit` の後に起きたときだけ、試行を失います） | メモリとフラッシュの拒否、時間の上限、誤動作する拡張に対する判断、署名、ロールバック、ウォッチドッグのルール。`dagon status` とページは、そのノードに "extensions unguarded" と表示し、`dagon doctor` は、プロジェクトがそこに信頼されていない拡張をリンクすると警告します |

DAGON が定義を配布している 3 つのマイコンを、スキーマの例として挙げます（規範となるマップは、`targets/mcu/*/dagon-mcu.toml` の定義から `dagon gen` が生成するものです。下の事実はベンダーのリファレンスマニュアルから取り、立ち上げ時にユニットのタイプレジスタから読み返します）。

**STM32F401RE** — `kind = "armv7m-mpu"`、`mpu_regions = 8`（`MPU_TYPE` から読み返します）、`min_region = 32`、`subregions_from = 256`、`fault_address = true`、`unwind =
"cortex-m-exception"`、ブート時とアプリで `level = "mpu"`（ST PM0214 4.2 節 MPU、ブロックのアドレスは ST RM0368 2 節のメモリマップ）。ウィンドウ 2 つの拡張 1 つについて生成されるマップ:

| リージョン | ベース | 大きさ | 属性 | 目的 |
|---|---|---|---|---|
| 0 | イメージの本体の中の `.ext.text`、大きさに揃える | 2 KiB … 32 KiB | 非特権 RO、X | 拡張のコードと定数 |
| 1 | RAM の `.ext.data` + `.ext.bss`、揃える | 1 KiB … 16 KiB | 非特権 RW、XN | 拡張のデータ |
| 2 | 拡張のスタック、揃える | 1 KiB … 8 KiB | 非特権 RW、XN。いちばん下のサブリージョンは無効 | ガード付きのスタック |
| 3–4 | 能力のウィンドウ: それぞれ 1 KiB の APB/AHB のブロック 1 つ | 1 KiB | 非特権 RW、XN | 宣言されたペリフェラル（ウィンドウにできるもの） |
| 5–6 | ウィンドウを増やすために空き | | | |
| 7 | `ram_bytes` が 2 のべき乗でないときの 2 つ目のデータのリージョンのために予約 | | | |

特権（`window_eligible = false`）: フラッシュインターフェース `0x4002_3C00`（`flash`）、IWDG `0x4000_3000` と WWDG（`watchdog`）、RTC とバックアップ `0x4000_2800`（`power`）、RCC `0x4002_3800`（`clock`、`reset`）、PWR、SYSCFG と EXTI `0x4001_3800`/`0x4001_3C00`（`irq-routing`、`pinmux`）、DMA1 `0x4002_6000` と DMA2 `0x4002_6400`（`dma`、バスマスタ）、USB OTG FS `0x5000_0000`（DAGON のリンクと共有）、CRC、システムメモリ `0x1FFF_0000`（`rom`）、オプションバイト、PPB。

**RP2040** — `kind = "armv6m-mpu"`、`mpu_regions = 8`（`MPU_TYPE` から読み返します）、`min_region = 256`、`subregions_from = 256`、`fault_address = false`、`unwind =
"cortex-m-exception"`、2 つの `[[core]]`、HardFault の帰属とロックアップヒューズを伴い、ブート時とアプリで `level = "mpu"`（RP2040 データシート 2.4 節の Cortex-M0+ の機能と MPU のタイプレジスタ、Arm ARMv6-M Architecture Reference Manual の例外モデル）。生成されるマップ:

| リージョン | ベース | 大きさ | 属性 | 目的 |
|---|---|---|---|---|
| 0 | イメージのスロットの本体の中の、XIP の領域の `.ext.text`、大きさに揃える | 1 KiB … 32 KiB | 非特権 RO、X | 拡張のコードと定数 |
| 1 | SRAM の `.ext.data` + `.ext.bss`、揃える | 256 B … 16 KiB | 非特権 RW、XN | 拡張のデータ |
| 2 | 拡張のスタック、揃える | 1 KiB … 8 KiB | 非特権 RW、XN。いちばん下のサブリージョンは無効 | ガード付きのスタック |
| 3–5 | 能力のウィンドウ: それぞれ 16 KiB の APB のブロック 1 つ（UART、SPI、I2C、PWM、ADC、PIO） | 16 KiB | 非特権 RW、XN | 宣言されたペリフェラル（ウィンドウにできるもの） |
| 6 | 2 つ目のデータのリージョンのために予約 | | | |
| 7 | ウィンドウの中の部分ブロックの拒否のために予約 | | アクセス不可 | |

特権: ブート ROM `0x0000_0000`（`rom`。そのフラッシュのルーチンには ROM を実行しないと届かず、ROM は全体が拒否されます）、XIP の制御 `0x1400_0000` と SSI `0x1800_0000`（`flash`）、DMA `0x5000_0000`（`dma`、バスマスタ）、USB `0x5010_0000`（自分の DPRAM へのバスマスタで、DAGON のリンク）、WATCHDOG `0x4005_8000`（`watchdog`）、CLOCKS `0x4000_8000`（`clock`）、RESETS `0x4000_C000`（`reset`）、PSM `0x4001_0000` と SIO `0xD000_0000`（`core-control`: 2 つ目のコアを DAGON は起動せず、起動できるブロックは特権なので、1 コアのマップで足ります。GPIO の出力は SIO にあり、サービスです）、IO_BANK0/QSPI と PADS `0x4001_4000`–`0x4001_C000`（`pinmux`）、XOSC/PLL/ROSC、VREG（`power`）、TIMER、RTC、PPB。

**ESP32-S3** — `kind = "esp-pms"`、2 つの `[[core]]`、`boot_extensions =
false`（ベンダーの stage 2 で、DAGON のブート段階はありません）、`fault_address = true`、`unwind = "xtensa-world"`。マイコンの定義のコードが PMS と WCL のレジスタをプログラムするまでは `level = "none"`（HAL はドライバを公開していません）、その後はアプリで `pms` です（Espressif ESP32-S3 TRM の "World Controller" と "Permission Control" の章）。`pms` で生成されるマップ:

| 資源 | World 1（拡張） | 補足 |
|---|---|---|
| 内部 SRAM（または MMU を通した IROM）の `.ext.text` のリージョン | 読み、実行 | |
| `.ext.data`、スタックのリージョン | 読み、書き、実行不可 | スタックの下の拒否されたリージョンがガードです |
| 宣言されたペリフェラル | 読み、書き | 定義の中のウィンドウにできるものの一覧 |
| ほかのすべて: DAGON のコードとデータ、ROM、SPI0/SPI1（キャッシュとフラッシュ、`flash`）、RTC と TIMG のウォッチドッグ（`watchdog`）、RTC_CNTL（`power`、`reset`）、システムと割り込みマトリクスのレジスタ（`irq-routing`、`clock`）、GDMA（`dma`、バスマスタ）、USB OTG（バスマスタ）、AES/SHA/RSA のエンジン（GDMA を通すバスマスタ）、両コアの制御レジスタ（`core-control`）、PMS と WCL のレジスタ（`protection`） | アクセス不可 | GDMA のパーミッション制御は、どのチャネルからの World 1 のメモリアクセスも拒否します。マップは両コアに適用し、World 1 に入る前にロックします |

### 4.6 サンドボックスが保証できないこと

どのレベルでも保証しないのは、宣言された能力が物理的な世界でできることです。マニフェストが宣言したピンは、配線が許せばボードの電源を切れます。宣言したデバイスの書けるレジスタへのバスの書き込みは、そのデバイスの状態を変えられます。マニフェストが宣言し、`dagon
gen` がプロジェクトのすべての拡張の宣言された能力を表示し、ボードの定義がバスの能力をデバイスとレジスタの集合に、ピンを許すレベルに絞り、配線はボードの作者の責任です。ペリフェラルをウィンドウにできるかどうかは、メモリに書けず、割り込みやクロックを経路づけられず、DAGON と状態を共有しないという、マイコンの定義の表明です（11）。バスマスタをウィンドウにできると印を付けた定義を、検査器は受け付けません。

保証が覆うのは DAGON の公開 API とサンドボックスです。特権でリンクされるコード（DAGON 自身、信頼された拡張）は、構造上その外です。

### 4.7 信頼された拡張と認証のチェーン

信頼された拡張だけのポート（2.1）を提供するプラグインは、安全のルールを作っている部分を置き換えます。受け付けるのは、次のときだけです。

1. マニフェストが `trusted = true` と言い、提供する信頼されたポートを挙げること。
2. プロジェクトの `dagon.toml` が、ノードの `[extensions.nodes.<id>] trusted` の下に、id と crate のソースのハッシュで挙げること。ハッシュは、crate のフォルダのすべてのファイルをパス（`/` 区切り）の順に並べ、ファイルごとにパス、ゼロのバイト、長さ（リトルエンディアンの u64）、バイト列（CRLF は LF として読む）を続けたものの SHA-256 です。`include!` と `#[path]` でコンパイラがどれでも読めるので、`target/` と隠しフォルダを含め、除外するファイルはありません。そのため、拡張のフォルダの中で cargo を実行すると、ハッシュが変わります。ハッシュがソースのものなのは、`dagon gen` の実行時にリンクされたバイト列が存在しないからです。`dagon gen` はハッシュを確かめ（違うときは見つけたものを示します）、次にホストでブート用クレートの依存グラフを確かめます。cargo がすべてのフィーチャ、すべてのターゲットで解決し（`Cargo.lock` ができた後は `--locked`）、これはどの 1 回のビルドよりも広い集合です。組み込まれるすべての crate（通常とビルドの依存）は、DAGON のもの、宣言された信頼された拡張のフォルダの中のもの（つまりそのハッシュの中）、またはブート用クレートの `Cargo.lock` にチェックサムがあるレジストリの crate でなければなりません。確認を通ると、gen は `graph.stamp` を書きます。ブート用クレートの `Cargo.toml`、`Cargo.lock`、各信頼された拡張のソースのハッシュの SHA-256 です。ブートローダのビルドはこのスタンプを計算し直し、無いときや違うときはビルドしません。そのため、いまの入力についてグラフを確かめていないブート用クレートがビルドされることはありません。グラフをビルドスクリプトではなくホストで確かめるのは、ビルドスクリプトからは外側のビルドのフィーチャも、オフラインでは cargo が解決するすべての crate も見えないからです。覆わないもの: 拡張が依存するレジストリの crate（チェックサムで固定しますが、レビューはしません）と、拡張が自分のフォルダの外から取り込むソース（外へ出るパスの `include!` や `#[path]`）です。これらは、特権で動くコードとしてレビューします。
3. それを含むイメージがプロジェクトの鍵で署名され、署名が拡張のバイト列を覆い、イメージが動く前にデバイスがその署名を検証すること（下記）。
4. `dagon gen`、`dagon status`、`dagon doctor`、ページが、各ノードの信頼された拡張を id で表示し、VERSION がその id とソースのハッシュの最初のバイトを表示用に（0x08）、ハッシュ全体を識別用に（0x09）報告すること。

プラグインは、ポートを提供せずに信頼されることもできます。マニフェストが `trusted = true` と言い、イベントを観測するだけの観測者です。上の項目 2 から 4 のもとで受け付け、判断点には答えず、ハンドラの呼び出しはそれぞれ、ほかの拡張と同じくマニフェストの `max_run_us` に照らして測ります（超えた呼び出しは `<layer>.extension_overran` と報告し、止めません）。理由は、プロジェクト自身のブート時のコード（ブートをディスプレイに表示する、自分のハードウェアを動かす）が、サンドボックスが拡張に禁じるもの、たとえば DAGON のマップが開かないバスやペリフェラルをまさに必要とすることが多いからです。信頼されたと宣言すると、すべてのプロジェクトのサンドボックスを弱める代わりに、そのプロジェクトで DAGON の一部になります。

信頼された拡張は、特権で、DAGON 自身のリージョンの中で動きます。そのプロジェクトでは DAGON のその部分です。4.1–4.6 のどれも、それに対しては成り立ちません。文書は、信頼された拡張に触れるところではどこでもそう示します。

チェーンのすべての検証器（`ImageVerifier` と `Digest` のポートで、既定がこのルールです）が使う、1 つの認証のルール:

- **署名するバイト列。** イメージのヘッダの署名は、ヘッダのオフセット 0 から署名の欄の始まりまでのバイトを覆います。そこには、形式番号、イメージの種類、マイコンの識別、ターゲットのタグ、スロットのバインディング、バージョン、本体の長さ、本体の SHA-256 が入ります。本体はそのダイジェストで認証され、本体はすべての `.ext.*` のセクションを含むので、拡張のバイト列も覆われます。
- **鍵の選択。** 検証器が受け付けるのは、ヘッダが示す id の鍵だけで、その id がビルド時に検証するイメージに埋め込まれたプロジェクトの鍵のときだけです（`DAGON_PUBKEY`。公開のテスト鍵は、それを選んだビルドだけ）。デバイスに鍵の一覧はなく、予備の鍵もありません。
- **だれが何を検証するか。** エージェントは `written` の前にイメージを検証し、ブートローダは試行のブートで `chosen` の前に検証します（ダイジェストはすべてのブートで確かめます）。ブートローダのイメージは、エージェントがこのルールで `written` の前と `pending_set` の前にもう一度検証します（SLOT_PENDING の記録がそのヘッダのダイジェストを示します）。定義が `[layout] stage0_verifies` を設定する stage0 のある self-boot のマイコンでは、stage0 はダイジェストに加えて、組み込まれたプロジェクトの鍵でブートローダのヘッダの署名を確かめます。省くと、stage0 はダイジェストだけを確かめます。ブートローダを起動するのは、ヘッダのダイジェストがブートローダの記録の示すもので、本体がそのヘッダのダイジェストと一致するときだけです。記録があるのはエージェントが署名を検証したコアだけなので、このチェーンが成り立つのは、状態ログを書くのが DAGON のエージェントとブートローダだけであるあいだです（拡張からはサンドボックス、4、が守ります）。署名の鍵なしでフラッシュに書けるコード（信頼されたコードの不具合、またはフラッシュに触れられる攻撃者）は、署名の無いコアを stage0 に起動させられます。検証器は、下の余裕の範囲では、どちらの stage0 の領域にも収まりません。ログの選択が確かめられないとき（壊れたスロット、またはコアの無いスロットを示す記録）は、stage0 は起動するものが無くなる前に、ほかのスロットを単独で試します。stage0 の無い self-boot のマイコンでは、ブートローダがルートで、書く前にホストが、書き込みの経路で書くときはエージェントが検証します。
- **stage0 の上限。** マイコンの定義の `[layout] stage0.size` は stage0 の全体です（ベクタテーブル、ログの読み手、検査、試行のセル）。イメージの memory.x はちょうどその領域を与えるので、stage0 がそれを超えるとリンクが失敗します。RP2040: boot2（ブート ROM の 256 バイトの第 2 段階）を含めて 16 KiB。STM32F401RE: 16 KiB、コアがリセットベクタを取るセクタ 0 のすべて（それを持つ最小の消去単位なので、どの書き込みも stage0 を消去しません）。どのビルドも領域の 1/16 を空けなければなりません（`scripts/stage0_size.py`、CI で実行）。ボードの無いビルドと、定義がそのマイコンを示すボードごとに 1 つです。ボードはブートの LED と停止のパターンを stage0 に入れるからです。stage0 に現場で書き込むことはありませんが、新しいノードごとにそのビルドのツールチェーンでビルドします。そのため、余裕の無いイメージは、ビルドできない最初の書き込みになります。どちらの stage0 もコンパクトな SHA-256 のラウンドを使います。展開したラウンドは RP2040 の領域に収まらないためです。もっと必要な stage0 は、最初の公開リリースの前により大きな領域を取ります。それ以前の形式に互換の約束はありません（6）。
- **試行のウォッチドッグの下で消去しない。** stage0 はフラッシュに書きません。ブートローダは記録（BOOT_ATTEMPT、ROLLBACK、ブートローダの CONFIRMED か ROLLBACK）を、ログにすでにある場所にだけ書きます（`Log::append_in_place`）。エージェントは、イメージを待機にするときに、その試行のための場所を予約し（`Log::reserve`、`trial_room`）、電断で記録が途中で切れた後は、アプリのエージェントが起動時にそれを戻します（`SlotStore::keep_room`）。消去はアプリの中だけで、定義の確認が十分に長いと証明したウォッチドッグの周期の下で行います。`[watchdog]` の両方の周期は、`[execution]
  erase_unit_max_ms` の 2 倍以上です。
- **試行のセル。** 試行のセルが書いたものを保持しないノード（`bootcore::cell_holds`、アプリのストアとブートローダが確かめます）は、ブートローダを受け取りません（XFER を unsupported として拒みます）。それが壊れていると気づいた待機中のコアは自分のロールバックを記録します。そのため、壊れたコアを回数なしで再試行することはありません。

- **ルートイメージ**（あれば stage0、無ければブートローダ）は、準備のときにマイコンの ROM ローダで書きます。ROM ローダは DAGON のものを何も検証しません。ROM ローダを動かせる者はルートを置き換えられ、それとともにすべてのルールを置き換えられます。そのため、このチェーンが成り立つのは、DAGON が提供するすべての書き込みの経路（リンク、UDS のような置き換えた転送、親機のリレー）に対してで、ROM を通した物理的な書き直しに対してではありません。マイコンがルートイメージの保護を持つところでは、定義がそのレベルを挙げ（`[isolation]
  root_protection`: STM32 では、`rdp1` は全消去で元に戻せ、`rdp2` は元に戻せず、セクタの書き込み保護 `wrp` もあります。RP2040 には何もなく、ブート ROM は常に BOOTSEL に従います。ESP32-S3 にはセキュアブートとフラッシュの暗号化の eFuse があります）、`dagon provision` がレベルの設定を提案してどれが元に戻せるかを示し、VERSION が正確なレベルと保護されたセクタを報告します（0x07）。そのためホストは、ルートが置き換えられるノード、元に戻せる形で保護されたノード、元に戻せない形で保護されたノードを示します。

### 4.8 アプリ、リレー、ホスト

サンドボックスのアプリの拡張（レベル `mpu`/`pms`/`tz`）は、ハンドラについて 4.1–4.4 の保証を持ちます。レベル `none` のアプリの拡張が持つのは、API の形の保証だけです。

| ルール | 強制するもの |
|---|---|
| ストアにアクセスしない | 拡張は、自分のリージョンにコピーされた読み取り専用のペイロードを受け取ります。アプリケーションは `ImageView` を得ます（2.3）。スロットのストアの変更メソッドには、イメージのライフサイクルの状態機械を通してだけ届きます。 |
| 検証と確定の順序 | 状態機械の遷移だけが状態を変えます。`self_check` に尋ねるのは、「試行中」のガードの後の `confirmed` の遷移の中だけです。`accept_update` は DAGON のガードの後だけです。拡張の答えは検証された `enum`（`RETURN`）で、DAGON が実行するクロージャではありません。プロトコルのプラグインはトリガーを作り、セッションの状態機械がガードを実行します。 |
| 時間 | 判断の上限は 3 にあります。通知にも同じ上限があり、ハードウェアのレベルでは割り込み、`none` では測ります。 |
| ウォッチドッグ | どのサービスも触れません。 |
| 再起動 | 3.2: 遅れに上限があり、拒めません。 |

リレーの拡張はアプリの拡張です。ホストの拡張は、利用者の権限で利用者の OS の中で動きます。データで定義するもの（アクション、エージェント、プログラムで指定する転送）は、共有の command の転送の上限のもとでプロセスの外で動きます。ホストの拡張は判断に答えません（3.4）。

### 4.9 適合試験のケース

マイコンの定義のコードの適合試験のビルドは、デバイス上の拡張から次のものを実行し、記録された結果を期待します。定義の各特権のブロックへの書き込み（フォールト）。DAGON のコードの読み出し（フォールト）。DAGON のコードへのジャンプ（フォールト）。偽造したサービスの id（フォールト）。リージョンの外のサービスのポインタとオーバーフローする長さ（拒否）。サービス呼び出しの前の不正な非特権のスタックポインタ（種類の文書にある結果: スタッキングのエラーとアンワインド、またはロックアップ、ウォッチドッグによるリセット、ヒューズのセットと次のブートでの拡張の無効化）。持ち時間を超えるループ（超過）。宣言されたペリフェラルからの割り込みの嵐（割り込み元の無効化）。ウィンドウにできないウィンドウを通した DMA の要求（生成時に拒否）。入れ子のフォールト（アンワインド 1 回）。同じ呼び出しのタイマとフォールト（アンワインド 1 回）。マイコンにある別のコアの起動の試み（フォールト）。`pms` のマイコンでは World 1 からの GDMA のアクセス（拒否）。サービスについて足すもの: 4.2 の取り消しの境界のケースと、リセット時の取り決めのケース（各プリエンプトできない部分の中のウォッチドッグによるリセットの後、最初の使用が成功する）。ヒューズについて: そのモデルの要件（5.5）のシナリオをデバイスで実行します。ケースは定義から生成するので（特権のブロックごと、コアごと、バスマスタごと、サービスの取り消しの境界ごとに 1 つ）、新しいマイコンはその定義からケースを得ます。各ケースは実行可能なモデル（5.5）のシナリオでもあり、ブートが拡張なしのときと同じ判断に達することを確かめます。

### 4.10 同じモデルでのほかのアーキテクチャ

モデルのマイコン固有の部分はすべて定義のデータで、それぞれトレイトの後ろに小さな閉じた仕組みの集合があります。

| 部分 | 定義のデータ | 仕組み（それぞれモジュール 1 つ） |
|---|---|---|
| 保護マップ | `[isolation] kind` とパラメータ、`[[peripheral]]`、`[[core]]` | `ProtectionKind`（4.4） |
| アンワインド | `[isolation.capabilities] unwind` | `cortex-m-exception`、`riscv-trap`、`xtensa-world`、`armv8m-tz` |
| サンドボックスのタイマ | `[isolation.capabilities] timer` | `systick`、`riscv-mtime`、`timer-group` |
| ウォッチドッグ | `[watchdog] kind, trial_ms, confirmed_ms` | 種類ごとの `WatchdogDriver` のポートの既定（`iwdg`、`rp-watchdog`、`rtc-wdt` など） |
| ブート記録の配置 | `[boot.record] kind, at, survives, rings, entropy`（entropy: 起動のたびに違う起動時のワードの出所。ブート記録には必須） | `noinit-ram`、`retained-registers`（5.5） |
| ロックアップヒューズ | `[boot.record] fuse`、`[watchdog] flag`（なし、またはリセットを越えて残り、起動が確認するウォッチドッグによるリセットのフラグの、レジスタかマイコンの定義のコードの出所） | ヒューズが使えるのはフラグがあるときだけです。それ以外は読み込みが使えないと報告します（5.5） |
| ルートの保護 | `[isolation] root_protection` | マイコンのファミリごと、`dagon provision` が適用します |
| キャッシュと TCM | `[isolation] cache, tcm` | ウィンドウの属性とバウンスバッファの配置（下記） |

最初の 3 つのマイコンには無いものの、同じスキーマに収まる種類:

- **TrustZone-M を持つ ARMv8-M**（Cortex-M23/M33/M55/M85）: 種類の表（4.4）の `armv8m-tz` の行が、その能力と保証のすべてです。表のほかにこのファミリに固有なのは、バスマスタがインターコネクトの帰属のユニットで Secure か Non-secure になることで、定義がマスタごとに挙げます（`bus_master_attribution = "secure" | "configurable"`）。拡張の入出力で使う設定可能なマスタは、DAGON の DMA のサービスを Secure として通します。
- **RISC-V PMP/ePMP**（ESP32-C/H シリーズ、RP2350 の Hazard3、ほか多数）: 種類の表の `riscv-pmp` と `riscv-epmp` の行が、項目、アドレスモード、保証を示します。表のほかに固有なのは、ESP32-C/H の部品がペリフェラルと GDMA のための Espressif の PMS も持つことで、その定義はメモリの `riscv-pmp` と `esp-pms` のペリフェラルと DMA の制御を組み合わせます（2 つを合わせるモジュールの種類 `riscv-pmp+esp-pms`）。
- **Cortex-M7 とほかのキャッシュのあるコア**: 種類の表の `armv7m-mpu` の行です。定義は `cache = "id-cache"` と TCM の範囲を示します。生成器は能力のウィンドウを Device（キャッシュなし）、コード、データ、スタックを Normal と印を付け、DMA のバウンスバッファをキャッシュされないリージョン（DTCM の範囲か、キャッシュなしの属性の MPU のリージョン）に置きます。DMA のサービスは、バッファがキャッシュされるときは送信の前にクリーンし、受信の後に無効化します。`.ext.text` はフラッシュか ITCM に置け、アンワインドは変わりません。バウンスバッファや TCM のリージョンを置かない生成器は、データキャッシュや TCM を持つ定義を、無視せず、`plan` で欄の名前を示して拒みます。
- **DAGON が 1 つのコアを使うデュアルコアのマイコン**: もう一方のコアは `[[core]]` の下に挙げ、その起動とコア間のブロックは `core-control` です。マップは、種類がコアごとに設定するところ（`esp-pms`）ではすべてのコアに適用します。ユニットがコアごとで 2 つ目のコアを起動しないところでは、検査器が起動のブロックを特権にすることを求めます。

どの種類も保証しないもの: 宣言された能力の物理的な効果（4.6）、特権のコードの DAGON 自身の不具合、ROM ローダが書き直せるもの（4.7）。

## 5. ボードの値、アクションプログラム、共有の資源、VERSION、モデルで決める形式

### 5.1 ボードの値は定義から、マイコンのコードからは決して来ません

`dagon-board.toml` は `[boot]` のテーブルを持ちます。`led`（ピンの名前か GPIO の番号）、`led_active = "high" | "low"`、`halt = { pattern = "blink" |
"code" | "off", period_ms }`、任意の `power_hold = { pin, level }`、`[[boot.action]]` の項目（5.2）です。そして、拡張の能力を照らすテーブル `[[pin]]`、`[[bus]]`、`[[peripheral]]`、`[[device]] { bus, addr, writable = [regs] }` も持ちます。定義の読み手は知らないキーを受け付けないので、これらのテーブルを使う定義は、それらを持つ `definitions` の形式を示します（6）。`dagon gen` はブートローダのビルドのために `boot_wiring.rs` を書きます（`LED`、`HALT`、`PROGRAM`、`RECORD`、拡張のチェーンの型とその保護マップ）。ボードの定義は、ブートローダのビルドが同じように読みます。マイコンの crate はこれらの定数を受け取り、自分ではピン、パターン、環境変数の既定を持ちません。

`dagon-mcu.toml` は、ボードではなくマイコンが決めるものを持ちます。`[watchdog] trial_ms, confirmed_ms`、`[isolation]`（4.4）、`boot_capabilities`（5.2）、`[boot.record]`（5.5）です。

### 5.2 ブートのアクションプログラム: いちばん単純な拡張の種類

LED のパターンや電源保持の線だけが要り、コードが要らないボードのために、ボードの定義は DAGON が解釈する宣言的で有界のプログラムを持ちます。チェーンのほかのものと同じ拡張で（ボードの定義の位置）、アクションに使えるイベントを観測し、同じルールで動きます。`WatchdogArmed` 以降でだけ、宣言されたピン、バス、デバイスでだけです。

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

操作の集合、`boot-program` 形式 1:

| 操作 | 欄 | コスト | 補足 |
|---|---|---|---|
| `led` | `state: on \| off \| toggle` | ~0 | `[boot] led` のピン |
| `gpio` | `pin`、`level: high \| low \| input` | ~0 | ピンはボードのピンの表にあり、`boot = true` の印が必要です。設定してそのままにします。宣言された `power_hold` のピンでは、保持のレベルだけを受け付けます |
| `delay` | `ms ≤ 20` | `ms` | イベントごとの合計 ≤ 50 ms |
| `bus_write` | `device`（`[[device]]` の項目）、`reg`（その `writable` の一覧にあるもの）、`bytes ≤ 8` | タイムアウト 5 ms | マイコンの定義が `boot_capabilities = ["i2c"]` を宣言するときだけ。任意のアドレスやレジスタは検証が拒みます |

上限（`dagon gen` が検証し、インタプリタが確かめ直します）: イベントごとに `ops ≤
32`、イベントごとに `delay total ≤ 50 ms`（バスの操作はタイムアウトで数えます）。ループ、分岐、読み出し、入力の待ち、フラッシュや状態へのアクセス、クロックやウォッチドッグの制御はありません。知らない操作に出会ったブートローダは、プログラム全体を無効として扱います（`boot.program_skipped`）。

### 5.3 共有の資源

複数の者が必要とするボードの資源は、持ち物ではなく共有です。

- ボードの定義が立ち上げと復旧に必要とし、アプリが自分のデバイスに必要とするバスは、`pub static SYS_I2C:
  Shared<I2c<'static, Blocking>>`（`dagon_events::Shared<T>`。`try_lock` で取る `embassy_sync::mutex::Mutex`）として公開し、`with(|bus| ..)` で使います。これは貸し出しで、クリティカルセクションではありません。貸し出しのあいだ割り込みは許可のままです。長さに上限の無い操作を割り込みをマスクして動かさず（実行モデル）、バスのドライバが自分の割り込みを待つことがあるからです。入れ子や並行の `with` は、待たずにビジーを返します。DAGON の立ち上げと復旧は `with` で取り、アプリはいつでも同じようにします。サンドボックスの拡張は、宣言したデバイスでバスのポートの呼び出しを通して届きます。各デバイスは、ボードの定義の `[[device]]` で、書けるレジスタと `max_transfer`（デバイスのデータシートから）を持ちます。ポートはほかのレジスタやそれより長い転送を拒みます。ボードの定義はデバイスの定数とレジスタのレベルの関数を公開するので、アプリは何もコピーしません。
- ボードの立ち上げは、`app.bring_up` でのボードの定義の拡張です（`BoardBringUp` のポート）。利用者のステップは、同じイベントでのチェーンの後の拡張か、`Latest.phase ==
  Serving` を待つタスクです。
- ボードの定義が持つのはボードで、その上に積むものではありません。積むペリフェラルのドライバは、それを使うアプリとともにあり、ボードの定義はバスのピンを単なるペリフェラルとして渡します。

### 5.4 VERSION の問い合わせ

コマンド `Cmd::Version` で、リンクの取り決めの一部です。実装していないノード（復旧用のエージェント、最小のイメージ）は、知らないコマンドのルールで `E_UNSUPPORTED` を返し、ホストはそれを "versions
unknown" と表示します。リクエスト: 要約の集合なら空、ブート記録のページなら 1 つのセレクタ `[0x21,
offset u16 LE, len u8]`。レスポンス: TLV で、`u8 count` と、その後に `u8 id, u8 len, bytes`。読み手は知らない id を読み飛ばします。デバイスが決められない値は省き、番兵の値は使いません。各 TLV の値は最大 255 バイトで、レスポンス全体がいちばん小さいリンクのペイロードに収まります（10）。

| id | 値 |
|---|---|
| 0x01 | ブートローダのバージョン（`Version`、LE u32） |
| 0x02 | インストールしたブートイメージの識別: その SHA-256 の最初の 8 バイト |
| 0x03 | エージェント（`dagon-agent`）のバージョン |
| 0x04 | マイコンの定義のバージョン |
| 0x05 | ボードの定義のバージョン |
| 0x06 | マイコンの識別（5.5）: イメージのヘッダとコンパイル済みの構成が持つ値。len 2 ではイメージのヘッダの `mcu_id`、u16 LE |
| 0x07 | 隔離（4.5）: `level u8`、`kind u8`、`boot_extensions u8`、`root_protection`（定義が列挙する種類と正確なレベル。例えば `rdp1`/`rdp2`、書き込み保護されたセクタ、レベルが元に戻せるか）、`fuse`（Clear か、モデルの符号化の無効にした拡張の識別を伴う Tripped、5.5） |
| 0x08 | 拡張、表示用: リンクされた拡張ごとに `id`（最大 32 バイト）、`trusted u8`、ソースのハッシュの最初の 8 バイト（4.7）。TLV を繰り返し、拡張ごとに 1 つ |
| 0x09 | 拡張、識別用: 信頼された拡張ごとに 32 バイトのソースのハッシュ全体（4.7）。TLV を繰り返します |
| 0x0A | stage0 のバージョン: stage0 が領域に持つバージョンの印（`dagon_format::stage0`。`Version`、LE u32）。印の無い stage0 と vendor-stage2 のマイコンでは省きます |
| 0x0B | ブートローダのスロット A、B: `active u8`（動いているスロット）、次にスロットごとに `state u8`（STATUS のスロットの状態）と `version`（LE u32） |
| 0x0C | アプリのスロット A、B、factory。0x0B の形です |
| 0x0D | エージェントが話すリンクのバージョン（`PROTO_VERSION`、u8） |
| 0x10 | 形式: `(format id u8, number u8)` の組: wire、image-header、state-log、event、definitions、boot-program、boot-record、extension-manifest |
| 0x20 | ブート記録の要約（5.5）: 記録にどのブートがあるかと、いちばん新しいものの判断と理由。レイアウトはモデルが決めます |
| 0x21 | 生のブート記録の領域の 1 ページ: `offset u16` と、その後に `len` バイト（`len ≤ RECORD_PAGE`、10）。ホストは領域をページごとに読み、モデルの読み手で解読します |

動いているブートローダは、アプリのコンパイル時の相手ではなく、フラッシュから識別します。

- **ブートローダのスロットとインラインのヘッダを持つ self-boot**: アプリはブートローダのログを読んでどのブートローダのスロットが起動したかを知り、次にそのスロットのヘッダ（0x01）を読み、イメージをハッシュします（0x02）。
- **トレーラのヘッダを持つ self-boot**: アプリはトレーラ（0x01）を読み、本体をハッシュします（0x02）。
- **vendor-stage2**: インストールされたブートローダに DAGON のヘッダはありません。アプリはベンダーのブートローダのオフセットでフラッシュから読み、ベンダーのイメージのセグメントの表をたどって長さを決め、ハッシュし（0x02）、0x01 を省きます。ホストは 0x02 を、準備のときに記録した識別（`host.provision.recorded`、プロジェクトに保存）と、マイコンの定義のベンダーの表に照らし、バージョンか "unknown vendor stage (hash …)" を表示します。

`dagon status` は、ノードごとに 1 行で、これらの値、隔離のレベル（"guarded" / "unguarded extensions"）、信頼された拡張、最後のブートの要約を出力します。`dagon doctor` は形式番号を `compat.toml` と比べ、CLI の読まない番号と、レベル `none` のノードにリンクされた信頼されていない拡張について警告します。

### 5.5 ブート記録、マイコンの識別、コンパイル済みの構成: 要件と実行可能なモデル

このモデルの 4 つの形式は、リセット、電断、部分的な書き込み、バージョンのずれのもとでのバイト単位の動きに正しさが依存する、保持や永続のデータに触れます。保持メモリのブート記録、その隣のロックアップヒューズ、イメージのヘッダ、識別の返答、VERSION が持つマイコンの識別、そしてコンパイル済みの構成のノードのレイアウトと置き換えです。そのような形式が故障のどんな組み合わせにも耐えることを、文章では示せません。実行可能なモデルなら示せます。そのため、これらの形式はこの文書では定めません。ランダムな故障の注入を持つホスト側の Rust のモデルとして設計し、性質を通った形が、各形式の書かれた仕様になり、モデルのテストを証明として公開します。モデルは、サンドボックスの観測できる取り決め（ブートのどのステップでも超過、フォールト、能力の誤用をする拡張）も、ブートの状態機械の続きに対して試します。

この節は、それらの形式が満たすべきことを示します。モデルを受け入れるのは、下のすべての性質が故障の注入のもとで成り立つときだけです。通った形式は、モデルの性質の一覧と各主張の限界とともに、[retained-formats.md](/explanation/retained-formats/) で定めます。

#### ブート記録

- **観測できること。** すべてのブート段階（stage0、ブートローダ）のすべてのブートの遷移を記録し、アプリのエージェントが、起動時に破損していない記録済みのイベントをすべて、カタログのイベントとして、`replayed: true` と、ブート、書き手、位置を区別する識別子を付けてリプレイします。停止した、ハングした、アプリに達しなかったブートは、その後で最初に動くアプリがリプレイします。イベントの欠落（容量）もそれ自体を記録してリプレイします。
- **途中で切れた書き込みへの安全。** どのバイトで中断された書き込みも、以前に有効だったスロットの上書きを含め、失うのは最大で書いているイベントで、読み手が破損していないと受け付ける別のイベントを生みません。欄をその場で変えることはなく、読み手が受け付けるものが 2 回の書き込みの産物であることはありません。
- **書き手の独立。** 書き手は、書き込みの位置を、マイコンの定義の定数と自分が検証したデータからだけ導きます。ほかの書き手の中身を解釈しないので、段階はもう一方の前でも後でも、どの順でも新しいバージョンにでき、不正な保持メモリが書き込みを記録の外に向けることはありません。識別子は、どちらの順で段階を新しいバージョンにしても一意で、その前に何も書いていない最初の段階の場合を含みます。
- **順序。** ブートは、シーケンスの一周をまたいで古い順にリプレイします。1 つの書き手がシーケンスの空間の半分より長く黙っていた場合も含みます。読み手は、新しいブートを古いブートより先に報告しません。順序づけられない組は、不確かと印を付けます。両方のイベントがまだ手元にあるあいだ、識別子が繰り返すことはありません。
- **判断の入力にならないこと。** `plan`、`commit`、`self_check`、`confirm` がこの記録を読むことはありません（4.1）。モデルの API は、ブート段階に `append` だけを、エージェントに `events` だけを与えます。拡張は届きません（拒否されたリージョン）。
- **バージョン付け。** 保持のレイアウトは、読み手が読めないレイアウトを拒むために要るものを持ちます（6）。古いか新しいレイアウトの保持のバイトを、読み手のレイアウトの有効なイベントとして受け付けることはありません。
- **大きさ。** 領域は最大 `RECORD_MAX` バイトで、`RECORD_PAGE(link)` バイトのページで読みます（10）。場所と書き手ごとの大きさは、マイコンの定義の `[boot.record]` で宣言し、1 つの経路（マイコンの crate のビルドスクリプト）ですべてのイメージの `memory.x` に届きます。イメージが一致することをテストが確かめます。
- **リプレイの意味。** 少なくとも 1 回です。記録には何をリプレイしたかがなく、受信側は識別子で重複を除き、エージェントは 1 回の実行で同じ識別子を 2 度リプレイしません。
- **状態ログからの独立。** `app.trial_started` と `app.rolled_back` は、記録に関わらず状態ログから来ます。

#### ロックアップヒューズ

- **所有者。** ヒューズを書くのは、サンドボックスの呼び出しを動かそうとしているレイヤー（ブートローダ、アプリ、リレー）だけで、読むのはすべてのレイヤーの起動です。ほかのコードは読み書きせず、ブートの判断、確定のルール、自己検査の入力にはなりません。
- **識別。** ヒューズは、イメージ（スロットとイメージのヘッダのダイジェスト）、拡張（イメージのチェーンの位置と、成果物のハッシュ全体）、レイヤーを示し、どのイメージのどの拡張のほかのビルドとも一致しないほど完全です。モデルは衝突が起きないことを構造として試験し、符号化が拡張やスロットの数に上限を設けることはありません。
- **誤ったトリップが無いこと。** 有効化、呼び出し、戻り、クリアのどの時点のリセットも、戻った呼び出しを示すヒューズを作りません。ロックアップした呼び出しの中のリセットは、常にその呼び出しを示すヒューズを作ります（リセットを越えて残るかはマイコンの定義の宣言のとおりで、電断はヒューズを失うことがありますが、偽造はしません）。
- **引き渡し。** アプリやリレーの呼び出しが有効にしたヒューズは、ブートローダの起動をそのまま通り抜け（ブートローダはそれを記録し、ほかのレイヤーのヒューズをクリアせず、それに基づいて動きません）、アプリの起動が読み、示された拡張をその実行のあいだ無効にします。ブートローダの呼び出しが有効にしたヒューズは、ブートローダの次の起動がそれに基づいて動きます。
- **古いものと新しいバージョン。** 識別がいまインストールされているイメージのどの拡張とも一致しないヒューズ（イメージか拡張が置き換わった）は、古いものとして記録し、それに基づいて動くはずだったレイヤーがクリアします。ほかのレイヤーはクリアしません。成果物のハッシュが変わった拡張は新しい識別で、もう一度動きます。
- **クリア。** DAGON には、ヒューズを消す要求はありません。プロトコルにも CLI にも無いので、識別がまだ存在するヒューズはトリップしたままです。retained-formats.md の 3.6 は、形式のモデルのクリアです。
- **バージョン付き。** ヒューズは、読み手が読めないレイアウトを拒むために要るものを持ちます（6）。別のレイアウトのバイトに基づいて動くことはありません。
- **大きさと配置。** マイコンの定義のブート記録の隣で宣言し（`[boot.record] fuse`）、同じリンカの経路ですべてのイメージに届きます（イメージの一致を確かめるテストが 1 つ）。
- **ウォッチドッグのフラグ。** ヒューズには、電源投入以外のすべてのリセットを越えて残り、起動が確認したときだけ消えるウォッチドッグによるリセットのフラグが要ります。マイコンの定義は、マイコンがそれを持つかどうかと出所（ハードウェアのレジスタか、マイコンの定義のコードが用意するもの）を宣言します。無いと、そのマイコンではヒューズを使えず、定義の読み込みがそれを示します。

#### マイコンの識別

- マイコンは、手で割り当てた番号ではなく、定義の `id` から導いた値で識別します。マイコンを足すのは定義で、表の編集ではなく、数値のマイコンの表はどこにもありません（テストがソースを調べます）。
- 識別は一意です。定義の読み込みは、識別を持つどの欄の幅でも衝突する 2 つの定義を、両方の id を示して受け付けません。読み込んだどの定義も一致しない識別のデバイスは不明と表示し、別のものに一致させることはありません。
- イメージのヘッダ、識別の返答、VERSION は、ホストと親機にとってあいまいでないとモデルが示す形でこれを持ちます。場所ごとの幅とハッシュ関数は、モデルの出力です。

#### コンパイル済みの構成

- **状態機械。** `Absent → Written`（最初のコピー）と `Written → Written`（置き換え）で、どちらもターゲット TOPOLOGY のイメージのライフサイクルの `written` を通り、どちらも `app.written` と `relay.children_set` を出します。
- **行き来。** ホストが送ったコピーは、親機で同じ構成（ノードの id、リンク、アドレス、マイコンの識別）として読み返せます。`host.topology.pushed` はそのダイジェストを持ち、親機の `relay.children_set` が繰り返します。
- **上限。** 親機の TOPOLOGY の領域より大きい構成は、1 バイトも送る前にホストが拒み（`E_RANGE` には達しません）、それでも送られたら親機が拒みます。
- **不正や未対応。** 親機が読まないレイアウトの番号のコピーや、自身の完全性の検査に失敗するコピーは、無いものとして読み、`relay.children_set { source: Fallback }` で報告し、プロジェクトの予備の一覧を使います。一部だけ適用することはありません。
- **中断された置き換え。** どのバイトで、1 度でも繰り返しでも中断された置き換えも、親機が読むのは、ちょうど前のコピーか、ちょうど新しいコピーです。モデルは中断点ごとにどちらかを示し、結果は次の書き込みまでの以後の読み出しで同じです。
- **古いコピーの正確な結果。** 前のコピーを残した中断された置き換えの後、前のコピーはバイト単位で元のままです。完了した置き換えの後は、前のコピーには届きません。

#### 文章で定めるもの

この文書のほかのバイト単位の構造は、ビルド時に生成してデバイスに届く前に検証するもの（ブートのアクションプログラムの表、`boot_wiring.rs` の定数、拡張の保護マップ）、永続を持たないリクエストとレスポンスの符号化（VERSION の TLV の枠、serve の JSON のメッセージ）、このモデルが変えない既存の取り決め（イメージのヘッダの署名とダイジェストの欄、状態ログ、ベンダーの選択データ、リリースのバンドル）のどれかです。どれも 2 度書くことも、故障の後で読み返すこともないので、モデルは要りません。モデルのハーネスが、VERSION のページの読み出しをすべてのリンクの大きさで試すことはあります。

## 6. バージョン付けのルール

番号と部品は[バージョンとリリース](/explanation/versioning/)に従い、`compat.toml` に登録します。このモデルに固有のルール:

| 取り決め | 番号が動くとき | 読み手のルール |
|---|---|---|
| `event`（`EVENT_FORMAT`） | 種類の名前を変える、消す、またはペイロードの意味を変えるとき。種類やペイロードのフィールドを足しても変わりません | 読み手は知らない種類とフィールドを読み飛ばし、読めない番号を受け付けません |
| `extension-manifest` | マニフェストのキーの意味が変わる、または読み手が拒むキーを足すとき | `dagon gen` は読めない番号のマニフェストを、両方の番号を示して受け付けません |
| `boot-program` | 操作を足す、消す、またはその欄が変わるとき | 知らない操作に出会ったブートローダはプログラム全体を無効として扱い、`boot.program_skipped` を出します。`dagon gen` は、番号がそれを読まないブートローダのためにプログラムを生成しません |
| `boot-record`（とその隣のロックアップヒューズ） | 古い読み手が読み違える形で保持のレイアウトが変わるとき（モデルの互換性のテストが決めます、5.5）。書き手のイベントの符号化は、その `event` の番号とともに、書き手ごとに独立して変わります | 番号を読まない読み手は記録を読めないと報告し、VERSION 0x21 で生のページは返します。ある書き手の `event` の形式を読まない読み手は、その書き手のイベントを数え、中身を読み飛ばします |
| `image-header`、`topology-bin` | マイコンの識別の欄やノードのレイアウトが変わるとき（5.5） | ブートローダ、エージェント、親機は、`compat.toml` が挙げる番号を受け付け、ほかを番号で拒みます |
| `action-defs` | アクションのファイルのキーが互換性なく変わるとき | CLI の読まない番号のアクションのファイルは、エージェントの定義と同じく、読み込まなかったと報告します |
| `definitions` | 読み手が拒むキーを足す（読み手は知らないキーを拒みます）、またはキーの意味が変わるとき | 定義は必要な番号を示します。CLI は対応するすべての番号を読み、拒むときは両方の番号を示します |
| `wire` | コマンドのリクエストや返答の意味が変わるとき。コマンドを足しても変わりません（知らないコマンドのルールが `E_UNSUPPORTED` を返します）。デバイスが求められていないフレームを送ることはありません（リンクはリレーを通したリクエストとレスポンスです） | コマンドを実装していないノードは `E_UNSUPPORTED` を返し、ホストはそれを不明と表示します |
| `serve-api` | 追加である `event` のメッセージでは変わりません | 受信側は知らないメッセージの型を読み飛ばします |

最初の公開リリースより前の形式に互換の約束はありません。最初の公開リリースに入る形式が、各取り決めの番号 1 で、それより前の開発中の形式を読み書きするものはありません。そのリリース以降は、上のルールがあてはまります。

部品の種類: crate の公開 API の互換性のない変更は、その部品の破壊的な変更です。既定を持つ新しいイベント、ポート、操作、TLV の id、定義のキーは機能の追加です。ほかは一般のルールに従います。ポートのトレイトがメソッドを増やすのは既定があるときだけです。ポートのメソッドを消す、または変えるのは、レイヤーの部品の破壊的な変更です。

廃止: このモデルが置き換える公開のトレイトや関数（ストアのアクセサ、起動の引数）は、その部品の 1 回のマイナーリリースのあいだ、置き換え先を示す `#[deprecated]` を付けて残し、次の破壊的なリリースで消します。`CATALOG` の項目は、1 つの形式番号の中で消しません。もう出さない種類は、次の `event` の番号まで `retired` の印を付けてカタログに残します。

## 7. DAGON つきアプリの付け外し

ブートローダが求めるのは、このマイコン（その識別で、5.5）、ターゲットのタグ、スロットの署名付きのヘッダを持つスロット、ダイジェストが一致する本体、本体の先頭のベクタテーブル、そしてブートローダが有効にしたウォッチドッグをキックするアプリです（試行では: 試行の周期の中で確定するか、ロールバックされます）。DAGON をリンクしないアプリも、ROM の経路でインストールでき、署名の後で動きます。確定はしないので、`dagon provision` が確定としてインストールしない限り、`max_attempts` の後でロールバックします。DAGON を付けるには: マイコンとボードの定義を足し、拡張のチェーン（`Nop` も有効です）で `start` を呼び、返される `Reader` で公開のイベントを読み、ガードを通してキックし続けます。外すには: `start` を呼ぶのをやめます。ブートローダは引き続き検証して試行を数え、自分の拡張を動かし、ブート記録を書きます。ホストには識別の返答が見えず、`dagon
status` がそう示します。宣言した信頼された拡張（4.7）を除き、どの拡張、イベント、定義、ブートのアクションも、ブートローダが確かめるものを変えません。

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
            Phase::Transferring => lcd.bar(s.progress), // Some((done, total))
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

宣言された I2C デバイスの後ろにある電源管理で、マイコンの定義が `i2c` のブートの能力を宣言し、ボードがそのデバイスを宣言している場合:

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

または非同期で、応答のタスクの時間に触れずに: `Reader` から `events.recent()` を取り出すタスク（所有された `EventRecord`、2.4）。

### 8.4 書き込みの転送とプロトコルの置き換え: CAN の上の UDS

車載のノードは、シリアルリンクの上の DAGON のリンクではなく、ISO-TP（ISO 15765-2）を使う CAN の上で UDS（ISO 14229）を話す診断テスタが書き込みます。1 つのプラグインが 2 つのポートを提供し、ブート時とアプリで動きます。

マニフェスト:

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

ブート段階。ブートローダはプラグインを `.ext.*` にリンクし、生成されたマップ（4.4）が、マイコンの定義でウィンドウにできる CAN1 のレジスタブロックと、自分のメモリを与えます。フラッシュコントローラ、状態ログ、ウォッチドッグ、DAGON の RAM は拒否します。`WatchdogArmed` から、ブートの状態機械は、ステップのあいだに限られた時間枠（プロジェクトの `[boot] service_window_ms`。試行の周期が上限です）のあいだ `UpdateTransport` のポートをポーリングします。`DiagnosticSessionControl(programmingSession)` を送るテスタは、ブートローダを復旧の経路に保ち（`boot.recovery { path: Service }`）、プラグインの `UpdateProtocol` が対応づけます。`RequestDownload` → `update_offered`、`TransferData` → `header_chunk`/`data_chunk`、`RequestTransferExit` → `transfer_end`、`RoutineControl(switchSlot)` → `switch`、`ECUReset` → `reboot` です。セッションの状態機械は、DAGON のリンクとまったく同じく、トリガーごとに DAGON のすべてのガードと検証器のポートを実行します。そのため、テスタが署名を飛ばしたり、動いているスロットに書いたりすることはできません。プラグインの `Diagnostics` のポートは、宣言した識別子について `ReadDataByIdentifier` に答えます（VERSION が持つのと同じ値からのバージョンと隔離のレベル）。各ハンドラの呼び出しは `max_run_us` の下で動きます。止まった CAN コントローラは超過のイベントで、ブートは時間枠なしで判断に進みます。

アプリの段階。同じ crate が、アプリのチェーンの中で、同じポートをエージェントに提供します。応答のタスクが `UpdateTransport` からフレームを読み（ISO-TP の組み立てはプラグインの中）、`UpdateProtocol` が UDS のサービスをセッションのトリガーに変え、エージェントの返答が同じ道を戻ります。`accept_update` と `self_check` は、ボードの定義、ライブラリ、アプリのものです（ここでは `decides = []`）。レベル `mpu` では、ハンドラは CAN のウィンドウだけをペリフェラルとして非特権で動きます。レベル `none` では同じコードが動き、`dagon status` がそのノードを "extensions unguarded" と表示します。

プラグインが、どのレベルでも API を通してできないこと: スロットのストアやログに届くこと（ハンドルがありません）、スロットを確定すること（宣言する判断ではありません）、ブートローダが起動するものを変えること。`mpu`/`pms` で、コードが何をしてもできないこと: フラッシュに書くこと、ウォッチドッグに触れること、DAGON のメモリを読むこと、ブートをハングさせること。

### 8.5 デプロイの結果に対するホストのアクション

2.5 のような `~/.dagon/actions/on-result.toml` です。その後 `dagon apply` が、ノードごとに `host.node.result` を、プログラムが終わった後に `host.action.ran` を出します。同じイベントが `/v1/events` と、`--events` で stdout に現れます。

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
| DAGON が解釈する閉じた操作の集合だけをブート段階の拡張にする | 必要は限りがなく（UDS のような診断の転送、追加のセキュリティ処理、ディスプレイ、別のストレージ）、新しい必要のたびに DAGON の変更になります。コードの要らない用途のためのいちばん単純な拡張の種類として残し、一般の必要は、ハードウェアが強制するサンドボックスの中の、ポートの後ろのコードにします。 |
| lint、依存の許可リスト、適合のスキャンを防壁にした、ブート時の任意のコード | それらが調べるのはソースで、動くものではありません。コールバックは、ウォッチドッグがチップをリセットするまでループでき、どのレジスタやフラッシュにも書けます。防壁はマイコンの保護ユニットとプリエンプトするタイマ（4.1）で、どちらも無いマイコンでは、定義が `none` と言い、製品がそう示します。 |
| 拡張が答える、ブート時の拒否権 | `Boot` をロールバックに変える信頼されていない答えは、故障した拡張が有効なイメージをいつまでも動かさないことを許します。ブートの方針は DAGON の `DecisionPolicy` のポートで、置き換えるのは宣言され、署名され、表示される信頼された拡張（4.7）です。 |
| スロットのストアの変更メソッドを公開し、エージェントをその利用者の 1 つにする | 永続のイメージの状態が、イベントも確定のルールもなしに変わりえます。イメージのライフサイクルの状態機械が唯一の書き手です。 |
| 新しいタイミングごとにメソッドを足す固定のフックのトレイト | 合成がなく、ディスプレイのためのペイロードのストリームもなく、部分の置き換えもなく、予想しなかったタイミングのたびに一覧を直すことになります。廃止予定のファサードとしてだけ残します。 |
| 手で保守するカタログに照らす、手で保守するイベントの列挙型 | 2 つの一覧が一致しても、遷移が何も出さないことがありえます。イベントを出さずには状態を変えられない基本要素だけが、カタログを派生物にします。 |
| リンカのセクションの登録簿（`linkme`） | 項目の順が定まらず、ライブラリによる見えない登録があり、`dyn` のディスパッチのコストと大きさがかかります。 |
| デバイスの `alloc` を使う `dyn Extension` の一覧 | ホストより下に alloc はありません。静的なスライスは登録簿の問題を再び持ち込みます。 |
| すべてをチャネルにする | 判断とポートには同期の答えが要ります。キューが正しい道具なのは公開の状態だけです。 |
| 待てるリングに借用のペイロードを残す | 静的なリングは作る側より長く参照を持てません。生成される所有された短い形（2.4）は有界で、表示やログのタスクに十分です。 |
| デバイスからホストへの求められていないイベントのフレーム | リンクは、親機がホップごとに中継するリクエストとレスポンスです。だれも求めていないフレームには、経路も、再試行の連番も、リレーの無通信の時間のモデルの中の場所もありません。`EVENT` のフレームのフラグは予約で、使いません。ホストはデバイスのすべての遷移を、返答、VERSION、ブート記録から導きます。 |
| ビルド時の環境変数でのボードの値 | 定義に無い、形を変えたマイコンの知識です。ボードの定義の `[boot]` に置き換えました。 |
| アプリがコンパイル時に相手にしたブートローダのバージョンを報告する | インストールされているものを識別しません。VERSION はインストールされたイメージをフラッシュから読みます。 |
| 再起動の遅れの固定の上限 | 1 度決めた数は、試行のウォッチドッグの短いマイコンでは誤りです。上限はマイコンの定義の周期から導きます。 |
| CRC で覆ったヘッダをその場で書き換えるブート記録 | その場で書き換える回数、状態、CRC は、リセットに対してアトミックにできません。途中で切れた書き換えは、ヘッダが覆うすべてのイベントを無効にします。ほかの書き手のセクションをヘッダで探す引き渡しは、書き手の形式を結びつけます。 |
| 保持メモリと識別の形式を文章で定める | リセット、部分的な書き込み、バージョンのずれのあらゆる組み合わせに正しさが依存する形式を正しいと示すのは、文章ではなく、ランダムな故障の注入を持つ実行可能なモデルです（5.5）。 |
| CLI に組み込みの例の構成と `demo` コマンドを置く | ハードウェアなしで DAGON を試すのはシミュレータの役目です（`dagon sim` はどの構成もその定義で動かします）。ブラウザで試すページは、別に公開するページです。DAGON は自分のボードや構成を持ちません。 |

## 10. 大きさの上限とその出所

どの上限にも唯一の出所があり、それに収まるべき場所をすべて挙げ、テストが各組を確かめます。

| 上限 | 値 | 唯一の出所 | 収まるべきもの |
|---|---|---|---|
| 種類ごとのリンクのペイロード | USB 1088、UART 1024、I2C 256 バイト | `dagon_proto::max_payload` | すべてのリクエストと返答、`RECORD_PAGE`、VERSION のレスポンス、表示の文字列、適合試験の詳細 |
| いちばん小さいリンクのペイロード | `max_payload::MIN` = 上の種類の最小（導いたもので、書いたものではありません） | `dagon-proto` | VERSION のレスポンス全体（デバイスが答えるすべての TLV の集合。いちばん大きい 0x08 の集合、0x20、0x21 の全ページで確かめます） |
| TLV の値 | ≤ 255 バイト（`len u8`） | VERSION の符号化（`dagon-proto` のペイロード） | 5.4 のすべての id、0x21 のページ |
| VERSION 0x21 の固定の費用 | `VERSION_PAGE_OVERHEAD = 6` = ステータス (1) + TLV の数 (1) + id (1) + len (1) + offset (2) | 欄の幅の合計と同じことを `const` のアサーションで確かめる `dagon-proto` の定数 | `RECORD_PAGE` |
| リンクごとの `RECORD_PAGE` | `RECORD_PAGE(link) = min(255 − 2, max_payload(link) − VERSION_PAGE_OVERHEAD)`: I2C 250、UART 253、USB 253。`RECORD_PAGE_MIN = 250` | リンクの種類ごとに `const` のアサーションを持つ `dagon-proto` の `const fn` | VERSION 0x21。ホストのページの読み手が求めるのは最大 `RECORD_PAGE_MIN` |
| ブート記録の領域 | `bytes ≤ RECORD_MAX = 4096`（0x21 の `offset u16` が全体を指せます）。内部の上限はモデルの定数です（5.5） | `dagon_format::bootrecord::RECORD_MAX`、マイコンの定義の `[boot.record]` | 定義の検証、すべてのイメージの `memory.x`、エージェントの読み手 |
| サービスのプリエンプトできない部分 | サービスと種類ごとの `np_max_us`、`COPY_MAX`、各 `[[device]]` の `max_transfer`、`dma_stop_max_us` | ポートのカタログと種類の能力の記録（4.4）。実装の測定した最悪の場合に照らして確かめます | サービスの実装、タイマのマスク、4.2 の検証 5 |
| 拡張の持ち時間 | `max_run_us ≤ BOOT_EXT_MAX_US`、`≤ APP_EXT_MAX_US`。1 回のブートで `Σ (calls × budget)` ≤ `trial_ms / 4` | マイコンの定義の `[watchdog]` からの `dagon-events` の `const fn`、`boot_wiring.rs` | `dagon gen` の検証、サンドボックスのタイマ、マニフェスト |
| 拡張のメモリ | `stack_bytes + ram_bytes ≤` マイコンの定義の `[isolation] ext_ram_max`。エントリはコード、データ、スタックの後の種類の上限以下 | マイコンの定義、`boot_wiring.rs` の保護マップ | `dagon gen` の検証、`memory.x` |
| 拡張の数と id | id ≤ 32 バイト。0x08 は拡張ごとに繰り返し、VERSION の返答全体がいちばん小さいリンクに収まること | マニフェストの形式、`max_payload::MIN` | `dagon gen` は、VERSION の返答が収まらないプロジェクトを受け付けません |
| ブートのアクションプログラム | イベントごとに `ops ≤ 32`、`delay total ≤ 50 ms`、`bus_write` ごとに `bytes ≤ 8`、`delay ≤ 20 ms` | `dagon_format::bootprogram` の定数 | `dagon gen` の検証、ブートローダのインタプリタ、`boot_wiring.rs` の表の型 |
| live のイベントのペイロード | ブートのイベントは ≤ 16 バイト（値で）。より大きいアプリやホストのペイロードは参照で | `dagon-events` の大きさのアサーション | `Published` のスナップショット、短い形 |
| `EventRecord` | 24 バイト: `layer`、`kind`、`flags`、`dropped`、`seq u32`、`payload [u8; 16]` | `state_machine!` の `compact` 属性（生成されたテストが、すべての種類が持つことを確かめます） | `Recent<N>`。ブート記録の短い形は、ペイロードの最初の 3 バイトを再利用します |
| カタログ | 数値の上限なし。`kind` はレイヤーごとの `u8` | `state_machine!` | 記録の `kind u8`（生成されたテストが、レイヤーの 256 種類で失敗します）、serve のストリーム |
| 最近のリング | レイヤーの crateが選ぶ `N` | `Published<S, N>` | レイヤーの crateの `Published`。あふれると `dropped` を数えます |
| 判断の上限 | 50 ms、`REBOOT_DELAY_MAX = min(trial_ms, confirmed_ms) / 2` | `dagon-events` の定数、マイコンの定義の `[watchdog]` | 応答のタスクのタイマ、`boot_wiring.rs` |

## 11. バスマスタと特権の制御ブロック: 定義のスキーマ

すべてのマイコンの定義は、マイコンのすべてのレジスタブロック（`[[peripheral]]`、4.4）を `window_eligible`、`bus_master`、`privileged_why` とともに挙げます。検査器は、種類が `none` でなく、必要な区分のブロックを欠く定義や、バスマスタをウィンドウにできると印を付けた定義を受け付けません。生成されるマップ（4.4）と適合試験のケース（4.9）はこの一覧から派生し、下のものをほかの場所で手で保守することはありません。ここの 3 つの表は、配布する 3 つの定義の一覧で、スキーマが定義の作者に求める判断の例です。ブロックはメモリに書けるか、設計はそれをどう特権に保つか、です。

**STM32F401RE**

| ブロック | メモリに書くか | 特権に保つもの |
|---|---|---|
| DMA1、DMA2（`0x4002_6000`、`0x4002_6400`） | はい（バスマスタ） | `bus_master = true`、`privileged_why = "dma"`。ウィンドウには決してしません。拡張の DMA は、バウンスバッファを使う `DMA_*` のサービスを通すときだけです |
| USB OTG FS（`0x5000_0000`） | この部品ではシステム RAM への DMA はありません（FIFO ベース）が、DAGON のリンクです | `shared`。ウィンドウには決してしません。エージェントが持ちます |
| フラッシュインターフェース（`0x4002_3C00`） | いいえ（フラッシュに書き込みます） | `flash` |
| IWDG、WWDG | いいえ | `watchdog` |
| RCC、PWR、RTC/バックアップ | いいえ | `clock`、`reset`、`power` |
| SYSCFG、EXTI、NVIC/SCB（PPB） | いいえ | `irq-routing`、`pinmux`。PPB はアーキテクチャで特権です |
| CRC | いいえ | `shared`（DAGON が使います） |
| システムメモリ（ROM ローダ） | いいえ | `rom`: 非特権モードから実行できません（バックグラウンドのマップ） |
| ボードで DAGON が使わない USART/SPI/I2C/CAN/TIM/ADC/GPIO のブロック | いいえ | 自己完結しているところは `window_eligible = true`。GPIO はサービスです（DAGON の LED と共有のバンク） |

**RP2040**

| ブロック | メモリに書くか | 特権に保つもの |
|---|---|---|
| DMA（`0x5000_0000`） | はい | `dma`、バスマスタ。サービスだけ |
| USB（`0x5010_0000`） | 自分の DPRAM にだけで、システム RAM には書きません。DAGON のリンクです | `shared`。ウィンドウには決してしません |
| XIP の制御、SSI（`0x1400_0000`、`0x1800_0000`） | いいえ（SSI を通してフラッシュに書き込みます） | `flash` |
| ブート ROM（`0x0000_0000`） | そのフラッシュのルーチンが SSI と DMA を動かします | `rom`: 非特権モードからの ROM の実行は全体を拒否します。ルーチンのアドレスは防壁に関係ありません |
| WATCHDOG | いいえ | `watchdog` |
| CLOCKS、RESETS、XOSC、PLL、ROSC、VREG | いいえ | `clock`、`reset`、`power` |
| PSM、SIO の FIFO とスピンロック | コア 1 の起動とコア間 | `core-control`。2 つ目のコアは起動しません |
| SIO の GPIO の out/oe | いいえ | `shared`（DAGON の LED）。GPIO はサービスです |
| IO_BANK0、IO_QSPI、PADS | いいえ | `pinmux` |
| PIO0、PIO1 | いいえ（FIFO。PIO のプログラムはシステムメモリにアクセスできません） | ボードが DAGON のリンクに使わないときは `window_eligible = true`。それ以外は `shared` |
| UART0/1、SPI0/1、I2C0/1、PWM、ADC、TIMER | いいえ | DAGON のものでないところはウィンドウにできます。TIMER は `shared` です |
| コア 1 | コア 0 の MPU に制約されない 2 つ目の CPU | `[[core]]`。起動しません。その起動のブロックは特権です |

**ESP32-S3**

| ブロック | メモリに書くか | 特権に保つもの |
|---|---|---|
| GDMA | はい。SPI2/3、I2S、UHCI、LCD_CAM、ADC、AES、SHA の代わりに | `dma`、バスマスタ。GDMA のパーミッション制御がチャネルごとに World 1 を拒否します。サービスだけ |
| USB OTG | はい（自分の DMA） | `shared`、バスマスタ。ウィンドウには決してしません |
| USB Serial/JTAG | DMA なし。DAGON のリンクです | `shared` |
| SPI0/SPI1（キャッシュとフラッシュ）、MMU | いいえ（フラッシュに書き込み、メモリを割り当てます） | `flash`、`protection` |
| AES、SHA、RSA、HMAC、Digital Signature のエンジン | GDMA を通して（AES/SHA） | `shared`/バスマスタ。`CryptoProvider` は信頼されたポートです（2.1） |
| RTC WDT、TIMG WDT、スーパーウォッチドッグ | いいえ | `watchdog` |
| RTC_CNTL、システムレジスタ、割り込みマトリクス、IO MUX、GPIO マトリクス | いいえ | `power`、`reset`、`clock`、`irq-routing`、`pinmux` |
| PMS と WCL のレジスタ | いいえ | `protection`。World 1 の前にロックします |
| Wi-Fi と Bluetooth の MAC | はい（バスマスタ） | バスマスタ。DAGON は有効にしません。特権です |
| コア 1 | 2 つ目の CPU | `[[core]]`。マップは両コアに適用します |
| DAGON が使わない UART1/2、DMA なしの SPI2/3、I2C、TWAI、PWM、RMT、PCNT | いいえ | 自己完結しているところはウィンドウにできます。TWAI（CAN）は UDS の転送の自然なウィンドウです |

## 12. 実行モデル

DAGON 自身のコードで何がどこで動くか、DAGON のコードが割り込みをマスクしてよい長さ、フラッシュの操作が何を止めるかを、マイコンごとに示します。この節は規範です。マイコンの定義のコードはこれに従って書かれ、マイコンの定義はそのハードウェアの事実（`[execution]`、`[watchdog]`）を持ち、下に挙げる確認がコードとデータをこれに従わせます。

### 12.1 ルール

1. **仕事はハンドラではなくタスクで。** DAGON の応答のループ、リレー、ウォッチドッグへのキックは、エグゼキュータの上のタスクとして動きます。DAGON 自身の割り込みハンドラは、タスクを起こすかバイトを動かすだけです（リンクのドライバ、エグゼキュータ自身の割り込み）。待つもの、消去するもの、フラッシュに書くものが、ハンドラの中で動くことはありません。
2. **マスク時間の上限。** DAGON 自身のコードのクリティカルセクション（ウォッチドッグのガードの状態、ロックの状態）が割り込みをマスクするのは、最大でマイコンの定義の `[execution] masked_max_us` です。フラッシュの消去や書き込み、ほかのマイクロ秒の上限の無い操作を、その中で動かすことはありません。複数の利用者を持つフラッシュドライバは、操作のためにマスクせずに*取る*ロックで共有します（`dagon_mcu_common::shared::SharedFlash`、`try_lock`。取られていると気づいたハンドルはすぐにエラーを返し、待ちません）。clippy が強制します。`targets/mcu/clippy.toml` は、マイコンの定義とブートイメージ（stage0、ブートローダ）の中のすべてのマスクの基本要素（`critical_section::with` と `acquire`、`cortex_m::interrupt::free` と `disable`、クリティカルセクションの `blocking_mutex::Mutex::lock`）を、解決したパスで拒むので、別名やラッパーでは隠せません。マスクする関数（ウォッチドッグのガードの状態の変更、リレーの共有の表示の状態）は、理由とともに `#[allow(clippy::disallowed_methods)]` を持ち、テストが一覧と例外をレビューされたとおりに保ちます。どのリクエストも、応答のループの `ServeHost::feed`（RP2040 のリレーの PC のループでは、ガードの `progress`）を通した、条件付きのキックで始まります。
3. **フラッシュの操作はハードウェアの事実です。** 消去や書き込みが何を止めるかは、定義の `[execution] flash_op` です。
   - `fetch-stall`: コードが動くシングルバンクのフラッシュ。命令の取得とフラッシュの読み出しは、操作が終わるまで止まります。割り込みは許可のまま待ち、何も動きません（DAGON は RAM にハンドラを置きません）。
   - `xip-off-masked`: 操作は execute-in-place を切って RAM から動きます。ハードウェアの求めるとおり、フラッシュドライバはその 1 回の操作のあいだ割り込みをマスクします（そしてもう一方のコアを止めます）。
   - `cache-off-masked`: 操作のあいだフラッシュのキャッシュは切れています。フラッシュドライバはその 1 回の操作のあいだ割り込みをマスクします（内部 RAM の外のハンドラは動けません）。

   どの場合も、DAGON は 1 回の操作で最大 1 つの消去単位（`[execution]
   erase_unit`）を発行し、かかりうる時間はデータシートの最大値 `erase_unit_max_ms` です。
4. **ウォッチドッグは 1 回の消去より長く持ちます。** `erase_unit_max_ms` は `[watchdog] confirmed_ms` より小さいです（定義の読み込み時に確かめます）。どのリクエストも条件付きのキックで始まり、長い消去は消去単位に分け、あいだに条件付きのキックを挟みます。待機中のイメージの試行は独立した期間です。そのあいだキックするかどうかは、ガードの試行の方針が決めます（`app.watchdog_taken` が報告します）。
5. **止まっているあいだのリンク。** リクエストは、新しいリクエストにリンクを閉じて処理します（I2C のターゲットは自分のアドレスに NACK し、ストリームのリンクの相手は返答を待ちます）。CPU が止まっているあいだに届いたものは、失われるかハードウェアが保持します。要求側は同じ連番で再試行し、ノードは繰り返しのリクエストに、すでに作った返答で答えます。
6. **リセットは返答が出た後でだけ。** 再起動は、返答を書いてフラッシュするまで待ち、その後で `allow_reboot_now` が求めた遅れのあいだ、応答のタスクをアイドルにして待ちます（ガードはキックを続けます）。送れなかった返答では何もリセットせず、要求側の再試行は同じ返答を得ます。
7. **事実は、どこまで確かめたかを示します。** 定義のハードウェアの事実は、出所に照らして確かめた数値（定義のコメントが文書と表を示します）、`{ unverified = N }`（まだ出所に照らしていない値）、または `"unmeasured"`（まだ値が無い）です。ツールと文書は後の 2 つを確かめていないと報告します。値の要る確認は unverified の値を使います。リリースがどれかに頼ることはありません。

### 12.2 マイコンごと

表はマイコンの定義のコードがすることで、上限はそれぞれのマイコンの定義のものです。

**STM32F401RE**（`flash_op = "fetch-stall"`、`erase_unit` はセクタ 1 つ）

| 仕事 | 動く場所 | 優先度 |
|---|---|---|
| I2C のターゲットのリンクの応答（エージェント、CONF_RUN）、フラッシュの消去と書き込み | DAGON の割り込みエグゼキュータ（USART6 のベクタ） | P6、thread モードより上 |
| リンクの I2C と DMA の転送 | I2C ドライバの割り込み | HAL の既定 |
| タイマ（`embassy-time`） | そのタイマの割り込み | HAL の既定 |
| ウォッチドッグへのキック | アプリのキックのタスク、ガードが条件を確かめます | thread モード |
| サンドボックスのタイマ（4.3） | SysTick | どの能力の割り込みよりも上 |

消去のあいだ、セクタが終わるまで何も実行しません。IWDG はリクエストのキックから数えます。自分の I2C アドレスはリクエスト全体のあいだ NACK するので、親のポーリングは CPU なしでペリフェラルが NACK し、親は自分の期限まで再試行します。

**RP2040**（`flash_op = "xip-off-masked"`、`erase_unit` はセクタ 1 つ）

| 仕事 | 動く場所 | 優先度 |
|---|---|---|
| PC へのリンクとエージェント、子機へのリンク（リレー） | DAGON の割り込みエグゼキュータ（SWI_IRQ_0） | P1 |
| USB デバイス | USB の割り込み | HAL の既定 |
| フラッシュの消去と書き込み | PC のタスク、スロットのストアの `SharedFlash` のハンドルを通して | 操作ごとにセクタかページ 1 つ、ドライバがマスク |
| ウォッチドッグへのキック | キックのタスク、ガードが条件を確かめます | thread モード |

フラッシュの操作はそれぞれ、セクタの消去 1 回かページの書き込み 1 回のあいだ割り込みをマスクします。スロットのストアは、セクタのあいだにガードに進み具合を報告します。そのあいだ USB コントローラはホストに NAK し、ホストは再試行します。構成のコピーは、書くときと同じフラッシュドライバを通して読みます。

**ESP32-S3**（`flash_op = "cache-off-masked"`、`erase_unit` はブロック 1 つ）

| 仕事 | 動く場所 | 優先度 |
|---|---|---|
| UART と USB Serial/JTAG の上りのリンク、エージェント、フラッシュの消去と書き込み | DAGON の割り込みエグゼキュータ（FROM_CPU_INTR1） | Priority2、thread モードより上 |
| ウォッチドッグへのキック（確定の後、動作の確認を条件に） | リンクのタスク | Priority2 |
| アプリのタスク | thread モード | いちばん低い |

フラッシュの操作はそれぞれ、ブロックの消去 1 回のあいだ割り込みをマスクします。スロットのストアは、ブロックのあいだにウォッチドッグをキックします。UART の受信 FIFO は収まる分を保持します。止まったことで切れたフレームは 20 ms の間隔のルールで捨て、要求側が再試行します。そのあいだ USB Serial/JTAG はホストに NAK します。

### 12.3 実機での測定

上の上限は、どれも測定で確かめたものではありません。マイコンごとに、リリースが頼るのは、ボードで次のものを測ってからです。DAGON 自身のコードのいちばん長いクリティカルセクション（`masked_max_us`: 測定した最悪に余裕を足したもの）、ボードのフラッシュの部品の最悪の消去時間（と、定義が示すそのデータシートの値）、1 回の消去の終わりに残るウォッチドッグの余裕、その後のリンクの回復（親の再試行に答えること）。
