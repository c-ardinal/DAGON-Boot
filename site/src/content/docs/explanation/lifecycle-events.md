---
title: "ライフサイクルイベントと拡張"
description: "ブートローダ、エージェント、リレー、ホストに共通の1つのモデル: 状態機械、イベント、ポート、保護です。"
---

この文書はDAGONのライフサイクルイベントと拡張のモデルの設計です。実装と、DAGONを拡張する全利用者のための参照資料です。以下の事項を提示します。

- 全レイヤーの全状態遷移を、観測可能なイベントにする方法
- ボードの定義、ライブラリ、アプリケーションがDAGONを拡張する方法。ブート段階を包含する全レイヤーで、イベントを観測し、少数の判断点に応答し、ポート経由でDAGONの一部を置換します。
- マイコンの保護ハードウェアが、書込の安全性を支える要素に拡張をアクセスさせない方法
- ボードの値とブート時のアクションを定義から取得する方法
- モデルのバージョン管理の方法

目標はDAGONと利用者のコードの完全な分離です。利用者（ボードの定義、ライブラリ、アプリケーション）は、DAGONの公開API、イベント、拡張のポート、定義で、必要な処理を全部実現します。DAGONの内部の編集、コピー、フォークはしません。DAGONはどのサンプル、ボード、ペリフェラルについての知識も保持しません。このモデルは固定の一覧ではなく、事前に不明なニーズ（車載の診断の転送、追加のセキュリティ処理、ディスプレイ、別のストレージ）のための設計です。

この文書で使用する用語です。

- *ブートローダ*: DAGONがアプリより先行して実行するイメージ（self-bootのマイコンではstage0とブートローダ。vendor-stage2のマイコンではベンダーの2nd stage）。
- *stage0*: リセット後に最初に動作する小規模なプログラム。起動するブートローダのスロットを選択し、起動可能なブートローダがない場合は確定済のアプリを起動します。
- *エージェント*: アプリ内の`dagon-agent`。
- *リレー*: コーディネータのPC側のリンクと、接続先のノードへのリンク。
- *ホスト*: CLIと`dagon serve`が実行する`dagon-host`。
- *拡張*: DAGONの外部にあり、DAGONがいずれかの拡張点で実行するコードかデータ。
- コーディネータ/ノード: 英語のhub/nodeに該当します。
- *キック*: ウォッチドッグのリセット（カウンタをリセットしてタイムアウトを防止すること）。
- *ウォッチドッグによるリセット*: タイムアウト時にウォッチドッグが実行するチップのリセット。

## 1. 状態機械とそのイベント

DAGONの状態変化は、例外なく宣言済の状態機械の遷移を経由して発生し、各遷移はイベントを発行します。状態機械は`state_machine!`でそれぞれ1回だけ宣言します（1.7）。規範となるカタログは生成物です。[イベント](/reference/events/)は状態機械の`CATALOG`の定数（種類、ペイロード、属性、観測の経路）から作成し、テストでそれらと照合して検証します。この節のイベントの表は、状態機械が作成する内容の例です。生成されたカタログにない項目は保持せず、両者が相違する場合は生成されたカタログが正当です。1.8は同一の留保のもとで、イベントの区分ごとの観測方法を提示します。

### 1.1 イメージのライフサイクル（スロットごと、永続）

イメージのスロットの状態は、リセットをまたいでスロットのストアに残存します（kind S: 状態ログ。kind V: otadataのようなベンダーの選択データ）。2個のレイヤーに遷移を保持する1個の状態機械です。エージェントはリンク経由で変更し、ブートローダはブート時に変更します。この状態機械の外部で永続のイメージの状態を変更する公開の経路はありません。

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

| 遷移 | レイヤー | ガード（拒否時のステータス） | 永続の形式 | イベント |
|---|---|---|---|---|
| `erase` | app | XFER_BEGINが全ガードと`accept_update`に合格。`Empty`、`Written`、`Pending`、`Trial`、`Confirmed`、`Reverted`から。書込側（セッションの型の`Writer`: アプリかブートローダ）が稼働中のスロットからは不可で、アプリでは確定済のスロットからも不可です。`Pending`からの遷移があるのは、待機にしたが未起動のイメージを、プロトコルが新規イメージで置換可能だからです。消去したヘッダはSLOT_PENDINGの記録と一致しなくなるので、そのスロットは空白と判定されます | スロットの本体とヘッダの領域を消去 | `app.transfer_started` |
| `written` | app | XFER_END: 本体のダイジェストがヘッダの値と同一で、ヘッダを書込して再検証した | ヘッダが所定の位置にある | `app.written { slot, version }` |
| `pending_set` | app | SET_PENDING: ヘッダがあり、署名と互換性、本体のダイジェストが正当。稼働中のスロットは不可。`Written`、`Reverted`、`Pending`（SET_PENDINGを反復すると、再度追記します）、`Confirmed`（他方のスロットの試行中に、確定済のスロットを再度待機にして、そのスロットへ復帰します）から | SLOT_PENDINGの記録/ ベンダーの選択 | `app.pending_set { slot, version }` |
| `pending_moved` | app | このスロットが`Pending`か`Trial`の場合に、他方のスロットへのSET_PENDINGがあり、このスロットが確定済ではない | 同一のSLOT_PENDINGの記録（ログは待機のスロットを1個指定します） | `app.pending_moved { from, to }` |
| `pending_withdrawn` | app | このスロットが確定済で、再度待機にされ、別のスロットが待機になったか、その試行がロールバックされた。ログの`confirmed`がこのスロットの指定を継続するので、`Confirmed`に復帰します | 同一のSLOT_PENDINGかROLLBACKの記録 | `app.pending_withdrawn { slot, version }` |
| `attempt` | boot | 待機のスロットを検査した（署名、ダイジェスト）、`attempts < max` | BOOT_ATTEMPTの記録/ ベンダーの"new" → "pending verify" | `boot.attempt_recorded { slot, attempt, max }` |
| `confirmed` | app | 試行中のCONFIRM、`self_check`が健全 | CONFIRMEDの記録/ ベンダーの"valid" | `app.confirmed { slot, version }` |
| `demoted` | app | 他方のスロットの`confirmed`（ログの`confirmed`は他方のスロットを指定します） | 同一のCONFIRMEDの記録 | `app.demoted { slot, version }` |
| `rolled_back` | boot | 試行回数が上限に到達した、または待機のイメージが検査に失敗した | ROLLBACKの記録/ ベンダーの中止 | `boot.rollback { from, to, reason }` |
| `chosen` | boot | 起動するスロットが決定し（確定か、試行）、その署名とダイジェストを検証した | なし | `boot.slot_chosen { slot, trial, version }` |
| `no_image` | boot | 起動可能なスロットがない | なし | `boot.no_image { reason }` |

`erase`と`pending_set`に追加した遷移元の状態と`pending_withdrawn`が存在するのは、プロトコルがそれらのリクエストを従来から許可しているからです。状態機械はリンク上の動作を制限せず、その動作に従います。

1個の永続の記録が2個のスロットを遷移させる場合があります（`confirmed`と`demoted`、`pending_set`と`pending_moved`か`pending_withdrawn`）。状態機械はスロットごとに1個の状態を保持し、1個のトリガーが両方の遷移を実行し、両方のイベントをこの順に発行します。テストはログから畳込したストア（`LogState`）の上でエージェントの`ImageStore`を、準備済のデバイスから、エージェントとブートローダが書込する操作（転送、切替、確定、ブートの試行、ロールバック）のあらゆる順序で実行し、各ステップの後に全スロットの状態機械の状態がSTATUSと同一であることを検証します。そのため、遷移のない畳込の結果は存在せず、エージェントは2個目のスロットに関する遷移も実行します。

状態機械はスロットのストアのポートをラップします。`ImageStore<S: SlotStore>`がストアを保持し、ポートの変更メソッド（`erase`、`write`、`write_header`、`set_pending`、`confirm`）は、状態機械だけが作成する`Transition`を、その変更のイベントを発行する遷移内で受信します。`SlotStore`のトレイトは、ボードが実装するポートのままです。エージェントがストアを保持し、誰にも受渡しません。ボードのその他の資源（フラッシュの読込側、ウォッチドッグ）は、マイコン定義のコードが自身で保持するハンドルです。それらのハンドルの到達先（未加工のフラッシュドライバ、シミュレータの準備用の補助）は、この保証の対象外です。同一イメージでフラッシュにアクセスするその他のコードと同様に、ボードやテスト環境の信頼されたコードであり、拡張からはサンドボックス（4）が保護します。ヘッダの書込が再読出不可能な場合は、スロットを再度消去します。そのため、状態機械が未記録のイメージを故障が残存させることはありません。エージェントは`images() -> &ImageView`を公開します（読取専用: ステータス、各スロットのライフサイクルの状態、自身のバージョン、試行中かどうか）。起動時に自身で確定するアプリ（テスト環境用）は、起動にその動作を要求します（`App { confirm: Confirm::Own, .. }`）。起動はボードの起動処理の後に`confirm_own()`を呼出します。これが`confirmed`の遷移です。最初に`self_check`の判断、次にポートの確定を実行し、イベントは必ず発行します。ブートのレイヤーの遷移は、アプリのステージの観測者も観測可能なように、アプリが起動時にリプレイします（1.2、5.5）（`app.trial_started`、`app.rolled_back`）。

### 1.2 ブートの手順

self-bootのマイコン（stage0とブートローダ）の状態は`Entry → HalReady →
WatchdogArmed → LogRead → BeforeDecision → Planned → Committed →
Jumping | Halted`です。`plan`は純粋です（フラッシュを読込し、判断と追記する記録を計算します）。`commit`がイメージのライフサイクルの遷移`attempt`、`rolled_back`、`chosen`、`no_image`を実行します。stage0が保持する状態は`Entry`、`CoreChosen`、`Halted`だけです。状態ログのブートローダの記録と試行のセルから、ブートローダのスロットを選択します（`dagon_core::bootcore::start`、retained-formats.md 7）。計数した起動が`trial_attempts`未満の間は待機のコアを、それ以外は確定済のコアを選択します。コアを起動するのは記録が指定する場合だけで、記録のないコアも、起動回数が上限に到達した待機のコアも起動しません。`core_slot_chosen`に先行して、コアのヘッダの署名が検証可能であること、ヘッダが記録の指定どおりであること、本体がヘッダのダイジェストと一致することを検証し（4.7）、試行のコアの起動前にウォッチドッグを開始し、拡張は実行しません。stage0はフラッシュに書込しません。起動可能なコアがない場合（ブートローダのスロットが1個のマイコンで、新規コアの書込が電断で途切れたか、新規コアが試行に不合格の場合）は、状態ログにある確定済APPの記録が指定するアプリを起動します。その起動前に、記録のヘッダのダイジェスト、署名（4.7）、イメージの種類、マイコン、ターゲットのタグ、スロットのバインディング、配置とロードの範囲、本体のダイジェスト、エントリポイントが本体内にあることを検証します。APPの試行がオープン中の間は起動せず、待機のアプリも起動しません。試行のセルは無変更にし、何も記録しません。そのアプリがブートローダを再度書込します。そのようなアプリも不在の場合だけ、何も起動せずに復旧を待機します（4.7）。起動したブートローダは、自身のブート判断の実行前にブートローダの試行を決着させます（`settle_log`）。待機のコアは自身の検査（起動可能なアプリがあり、そのアプリのヘッダがコアのキーで検証可能）の合格後にだけCONFIRMED（ターゲットBOOTCORE）を追記し、それ以外はリセットします。別のコアが待機中の場合に起動した旧コアは、ROLLBACKを追記します。ウォッチドッグの規則は4.1に記載しています。

ブートローダのイメージのライフサイクル（エージェントの`core_image`の状態機械）は、1.1のアプリイメージのライフサイクルに、ブートローダの経路が保持する遷移で従います。エージェントの`erase`、`written`、`pending_set`（ターゲットBOOTCOREのXFERとSET_PENDINGで、未使用のブートローダのスロットへ遷移。稼働中のコアや確定済のコアのスロットは不可。マイコン定義がブートローダのスロットを1個だけ付与する場合はその1個へ書込し、アプリの試行中はその転送と待機の設定を拒否します。ホストは、最初にアプリを確定またはロールバックするよう要求します）、新規コア自身のCONFIRMEDによる`confirmed`と`demoted`、旧コアのROLLBACKによる`rolled_back`（アプリの起動時にリプレイします）です。アプリから観測可能な`Trial`の状態はありません。アプリの動作開始前に必ず、コアが試行を決着させるからです。ターゲットのバイトが`BOOTCORE`のSTATUSは、ブートローダのスロット（バージョン、ヘッダのダイジェスト、稼働中のコアと確定済のコア）を報告します。マイコン定義がブートローダのスロットを宣言しないボードは、`Unsupported`を返却します。ブートローダのスロットの数と配置は、マイコン定義の`[[slot]] image = "bootcore"`の項目に由来します（1個か2個です。スロット1個で不可能な動作は[制限事項](/explanation/limitations/)に記載しています）。

書込モードと復旧モードについて説明します。ブートローダも受信します。アプリと同一の書込のセッション（1.3）を、セッションのブートローダの書込側として、書込モードか復旧モードの場合だけ実行し、HELLOに`mode`の1（書込）か2（復旧）で応答します。アプリは0、復旧用エージェントは3で応答します。待受時間内に到達したSTAY（0x05）か、UPDATE_DONEが未消費のUPDATE_STAYの記録で、書込モードに移行します。有効なログの面がないか、起動可能なアプリがない場合は、復旧モードに移行します。待受時間はブートローダの起動から計測し、時間長はマイコン定義の`[boot_receive] listen_window_ms`です（0から200ミリ秒。省略時は50ミリ秒、0は待受時間なし。プロジェクトは`[mcu_override]`で変更します）。`[links]`の種類のうち、`[boot_receive] kinds`が参加対象とした種類のリンクで待受時間を設けます。対向側が最初に列挙を実施する種類（USB）は対象外で、ブートローダはその種類では書込モードか復旧モードの場合だけ受信します。ブートローダは定義が付与する全リンクで受信し、リンク間に優劣はありません。アプリはREBOOTのmode 1で引渡します。アプリは状態ログに、試行の分とUPDATE_STAY（0x06）とUPDATE_DONE（0x09）の領域を確保し（`Log::request_stay`。アプリの稼働中にコンパクションを実行します）、UPDATE_STAYを追記した後にリセットします。ブートローダは書込モードへの移行時に、その領域へUPDATE_DONEを追記します（`Log::take_stay`。消去はしません）。そのため、UPDATE_STAYは1回だけ使用されます。コンパクションは、未消費のUPDATE_STAYを残存させます。書込モードのブートローダは、書込が指定するスロットだけを消去して書込し、書込開始前に共通の確保（`Log::reserve`。他方の面にコンパクションを実行し、その面のヘッダを最後に書込するので、唯一の有効な面を消去することはありません）でログの領域を確保し、REBOOTの直前にログを消去することはありません。STAYとREBOOTのmode 1では書込モードに滞留し、REBOOTのmode 0で退出します。STAYかUPDATE_STAYで移行した場合は、要求がないまま60秒経過しても退出します。復旧モードには起動するアプリがないので、ブートローダはそのモードに滞留します。アプリの稼働中はSTAYを拒否します（`E_STATE`）。書込モード以外では、ブート段階は消去しません。ブートローダのセッションが書込するのは、マイコン定義の`[writers]`がブートローダに列挙する種類（アプリ）だけです。その他の種類の転送は、何も消去しない段階で`E_UNSUPPORTED`で拒否します。これらの記録以前の読込側はこれらをスキップし、STAY以前のエージェントは`E_UNSUPPORTED`で応答するので、`wire`と`state-log`の番号は変化しません。ブートローダは中継しません（[制限事項](/explanation/limitations/)）。

ブート段階の拡張（2）とブートのアクションプログラム（5.2）は、`WatchdogArmed`以降でだけ動作します。それ以前のイベントは例外なく記録してリプレイしますが、実行中の観測者はいません。そのため、ウォッチドッグによるブートの保護開始前に、DAGONの外部のコードが動作することはありません。ブートのイベントは例外なく、フラッシュの書込を伴わない保持メモリであるブート記録（5.5）にも追記され、アプリのエージェントが起動時に同一の`boot.*`のイベントとして`replayed: true`を付与してリプレイします。この記録が`plan`、`commit`、`self_check`、`confirm`の入力になることはありません。

| イベント | 時点 | ペイロード | ブート時にlive |
|---|---|---|---|
| `boot.entry` | リセット後に実行するDAGONコードの最初の命令、HALは未初期化 | `stage`、`reset_cause` | いいえ（記録のみ） |
| `boot.hal_ready` | HALとクロックの初期化後 | `stage` | いいえ |
| `boot.watchdog_armed` | ウォッチドッグを開始した（4.1）。この時点以降、拡張が動作します | `period_ms` | はい |
| `boot.log_read` | 状態ログを読込した | `ok`、`pending`、`confirmed`、`attempts` | はい |
| `boot.before_decision` | `plan`の実行直前。判断前の最後のイベント | `pending`、`confirmed`、`attempts` | はい |
| `boot.decision_planned` | 判断が決定し、何も書込していない | `Decision` | はい |
| `boot.core_slot_chosen` | stage0がブートローダのスロットを選択した | `slot`、`version` | いいえ |
| `boot.slot_chosen` | イメージのライフサイクルの`chosen` | `slot`、`trial`、`version`、`entry` | はい |
| `boot.attempt_recorded`、`boot.rollback`、`boot.no_image` | イメージのライフサイクル（1.1） | | はい |
| `boot.before_jump` | 制御がDAGONから離脱する前の最後のイベント | `slot`、`trial` | はい |
| `boot.recovery` | アプリの代替として復旧の経路に移行 | `path` | はい |
| `boot.halt` | コアが完全に停止 | `code` | はい（ブートローダ） |
| `boot.extension_ran` / `boot.extension_faulted` / `boot.extension_overran` | 拡張のハンドラが復帰した/ フォールトした（保護、バス、命令、不正なサービス）/ 時間を超過した | `id`、`us` / `id`、`fault`、`pc?`、`addr?` / `id`、`budget_us` | はい |
| `boot.extension_disabled` / `boot.extension_irq_storm` | 拡張をブート終了まで無効にした（フォールトの上限か、前回ブートのロックアップヒューズ）/ 宣言済のペリフェラルの割込元を無効にした | `id`、`why` / `id`、`peripheral` | はい |
| `boot.program_step` / `boot.program_skipped` | ブートのアクションを1個実行した/ プログラムを実行しなかった | `index`、`op`、`ok` / `why` | はい |
| `boot.step` | 診断のステップに到達した | `n` | いいえ |

vendor-stage2のマイコンにはDAGONのブート段階がありません。アプリが起動時に選択データから`boot.slot_chosen`、`boot.rollback`、`boot.recovery`を`replayed:
true`で発行し、ブート段階の拡張はそこには存在しません（4.5）。

### 1.3 エージェントのセッション

状態は`Idle`、`Header { slot, body_len, got }`、`Body { target, slot,
len, next }`です。プロトコルのリクエストは例外なく遷移です（HELLO、STATUS、VERSION、進捗は自己遷移）。プロトコル自体もポートです（2.1）。状態機械のトリガーはプロトコルに依存せず（`update_offered`、`header_chunk`、`data_chunk`、`transfer_end`、`switch`、`reboot`、`confirm`、`status`）、プロトコルの拡張（DAGONのリンク、UDSなど）がメッセージをトリガーに対応付けます。

生成される起動時の順序: `app.starting` → `app.store_opened`（または`app.store_failed`）→ ブートのリプレイ（`replayed: true`の`boot.*`）→ `app.boot_replayed`（または`app.boot_record_missing`）→ `app.trial_started` / `app.rolled_back` → `app.watchdog_taken` → `app.bring_up` → `app.link_up` …

| イベント | 時点 | ペイロード |
|---|---|---|
| `app.starting` | DAGONの起動関数に進入した | `tag`（名称のダイジェストとして、2.4）、`role`、`mode` |
| `app.store_opened` / `app.store_failed` | 状態ログや選択データを読込した/ 読込不可能 | `active`、`trial`、`version`、`attempts`、`last_reason` / `error` |
| `app.boot_replayed` / `app.boot_record_missing` | ブート記録の有効なイベントをリプレイした（検出したブートごとに要約1個、時系列順）/ 有効なイベントが皆無（5.5） | ブートの識別、`writers`、`entries`、`invalid_entries` / `why` |
| `app.trial_started` / `app.rolled_back` | 状態ログからリプレイした | `slot`、`version`、`attempt`、`max` / `from`、`to`、`reason` |
| `app.watchdog_taken` | ブートローダのウォッチドッグを継承した | `period_ms`、`trial_policy` |
| `app.bring_up` | ボードの起動処理を実行した | `board_id`（名称のダイジェストとして、2.4） |
| `app.extension_started` / `app.extension_faulted` / `app.extension_overran` | 拡張のハンドラが動作した/ フォールトした/ 超過した（4.2） | `id`、`point` / `id`、`point` / `id`、`point`、`us` |
| `app.link_up` / `app.link_down` | DAGONのリンクがフレームを送受信する（起動時か、切断後の最初のフレーム）/ 停止した（返答の送信不可能、またはリンクがクローズ） | `link`、`addr` |
| `app.request` / `app.replied` | リクエストが到達した（どのプロトコルでも）/ その返答を出力した | `protocol`、`cmd`、`seq`、`via` / `status`、`ms` |
| `app.hello` / `app.status_asked` / `app.version_asked` | 識別/ ステータス/ VERSIONに応答した | `via` |
| `app.update_offered` | 書込のリクエストがDAGONの全ガードに合格した。**判断**`accept_update` | `target`、`slot`、`body_len` |
| `app.update_refused` | 拒否した | `status`、`by: Dagon \| Extension` |
| `app.header_verified` / `app.header_rejected` | ヘッダの署名と互換性（検証器のポート経由） | `slot`、`version`、`key_id` / `status` |
| `app.transfer_started`、`app.transfer_progress`、`app.transfer_aborted` | 消去が終了した/ 各チャンクを書込した/ 無通信のタイムアウト、ストアのエラー、新規リクエスト | `target`、`slot`、`len` / `done`、`total` / `cause` |
| `app.verify_started`、`app.verify_ok`、`app.verify_failed` | 本体のハッシュと再読出 | `len` / `slot`、`version` / `status` |
| `app.written`、`app.pending_set`、`app.pending_moved`、`app.pending_refused`、`app.confirmed`、`app.demoted`、`app.confirm_refused` | イメージのライフサイクル（1.1） | |
| `app.percent_told` | ホストが、書込の進捗を表す1個の数値を通知した（DAGON Consoleの%と同一。転送中、ノードが受信可能な場合だけ。表示のタスクは`AppSnapshot::percent`で読込します） | `percent` |
| `app.reboot_requested` | 再起動を受理した。**判断**`allow_reboot_now` | `mode` |
| `app.before_reboot` | 返答を出力した。その後にリセットします | |
| `app.self_check` | 試行中の確定。**判断**`self_check` | `slot`、`version` |
| `app.rom_entry` | 復旧への進入を受理した | `method` |
| `app.conformance` | 適合試験に応答した | `test` |
| `app.liveness_overdue` | タスクの遅延でガードがキックを中止した | `task` |

### 1.4 リレー

リレーもアプリです（自身のエージェントが`app.*`を発行します）。リレー自身の状態機械が保持するのは、PCへのリンクの接続/切断、中継したリクエスト後の無通信の時間です。接続先のノードへのリンクはポートです（接続先のノード向けのプロトコル拡張は、DAGONのフレームだけでなくUDSも中継可能です）。

| イベント | 時点 | ペイロード |
|---|---|---|
| `relay.pc_up` / `relay.pc_down` / `relay.pc_quiet` | PCへのリンクをオープンした/ クローズした/ 無通信の時間が終了した | |
| `relay.relay_request` | 中継のリクエストを受信した | `link`、`addr`、`cmd` |
| `relay.child_control` | 接続先のノードの制御線を操作した/ 未加工のバスアクセス | `line`、`level` / `addr`、`len` |

### 1.5 ホストの操作

操作（`apply`、`rollback`、`recover`、`confirm`、`provision`、`build`）ごとに1個の状態機械があり、ノードごとの副状態を保持します。その他にプロジェクトのセッションとエージェントとの接続があります。ホストの転送、署名、ビルドの実行はポートです（2.1）。

| イベント | 時点 | ペイロード |
|---|---|---|
| `host.build.started` / `host.build.finished` | ノードとスロットごと | `node`、`slot`、`ok`、`elapsed_ms`、`image_sha256` |
| `host.release.prepared` | バンドルを作成した | `version`、`nodes[]`、`path` |
| `host.deploy.planned` | 一覧を表示した。**判断**`confirm_plan` | 一覧 |
| `host.deploy.started` / `host.deploy.finished` | | `release`、`nodes[]` / `ok`、`results[]` |
| `host.node.discovered` | 検出 | `target`、識別 |
| `host.node.step` | ノードの各段階（TRANSFER、VERIFY、REBOOT、CONFIRM、REJECTED、FAILED、ROM） | `node`、`target`、`phase`、`percent`、`version` |
| `host.node.reconnecting` | 再起動後のバージョンを待機中 | `node`、`elapsed_ms`、`other_version?` |
| `host.node.result` | | `node`、`Done(v) \| Failed(reason, status?) \| NotStarted` |
| `host.confirm.result` | | `Confirmed \| NotOnTrial \| NeedsChoice` |
| `host.rollback.*`、`host.recover.*` | deployと同一の形式 | |
| `host.provision.started` / `host.provision.recorded` | ROM経由の初回インストール。インストールしたブートイメージの識別を記録した | `node`、`boot_sha256` |
| `host.project.opened` / `host.project.closed` | プロジェクトのセッション | `path` |
| `host.agent.linked` / `host.agent.unlinked` | 同意を記録した/ 取消した | `agent`、`way` |
| `host.version.read` | VERSIONに応答した（隔離のレベルと、ノードが報告する信頼された拡張の情報付き）、またはUnsupported | `target`、`VersionResp?` |
| `host.extension.loaded` / `host.extension.rejected` | ホストの拡張（データで定義、またはcrate）を読込した/ 拒否した | `id`、`kind` / `why` |
| `host.job.*`、`host.device.*` | serveの作業とデバイスのメッセージ | |
| `host.action.ran` | データで定義したアクションが終了した | `id`、`ok`、`elapsed_ms` |

### 1.6 永続の状態とその状態機械

リセットや再起動をまたいで残存する対象は例外なく状態機械の背後にあり、その書込は例外なく最後の列の意味でアトミックです。どの時点のリセットや電断でも、旧状態か新状態のいずれかが残存し、読込側が受理する中途半端な状態は残存しません。

| 永続の状態 | 位置 | 状態機械 | 書込する遷移 | アトミックである方法 |
|---|---|---|---|---|
| アプリのイメージのスロット（ヘッダ、本体）とその選択（状態ログkind S） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `erase`、`written`、`pending_set`、`pending_moved`、`attempt`、`confirmed`、`demoted`、`rolled_back` | 本体が先行、ヘッダが最後（スロットにイメージが存在するのは、ヘッダの再読出と検証の完了以降です）。状態ログは追記だけの記録で、それぞれCRCを保持し、交互に使用する2個の面にあります。途中で途切れた記録はCRCに失敗して無視され、前の記録が有効なままです |
| ベンダーの選択（kind V） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `pending_set`、`attempt`、`confirmed`、`rolled_back` | ベンダーの2セクタの方式: 連番とCRCを保持する項目全体を別のセクタに書込し、有効で最新の連番が優先されます |
| ブートローダのスロットとその記録（ブートローダのログ、またはマイコン定義が両方のログに同一の面を付与している構成ではAPPの記録と共有する状態ログ） | ノードのフラッシュ（stage0のあるself-boot） | ブートローダのイメージのライフサイクル（`core_image`、1.2） | エージェントの`erase`、`written`、`pending_set`。新規コアの`confirmed` / `demoted`。旧コアの`rolled_back`。ホストの`provision`（`host.provision.recorded`）。stage0の`chosen`（`boot.core_slot_chosen`、書込なし） | アプリのスロットと状態ログと同一です。stage0の起動回数は、フラッシュではなく試行のセルにあります（retained-formats.md 7） |
| 工場出荷の復旧用エージェント（vendor-stage2のマイコン） | ノードのフラッシュ、factoryのパーティション、ヘッダは分離 | イメージのライフサイクル（1.1）、ターゲットAGENTの`erase`と`written` | エージェント。確定済のアプリがOTAスロットから稼働中の間だけ | 本体が先行、ヘッダが最後。書込中は、稼働中のアプリ（そのエージェントも対象）が復帰経路です |
| インストールしたブートイメージの識別（vendor-stage2のマイコン） | プロジェクトの準備の記録 | ホストの`provision` | `host.provision.recorded` | 一時ファイルに書込し、旧ファイルに上書きする形で名称を変更します |
| ブート記録（保持メモリ、5.5） | ノードのRAMか保持のレジスタ | ブート記録: 追記だけで、記録したイベント以外に状態はありません | `boot.*`の遷移がそれぞれイベントを1個追記します | 必要な性質: どのバイトで中断された書込も、失うのは最大でそのイベントで、別のイベントを有効と誤読させることはありません。これを満たす形式は実行可能なモデルが規定します（5.5） |
| ロックアップヒューズ（保持メモリ、5.5） | ノードのRAMか保持のレジスタ | ヒューズ: `Clear → Armed → Clear`（呼出）、`Armed → Tripped → Clear`（ロックアップ、その後のクリア） | サンドボックスの呼出の前後で有効化とクリア。次回起動でトリップ | 必要な性質: どの時点のリセットも、復帰済の呼出のヒューズを偽造せず、ロックアップした呼出のヒューズを失いません。これを満たす形式は実行可能なモデルが規定します（5.5） |
| エージェントの同意 | `~/.dagon/agent.json` | 接続: `Unlinked → Linked → Unlinked` | `host.agent.linked` / `host.agent.unlinked` | 一時ファイルと名称の変更 |
| プロジェクトのセッション（オープン中のプロジェクト、リリースのキー） | serveのプロセス | プロジェクト: `Closed → Open` | `host.project.opened` / `host.project.closed` | 1個のロックで保護したプロセスメモリ |
| 診断のステップ、拡張の時間、無通信の時間、観測済の対象の表 | RAMだけ、プロセスの寿命 | 永続ではなく、イベントだけ | | |

このモデルの永続の構造に、その場で再書込するヘッダ、カウンタ、状態の欄を保持するものはありません。永続の書込は例外なく、1単位でコミットする新規記録か、名称の変更で置換するファイル全体です。このモデルが保持や永続のデータのために導入する形式（ブート記録、ロックアップヒューズ、マイコンの識別を保持するコンパイル済の構成）は、文章では規定しません。ランダムな故障の注入を伴う実行可能なモデルとして設計し、そのモデルに合格した形式が仕様です（5.5）。

### 1.7 遷移の基本要素

イベントのない遷移は存在不可能です。状態機械の状態を変更する唯一の方法が、イベントを配信する生成された遷移メソッドだからです。crateは全状態機械を1個の`state_machine!`で宣言し、crateのルートで1回だけ呼出します。

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

このマクロは状態機械ごとに以下の要素を生成します。状態の列挙型は、生成された`Machine`型にある非公開の欄の背後に配置します（宣言したcrateでのみ作成可能で、`Default`も`Clone`もなく、変更メソッドは遷移メソッドだけです。各メソッドはレイヤーの合成されたチェーン（2.3）を受信し、イベントと遷移後の状態を作成し、イベントを配信し、その後に状態を設定します。遷移元ではない状態からは何も変更せず、何も配信しません。`from`の状態は名称で、任意でフィールドを束縛し、値のパターンにはなりません）。遷移ごとに1個のペイロードの構造体を生成します（`#[non_exhaustive]`で、`Copy`でも`Clone`でもないので、宣言したcrateだけが作成可能です）。各ペイロードが所有する短縮形とその解読も生成します（2.4）。`CATALOG`も生成します（状態機械、状態、そして遷移ごとにfrom、to、トリガー、種類、コード、バイト数付きのペイロードのフィールド、`observe`の経路、`action`と`retired`のフラグ）。宣言から遷移表も生成します（`StateId`、`Trigger`、`State::leaves(Trigger)`）。状態からの遷移の有無の把握が必要なコードは、名称を比較せずにこの遷移表に照会します。リプレイが受信する状態機械の記録の解読`REPLAY`も生成します。全状態から全遷移を実行し、記録をリプレイし、遷移表をカタログと照合する、状態機械ごとのテストも生成します。イベントが保持するのはレイヤー、種類のコード（`Event::kind()`、`u8`）、ペイロードだけです。そのため、イベントを配信するイメージはカタログの名称をリンクしません。名称は、人間向けの箇所（ログの表示、`dagon_catalog::name`、生成するページ）でだけコードから参照します。crateごとに`MACHINES`のstaticを定義し、カタログの登録簿（`dagon-catalog`）がそれを列挙します。`dagon-events`に依存するcrateが欠落している間は、そこのテストが失敗します。どの遷移も到達しない状態、`emits`や`observe`の経路のない遷移、重複したコード、過大なペイロード、crate内の2個目の`state_machine!`、その外部の状態機械は、コンパイルエラーです。[イベント](/reference/events/)はカタログから生成し、テストが両者を同一に維持します。

判断点（3）は`emits`の節にはなりません。新規の判断点は設計の変更で、最初にこの文書に、上限、既定値、応答する側、安全の論拠とともに記述し、独立したトレイトのメソッドにします。

イベントを発行するのはDAGONだけです。拡張は観測するだけで、作成することはありません。

### 1.8 だれが何を観測するか

どのイベントにも最低1個の観測の経路があり、経路は生成されたカタログの属性です。そのため、生成された[イベント](/reference/events/)がこの表をイベントごとに保持します。経路のない種類があると、生成されたテストが失敗します。*Live*は同一イメージの拡張（2）です。*replay*は、アプリのエージェントが起動時にブート記録（5.5）か状態ログから再発行するものです。*host*はCLI、serveのストリーム、アクション（2.5）で、返答やVERSIONが保持するデバイス側の全イベントも観測します。リプレイが配信するのは、カタログが`replay`とマークし、`retired`ではない種類だけです。その他の記録は推測せず、受理しません。

| イベント | レイヤー | Live | Replay | Host |
|---|---|---|---|---|
| `boot.entry`、`hal_ready`、`core_slot_chosen`、`step` | boot | いいえ（ウォッチドッグの前、またはstage0） | 破損していない記録済のイベントは例外なく → `boot.*` `replayed: true`、最低1回、識別子付き | VERSIONの記録のTLV |
| `boot.watchdog_armed`とそれ以降の全ブートのイベント（`halt`と拡張やプログラムのイベントも対象） | boot | はい: ブート段階の拡張（2）、4のサンドボックス内 | 同一 | VERSION |
| `boot.halt`、および`before_jump`以前にハングしたブートや拡張でフォールトしたブートのイベント | boot | 停止やハングの時点まで | イベントは記録に残存します。最初に動作するアプリが、残存する全ブートを時系列順にリプレイします | VERSION（上書きされるまで読込可能） |
| `boot.slot_chosen`、`attempt_recorded`、`rollback`、`no_image` | boot | はい | ブート記録、そして記録がなくても状態ログ（`app.trial_started`、`app.rolled_back`） | VERSION、STATUS |
| `app.*` | app | はい | `boot_replayed`、`trial_started`、`rolled_back`はそれ自体がリプレイです | 返答、リレーの`child_*` |
| `relay.*` | relay | はい（コーディネータのアプリのチェーン） | | 中継したリクエスト |
| `host.*` | host | `on_event`のコールバック（CLI、serve）、ホストの拡張 | | ストリーム、`--events`、アクション |

## 2. 拡張のモデル

全レイヤーで形式は1種類です。構成要素は、DAGONが置換のために公開する**ポート**、ポートを提供してイベントを観測し判断に応答する**拡張**、既定では何もしない**イベント**、**1個の合成の順序**（ボードの定義 → ライブラリ → 利用者）の4点です。実行の文脈は異なりますが（ブートローダと、マイコンが許可する場合はアプリの非特権のサンドボックス。アプリのembassyのタスク。ホストのスレッドとJSON）、形式は同一です。宣言的なアクションプログラム（5.2）は、コード不要の用途のための、最も単純な拡張の種類です。

### 2.1 ポート

ポートは、DAGONが置換可能な1個の部分のために呼出するトレイトで、DAGON自身の実装が既定です。ポートを提供する拡張は、リンクされたイメージのその部分を置換します。ポートは全レイヤーにあります。

ポートのカタログは、コード内の1個の表`dagon_events::PORTS`で、ポートごとに1個の記録（`id`、存在するレイヤーの集合、レイヤーごとの既定、信頼の方針、必要なサービス、説明）です。この表から、以下の表、生成された[ポート](/reference/ports/)、マニフェストの検証（`provides`は拡張のレイヤーのポートを指定する必要があります）、4.2のサービスの表、`dagon status`のポートの列が派生します。テストがこの抜粋をカタログと同一に維持します。ポートでは*ない*DAGONの部分も、同一のカタログに`port = false`と理由とともに列挙するので、「これは置換可能か」という問いに1個のソースが応答します。

<!-- generated from dagon_events::PORTS -->
| ポート | レイヤー | 既定 | 信頼の方針 | サービス |
|---|---|---|---|---|
| `UpdateTransport` (リンクに入出力するバイト) | boot, app, relay, host | boot, app, relay: USB CDC、UART、I2Cターゲット、PIO UART; host: シリアル | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` (メッセージ ↔ セッションのトリガー: 識別、ステータス、転送、切替、再起動、確定) | boot, app, relay, host | DAGONのリンク | untrusted (ガードの省略は不可能です。ガードを実行するのはセッションの状態機械で、プロトコルではありません) | return, time, memory |
| `ImageVerifier` (ヘッダの署名、キーの検索) | boot, app | プロジェクトのキーによるEd25519 | trusted only | – |
| `Digest` (本体のハッシュ) | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` (検証器とダイジェストが使用する基本要素。ハードウェアによる高速化、HSM) | boot, app, host | ソフトウェア | trusted only | – |
| `DecisionPolicy` (試行の上限、優先するスロット) | boot | 定義の`max_attempts`、確定より待機を優先 | trusted only | – |
| `WatchdogDriver` (ブートと試行を保護するウォッチドッグ) | boot, app | マイコンの定義 | trusted only | – |
| `SlotStore` (イメージの格納先) | boot, app | マイコン定義のフラッシュの配置 | trusted only | – |
| `LogStorage` (状態ログの格納先) | boot, app | マイコン定義のフラッシュの配置 | trusted only | – |
| `Recovery` (追加の復旧経路: 転送用のサービスの時間枠、ベンダーのダウンロードモード) | boot, app | マイコンとボードの定義に従うBOOTSEL/UF2、factory、ROMの入口 | untrusted, additive (拡張は経路の追加だけが可能で、DAGONの経路は削除不可です) | return, transport, time, memory |
| `Diagnostics` (追加のVERSIONのTLV、追加のステータス、UDSのDIDの表) | boot, app, relay | なし | untrusted | return, time, memory |
| `Display` (段階やブートの結果の表示方法) | boot, app, relay | boot: アクションプログラム経由の、ボード定義の`[boot] led`と`halt`; app, relay: なし | untrusted (ブート時は宣言済のピンとバスを使用してサンドボックス内で動作します) | return, pins, buses, time, memory |
| `LogSink` (`Recent`の溢れと拡張のログの出力先) | app, relay, host | なし | untrusted | return, events, time, memory |
| `SelfCheck` (試行中スロットの確定時における健全性の応答) | app | 健全 | untrusted (判断点、3) | return, time, memory |
| `BoardBringUp` (アプリ起動時のボードの電源、ピン、バス) | app, relay | ボードの定義 | untrusted | return, pins, buses, time, memory |
| `NodeLink` (`dagon_port::NodeLink`による、リンク経由での別ノードへの到達方法) | relay | I2Cコントローラ、PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` (イメージとリリースの署名) | host | ファイルのキーの組 | untrusted (ホストは定義上信頼されます。検証の担当はデバイスです) | – |
| `KeyStore` (利用者が選択した格納先（環境変数、OSのキーストア、1Passwordなど）から署名のキーを受信) | host | ファイルのキーの組 | untrusted (ホストは定義上信頼されます。検証の担当はデバイスです) | – |
| `BuildRunner` (ノードのイメージのビルド) | host | cargo | untrusted | – |
| `Agent` (CLIとページがリクエストを委託する外部のエージェント) | host | データ定義（エージェントの定義） | untrusted | – |
| `Action` (ホストのイベントの後に実行するプログラムやリクエスト) | host | データ定義（アクションの定義） | untrusted | – |
<!-- end of generated table -->

ポートの既定はDAGONのコードです。信頼された拡張に限定したポートの置換は特権で動作し、4.7の条件下でだけ受理します。マニフェストの`provides`は、レイヤーの集合がマニフェストの`layers`の全レイヤーを包含するポートを指定します（8.4のUDSの例は、`boot`と`app`に`UpdateTransport`、`UpdateProtocol`、`Diagnostics`を宣言し、カタログはこの宣言を許可します）。その他のポートの置換は、マニフェストが宣言する能力で、レイヤーのサンドボックス（4）内で動作します。

### 2.2 拡張とマニフェスト

拡張はマニフェスト`dagon-extension.toml`を保持するRustのcrateです（最も単純な種類では、定義内のアクションプログラムです）。

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

コードの形式は全レイヤーで同一です。

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

`Ctx`は、マニフェストが宣言した能力のハンドル（GPIO、バス、タイマ、ペリフェラルのウィンドウ）を保持し、その他には何も保持しません。ポートの実装は同一の型への独立した`impl UpdateTransport for MyExt`です。`Ctx`に、フラッシュ、状態ログ、ウォッチドッグ、スロットのストア、DAGONのメモリへのハンドルはありません。サンドボックスのレイヤーでは、コードの動作にかかわらず、ハードウェアがそれらへのアクセスを拒否します（4）。

拡張のインターフェースが信頼の境界です。拡張が受信するのは`&Event`、`&Ctx`、判断の引数だけで、いずれもチェーン、公開の状態、リプレイには接続しません。チェーンの合成、チェーンへの配信、リプレイは、レイヤーの起動のAPIです。拡張のインターフェースの外部で同一イメージに組込まれたコード（DAGONのcrate、アプリケーション自身の起動コード）は、1個のアドレス空間のコードと同様に信頼の基盤に属します。信頼されていない拡張の隔離は、サンドボックスの役目です（4）。

### 2.3 合成

型付きのチェーンで、順序は型のレベルで、つまりビルド時に固定されます。

```rust
pub struct Chain<A, B> { /* private */ }   // Chain::new(a, b)
impl<A: Extension, B: Extension> Extension for Chain<A, B> { /* on_event: both; decisions: each in turn, first Some wins */ }
pub struct Nop;  // no extension
#[macro_export] macro_rules! extensions { ($a:expr $(, $b:expr)*) => { … nested Chain::new … } }
```

順序は全レイヤーで常に**公開元 → ボードの定義 → ライブラリ → 利用者**です。レイヤーの起動（ブートローダの生成された`main`、マイコン定義のコードの`start`、ホストの`Host::new`）が、`compose`（公開の状態と判断点を保持するレイヤー: app、relay）か`compose_unpublished`（ブートローダ、ホスト）で自身でチェーンを作成します。`Published`の状態（2.4）のための公開元、次にボードの定義のチェーン、次に利用者の`extensions![..]`（ライブラリが先行し、次にアプリ自身）です。結果は内容を参照不可能な`Composed`です。部分へのアクセスも順序変更も不可能で、拡張ではなく、レイヤーの起動だけが保持します。利用者のコードが自身を先頭に配置することは不可能で、`extensions![..]`が公開元を包含することはありません。通知は全拡張に到達します。判断は棄権ではない最初の応答で停止します。ポートを提供可能な拡張は、イメージごとに最大1個です。チェーンがポートを2回提供するプロジェクト（ライブラリと利用者の両方が`UpdateProtocol`を持込むなど）は、`dagon gen`が両方の名称を提示して拒否します。

レイヤーごとの登録は以下のとおりです。

- **ブートローダ（self-bootのマイコン）。** プロジェクトの`dagon.toml`がブートローダの拡張を列挙します（`[boot] extensions = ["uds-transport",
  …]`）。`dagon gen`がチェーンの型を保持する`boot_wiring.rs`を出力し、crateを、4.4の保護マップが隔離する独立したセクション（`.ext.text`、`.ext.data`、`.ext.bss`）でブートローダのイメージにリンクします。チェーンは`const`で作成します（割当なし）。ハンドラは入口のゲート（4.1）経由でブートの状態機械から同期で呼出し、呼出ごとに`max_run_us`の制限を適用します。署名付きのブートローダのイメージが拡張を包含するので、拡張を変更して書込することは、その他のブートローダの変更と同様に、ROMの経路でのブートローダの書込です。
- **アプリ（embassy）。**`node::start` / `board::start`は`App`を受信します。自己検査の関数、`exts: Option<&'static mut (dyn Extension +
  Send)>`（`static`に配置したアプリの`extensions![..]`。embassyのタスクはジェネリック化が不可能で、応答のタスクがチェーンを保持するので、ジェネリックではなく参照です）、`confirm`（`ByHost`か`Own`: 起動がアプリの試行を確定します）です。起動は、リンクの応答開始前に、ボードの`BoardBringUp`のポート（`app.bring_up`。ポートがないボードは何も発行しません）を実行し、次に自身の確定を実行します。ボードの定義の拡張（ボードの定義の`extension()`。ESP32-S3では`NodePeripherals::
  board_extension`）は、チェーンのボードの位置に配置されます。エージェントは起動とその応答のタスクの内部に限定され、受渡する公開の関数はありません。マイコン定義のコードがレイヤーの`Published`を作成して保持し、合成し、アプリに`Reader`を返却します。エージェントの状態機械が全イベントをチェーンに配信し、チェーンが3個の点で判断のメソッドに照会します。マイコンの隔離のレベルが許可する場合は（4.5）、応答のタスクはブート時と同一のサンドボックスの下で、チェーンのハンドラを非特権で実行します。それ以外の場合は、時間の上限を計測して報告すると同時に、通常のコードとして実行します。
- **リレー。** リレー自身のエージェントは、同一のチェーンを保持するアプリです。リレーの状態機械のイベントと`NodeLink`のポートはそのチェーンを経由します。
- **ホスト。**`dagon-host`は、CLIがプロジェクトの`dagon.toml`（`[host] extensions`）から作成したチェーンを`Box<dyn Extension>`として受信します。データで定義する種類（エージェントの定義、アクションの定義、プログラムやデバイスで指定するシリアルやCANのアダプタのような転送の定義）と、`dagon_host::Transport`、`Signer`、`BuildRunner`を実装するRustのcrateで、crateはCLIをビルドするワークスペースの依存として読込します。ホストは利用者の権限で動作し、そのサンドボックスはOSです。

アプリのエージェントは、合成されたチェーンだけを受信します。`Agent::new(config, verifier, store, chain)`で、`chain`は`compose`の結果、`verifier`はマイコン定義の署名の検証器です。別のフックのトレイトはありません。イベントの観測と、`accept_update`、`allow_reboot_now`、`self_check`への応答は`Extension`のメソッドです。マイコン定義のコードの`start`は、アプリの`fn() -> bool`の自己検査も継続して受理し、1メソッドの拡張（`SelfCheck`）にラップして、チェーンの利用者部分で、アプリの拡張の後に配置します。この拡張は棄権しないので、前方に配置すると拡張の応答を隠蔽するからです。

### 2.4 待機可能な公開の状態

非同期の表示やログのタスクが、ハンドラ内で動作することはありません。チェーンの先頭の公開元が、各イベントを以下の2個にコピーします。

- `Latest`: `Latest { state, last }`を保持する`embassy_sync::watch::Watch`（複数の受信側、最後の値）です。全イベントから畳込したレイヤーのスナップショット（アプリの`{ phase, progress, percent, version, trial }`、リレーの接続先のノード）と、最後のイベントの記録です。
- `Recent<N>`: 直近N個のイベントを保持する固定のリングで、`EventRecord`という、`state_machine!`がイベントごとに生成する所有された有界の形式です。`{ layer: u8, kind: u8, flags: u8, dropped: u8, seq: u32, payload:
  [u8; 16] }`。フィールドは、カタログが指定する短縮形でコピーします（レングス、id、ダイジェストの先頭。ノードのタグやボードのidのような名称は、ドメインの接頭辞と名称から計算したSHA-256の最初の8バイトとして記録し、ホストが把握済の名称を経由して対応付けます）。作成側の復帰後に参照を残存させることはありません。溢れると、最古の記録を破棄します。その後の記録が`dropped`で欠落を報告し（それぞれ最大255で、残余は後続の記録へ繰越します）、作成側は決して待機しません。`seq`は一周し、連番の演算で比較します。

どちらも1個の`Published<S, N>`にあり、レイヤーのcrate（マイコン定義、ホスト）が作成して保持し、排他の参照で`compose`に受渡します。そのため、チェーンが唯一の書込側です。表示、ログ、アプリのタスクは、読取専用の`Reader`（受信側、最新の状態、最近のリング）を受信します。ブートローダに公開元はありません。ブートローダのイベントはブート記録と拡張のハンドラに到達します。

### 2.5 ホスト: イベントのストリーム、アクション、crateの発見

`dagon serve`は、全ホストのイベントを`/v1/events`に以下の形式で出力します。

```json
{"type":"event","format":1,"layer":"host","kind":"node.result","at":"<RFC 3339 UTC>",
 "job":17,"payload":{"node":"n1","outcome":"done","version":"1.1.0"}}
```

既存の`job.*`と`device.*`のメッセージは変化しません（serve-api 1。`event`のメッセージは追加です）。受信側の規約: ストリームや`--events`の出力の受信側は、未知のメッセージの型、イベントの種類、ペイロードのフィールドをスキップし、読込不可能な`format`だけを拒否します。これにより、`EVENT_FORMAT`を上げずに種類を追加可能です。CLIは`--events`で同一のイベントを出力します（それぞれJSON 1行）。

アクションはエージェントの定義と同様に、ホストのデータで定義する拡張の種類です。実行する内容と時点を指定するTOMLファイルで、DAGONが実装する転送の種類を使用します。commandの転送（環境変数の許可リスト、新規の作業フォルダ、プロセスグループ、時間と出力の上限）は、エージェントの定義とアクションが共用するCLIの1個のモジュールです。

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

`kind = "http"`は、イベントのJSONをURLにPOSTします（httpsだけ）。アクションはイベントの発行後に動作し、操作の内部では動作しません。その結果は`host.action.ran`です（アクションは`host.action.*`を購読不可能です）。アクションは判断に応答しません。`confirm_plan`に応答するのはTTY、ページ、`--yes`です。commandのアクションは、commandのエージェントと同様に利用者の権限で動作します。

serveによるcrateの探索範囲: `dagon.toml [serve] packages`は、プロジェクトからの相対のglobの一覧です（生成されたプロジェクトでは`["nodes/*"]`）。この一覧に一致し`Cargo.toml`を保持するディレクトリだけを列挙してビルドします。DAGONに組込の探索パスはありません。

## 3. 判断点

この表にないものは通知です。ブートローダに判断点はありません。`DecisionPolicy`を提供する信頼された拡張（4.7）は例外で、それ以外のDAGON外部のコードがブートローダの起動対象に影響することはありません。その拡張は、その時点でその部分のDAGONです。

| # | 判断 | レイヤー | 応答可能な主体（順に、棄権ではない最初のもの） | 上限 | 既定 | 決して変更不可能な事項 |
|---|---|---|---|---|---|---|
| 3.1 | `accept_update(&UpdateOffer) -> Accept` | app | ボード、ライブラリ、利用者 | 50 ms。DAGONの全ガードに合格した後、消去の前 | 受理する | DAGONが拒否したものは受理されません（その場合は呼出されません）。拒否はプロトコルの"state"のステータスで応答します |
| 3.2 | `allow_reboot_now(&RebootRequest) -> Delay` | app | ボード、ライブラリ、利用者 | `Now`か`After(ms)`、`ms ≤ REBOOT_DELAY_MAX`。マイコン定義のウォッチドッグの周期（`[watchdog] trial_ms`、`confirmed_ms`）から作成するビルド時の定数で、2個の周期のうち最小値の半分です。DAGONは最初に返答し、待機中はガードの方針のもとでキックを継続し、その後にリセットします。待機中のウォッチドッグによるリセットも同一のリセットです | 即時 | 再起動は拒否不可能で、遅延には上限があります |
| 3.3 | `self_check(&TrialInfo) -> Health` | app | ボード、ライブラリ、利用者 | 50 ms。`Pending`は"state"のステータスのままで、ホストに再試行させます | 健全 | 試行中ではないスロットは確定不可能で、`confirmed`の遷移をスキップ不可能です |
| 3.4 | `confirm_plan(&Prepared) -> bool` | host | 対話する利用者（TTY、ページ）か`--yes`。アクションとホストの拡張は不可能です | 利用者の応答まで | 照会する | キーの確認とイメージの検証は、計画の表示前に実施します |

上限は計測します。合成されたチェーンは、レイヤーのクロック（マイコン定義のコードに由来するDAGONのクロックで、ポートではありません）で、拡張の呼出をそれぞれ計測します。遅延した応答は棄権として無視し、チェーンは`app.extension_overran { id, point, us }`を発行します（`id`は拡張のidで、記録ではその64ビットのFNV-1aのハッシュです）。サンドボックスのレイヤーでは、タイマが上限で呼出をプリエンプトします（4.3）。

## 4. 隔離と安全の規約

安全は、マイコンが許可する範囲では、信頼ではなくハードウェアで強制します。信頼されていない拡張はサンドボックス内で非特権で動作し、出口は、DAGONが使用のたびに検証する特権のサービス呼出だけです。マイコンの保護ユニットは、拡張に自身のコード、データ、スタックと、マニフェストが宣言した能力のウィンドウを付与し、その他には何も付与しません。DAGONのコードも、DAGONのメモリも、フラッシュコントローラも、状態とログのストレージも、ウォッチドッグも、どのバスマスタも、どのクロック、リセット、割込の経路のブロックも付与しません。DAGONはプリエンプトして呼出をアンワインドするタイマで時間の上限を強制します。超過やフォールトがあっても、DAGONはブート（や応答）を継続します。信頼されていない拡張の動作にかかわらず、ブートの判断、署名、ロールバック、ウォッチドッグの規則、確定の規則は変化しません。

このモデルは、DAGONが未対応のマイコンも想定した設計です。マイコンに固有の要素は例外なく**マイコン定義内のデータ**です。保護ハードウェアの種類とそのパラメータ、バスマスタと特権の制御ブロックとそのアドレスの範囲、フォールトとアンワインドの機構、ウォッチドッグ、保持メモリです。DAGONはマイコンごとではなく、保護ハードウェアの*種類*ごとに生成器と検査器を1個ずつ保持します（4.4）。既知の種類に属するマイコンは、定義の追加だけで対応します。新規の種類は、トレイトの背後に生成器のモジュールを1個追加して対応します。4.5にある3個のマイコンの表は定義から生成した例で、規範ではありません。規範は定義のスキーマと種類の規則です。

### 4.1 サンドボックス

| 要素 | ルール |
|---|---|
| 特権 | 拡張のハンドラは、アーキテクチャの非特権モードで、自身のスタックで動作します（Cortex-M: `CONTROL.nPRIV = 1`、`SPSEL = 1`のThreadモード。RISC-V: Uモード。PMSを搭載したXtensa: World 1。TrustZoneを搭載したARMv8-M: Non-secure状態）。DAGONは特権（またはSecure）で動作します。非特権のコードは、割込のマスク、自身の特権の変更、特権モードのスタックポインタ、保護ユニット、割込コントローラ、システム制御ブロックへの書込が不可能です（対応するどの種類でも、アーキテクチャで決定済です）。DAGONのコードやデータの実行も読出も不可能です（4.4）。 |
| 入口 | DAGONは入口のゲート経由でハンドラを呼出します。特権のRAMに`CallFrame`を記録し（呼出先が保存するレジスタ、特権のスタックポインタ、継続のアドレス、拡張のid、呼出の割当時間）、拡張の保護マップを読込し（4.4）、非特権のスタックポインタを拡張のスタックリージョンの先頭に設定し、サンドボックスのタイマを有効にし（4.3）、引数を引数レジスタに格納して、非特権モードでハンドラの入口に遷移します（借用のペイロードはDAGONが最初に拡張のデータリージョンにコピーするので、ハンドラが受信するのは自身のメモリへのポインタだけです）。 |
| 出口 | 出口は、サービス呼出`RETURN`（4.2）、サンドボックスのタイマ、フォールトだけです。3個とも4.3を経由して`CallFrame`の継続先へアンワインドし、拡張のスタックを経由することは決してありません。 |
| メモリ | 保護マップ（4.4）が許可する対象は、拡張の`.ext.text`（読込、実行）、`.ext.data`/`.ext.bss`（読込、書込、実行禁止）、スタック（読込、書込、実行禁止、底にガード）、マニフェストが宣言した能力のウィンドウだけです。その他は例外なく、非特権のアクセスでフォールトします。対象はDAGONのコードとデータ、ブート記録、ROM、全バスマスタ、全クロック、リセット、割込の経路、パッドの多重化のブロック、フラッシュコントローラ、状態とログのストレージ、ウォッチドッグ、その他の拡張のメモリです。 |
| 能力のウィンドウ | ペリフェラルをウィンドウ化可能なのは、マイコン定義がウィンドウ化可能と列挙している場合（`[[peripheral]] window_eligible = true`）だけです。メモリに書込不可能で、割込やクロックを経路付け不可能で、DAGONと何も共有しない、自己完結したレジスタブロックです（4.6、11）。その他の対象には例外なく、同一の宣言のもとでDAGONが拡張の代理で実行するサービス（4.2）経由で到達します。 |
| 時間 | マニフェストにある呼出ごとの割当時間で、レイヤーが上限を設定します。サンドボックスのタイマが割当時間でプリエンプトします（4.3）。ウォッチドッグへのキックはDAGONだけが、自身のスケジュールで実行します。 |
| 割込 | 拡張は割込ハンドラを保持せず、割込の許可、禁止、経路付けが不可能です。宣言されたペリフェラルの割込は、特権で有界のDAGONのハンドラが処理し、データを拡張ごとの特権のキューに移動し、拡張がサービスでポーリングするフラグをセットします（4.2）。サンドボックスのタイマは設定可能な最高の優先度を保持し、どの能力の割込もそれより低く、宣言した頻度を超過して割込む割込元はハンドラが無効にします（`extension_irq_storm`）。 |
| DMA | バスマスタのコントローラ、チャネル、ディスクリプタ、連結のレジスタが、ウィンドウの対象になることはありません（11）。DMAが必要な拡張の入出力は`dma_*`のサービスを経由します。DAGONが拡張のバッファを検証し、特権のバウンスバッファ経由でコピーし、コントローラを自身でプログラムし、キューで完了を報告します。キャッシュのあるマイコン（4.10）では、バウンスバッファをキャッシュ対象外のメモリに配置するか、サービスがクリーンと無効化を実施します。 |
| 反復するフォールト | 1回のブートやアプリの実行で`EXT_FAULT_LIMIT`（3）回フォールトか超過をしたハンドラは、その実行の終了まで無効にします（`extension_disabled`）。ロックアップヒューズ（4.3）は、前回のブートでコアをロックアップさせた拡張を無効にします。 |
| 有効化の順序 | サンドボックスが存在するのは`WatchdogArmed`以降です。ウォッチドッグはHALの初期化の直後、ログの読込前、どの拡張やアクションも動作を開始していない段階で、マイコン定義の`trial_ms`で有効にします。`plan`の後、判断が試行でなければ`confirmed_ms`に設定し、ジャンプの直前に1回キックします。 |

### 4.2 サービスのABI

サービス呼出は、アーキテクチャのスーパーバイザコール命令で、サービスのidと引数を引数レジスタに格納し、結果を最初のレジスタに返却します（Cortex-M: `svc #0`、r0–r3。RISC-V: `ecall`、a0–a3。Xtensa: `syscall`。TrustZoneを搭載したARMv8-M: Secureゲートウェイのベニヤの呼出で、idをベニヤの引数にした同一のABIです）。ハンドラは特権で動作し、拡張からDAGONに進入する唯一の想定された入口です。DAGON自身のコードはこの命令を使用しないので、特権モードからのサービス呼出はそれ自体がフォールトです。ABIはフラッシュ、状態ログ、ブート記録、ウォッチドッグ、DAGONのメモリに対する読込や書込のサービスを公開しません。サービスの特権の実装は、DAGONのその他のコードと同様に信頼の基盤の一部で、その不具合はDAGONの不具合です。

検証は呼出のたびに、何らかの処理の開始前に実施します。

1. **呼出元。** 実行中の拡張のidは、DAGONが入口で作成した`CallFrame`から取得し、呼出元からは取得しません。ハンドラ外（フレームなし）のサービス呼出はフォールトです。
2. **サービスのid。** レイヤーのサービスの表（ポートのカタログから生成する`const`、2.1）にあることが条件です。未知のidはフォールトで、`extension_faulted { fault: BadService }`と記録します。
3. **能力。** 能力に作用するサービスはハンドルの番号を受理します。番号は、その拡張がそのサービスの種類で宣言した能力の数（`boot_wiring.rs`から）未満であることが条件です。能力自身の制限（デバイスの書込可能なレジスタ、ピンが許可するレベル、ペリフェラルの宣言した割込の頻度）は、サービスが検証します。
4. **ポインタとレングス。** 全ポインタは呼出元の拡張のデータかスタックのリージョン内にあり、全`(pointer, length)`の範囲は、オーバーフローを検査する演算でそのリージョンを逸脱せず、アラインメントはサービスに適合し、レングスはサービスの上限を超過しないことが条件です。DAGONは**ポインタを受渡した呼出の間に**特権のバッファ経由でコピーし、その呼出の復帰後に拡張のポインタの保持を継続することはありません。後の呼出がデータを要求する場合は、再度ポインタを受渡し、再度検証します。
5. **割当時間。** サービス呼出は呼出元の割当時間内で動作します。プリエンプト不可能な部分が残存時間内に終了しないサービスは、開始前に`Busy`で拒否します。

#### 特権の操作と取消

サービスは、DAGONが呼出ごとに`CallFrame`の操作のスロットに保持する明示的な状態（`OpState`: サービス、能力、保持中の資源、段階）を保持する**特権の操作**です。サンドボックスのタイマが、クリティカルセクションの途中で特権の操作を中断することはありません。サービスは2個の部分で動作します。

- **プリエンプト不可能な部分**。有界で、サービスごとに宣言します（カタログの`np_max_us`。実装の最悪の場合と照合してテストが検証します）。バスのミューテックスの取得か解放、ペリフェラルやDMAのチャネルのプログラムか停止、割込元のマスクかクリア、最大`COPY_MAX`バイトのコピーです。タイマの割込をマスクするのはこの部分だけで、種類のクリティカルセクションの基本要素（後述）を使用します。プリエンプト不可能な部分が残存の割当時間を超過するサービスは、開始前に拒否するので（検証5）、割当時間の超過は最大でも`np_max_us`です。
- **プリエンプト可能な部分**。転送の終了を待機する部分です。サービスはタイマをマスクしたままスピンしません。待機は拡張に`Pending`として返却するか（非同期のサービス）、タイマをマスクせず`OpState`に取消対象を記録して実施します。

呼出が異常終了すると（タイマかフォールト）、4.3のアンワインドが、DAGONへの復帰前に`OpState`について**取消の手順**を実行します。この順で、各ステップは有界です。操作がアクセスしたハードウェアがアイドルになるまで、何も解放しません。

1. 操作が開始したDMAのチャネルを停止して無効にし、コントローラがチャネルのアイドルを報告するまで待機します（種類の記録の`dma_stop_max_us`が上限です。時間内に停止しないコントローラは`extension_faulted { fault: DmaStuck }`と記録し、チャネルは手順3がペリフェラルを再初期化するまでリセット状態に維持します）。
2. 能力の割込元を、割込コントローラとペリフェラルでマスクしてクリアします。
3. ポートの既定経由でペリフェラルを静止させます（転送を中止し、FIFOをクリアし、ブロックをアイドルに復帰させます。DMAが停止しない場合は、リセットのブロックでペリフェラルとチャネルをリセットします）。
4. 呼出のバウンスバッファと特権のキューの項目を回収します（DAGONが保持し、拡張で索引し、拡張のポインタでは索引しません）。
5. 保持中ならバスのミューテックスを解放します。拡張の転送がバスにアクセス不可能になったからです（バスは手順3でポートの既定が初期化した状態なので、DAGONの次回の使用は既知の状態から開始します）。
6. `OpState`をクリアします。

同一の手順が通常の呼出の終了時にも、拡張の残存物（開始して未回収のDMA）について動作します。そのため、呼出は必ず、拡張の資源が動作していない状態で終了します。したがって、呼出をまたぐ操作（非同期の入出力）がまたぐことが可能な範囲は、**1回のハンドラの呼出内**だけです。1回のハンドラの呼出で開始した`DMA_RX`は、その呼出の復帰前に回収するか取消します。

プリエンプト不可能な部分は、種類の能力の記録（`critical_section`、4.4）に記録されたクリティカルセクションの基本要素で、サンドボックスのタイマをマスクします。ARMv6-Mでは`PRIMASK`です（全割込が対象なので、この部分の上限はレイヤーの割込遅延の上限でもあり、`np_max_us`は縮小します）。ARMv7-MとARMv8-Mでは、タイマと全能力の割込をマスクし、DAGONのフォールトは許可したままの優先度のしきい値に設定した`BASEPRI`です。RISC-Vでは`mstatus.MIE`のクリア（または`mie`のタイマのビット）です。Xtensaでは、割込レベルをタイマより上に設定します。各サービスの上限`np_max_us`は、その種類で実装の最悪の場合を測定した値と照合してテストで検証します。

**リセット時の規約。** ロックアップ、ウォッチドッグによるリセット、電断は、前述の手順を実行せずに呼出を終了させます。そのため、全リセットの後、その他のコードの使用前に、レイヤーの起動が、イメージのどの拡張のどの能力がアクセスした可能性のあるペリフェラル、DMAのチャネル、割込元も全部再初期化します。方法は種類の記録の`reset_cleanup`が指定し（マイコンにリセットのブロックがあればそのブロック経由の`peripheral-reset`、なければ各ポートの既定経由の`reinit`）、ブロックの一覧はイメージのマニフェストに由来します。そのため、後始末は保護マップと同時に生成され、最初のサービス呼出以前、`boot.hal_ready`（ブートローダ）と`app.bring_up`の前（アプリ）で動作します。DAGON自身が使用するペリフェラル（リンク、LED）は、どのリセットの後もDAGON自身の起動処理が再初期化します。適合試験のビルドは、各サービスのプリエンプト不可能な部分の内部でウォッチドッグによるリセットを発生させ、次回のブートで同一のペリフェラルとチャネルの最初の使用が成功することを検証します。

#### サービス

完全な一覧はポートのカタログから生成します。種類は以下のとおりです。

| 種類 | サービス | プリエンプト不可能な部分 | `OpState`の状態 | 取消 |
|---|---|---|---|---|
| Return | `RETURN(result)` | なし | なし | なし |
| Events | `LOG(ptr, len)` | シンクのバッファへ ≤ `COPY_MAX`のコピー | なし | なし |
| Pins | `GPIO_SET(cap, level)`、`GPIO_GET(cap)` | レジスタの書込か読出1回 | なし | なし |
| Buses | `BUS_WRITE(cap, reg, ptr, len)`、`BUS_READ(cap, reg, ptr, len)` | ミューテックスを取得し、転送を開始する。**この呼出内で**、デバイスの`max_transfer`以下を拡張のバッファとの間でコピーする | ミューテックスの保持、転送中 | 手順2、3、5 |
| Transport | `IO_TX(cap, ptr, len)`、`IO_RX(cap, ptr, len) -> n`、`IO_POLL(cap) -> flags` | 拡張のバッファと特権のキューとの間のコピー。割込元の許可か禁止 | キュー以外になし | 手順2 |
| DMA | `DMA_TX_START(cap, ptr, len) -> op`、`DMA_RX_START(cap, len) -> op`、`DMA_STATUS(op) -> Pending \| Done(n) \| Failed`、`DMA_COLLECT(op, ptr, len) -> n`、`DMA_CANCEL(op)` | `START`: 拡張のバッファをバウンスバッファにコピーし（TX）、チャネルをプログラムする。`COLLECT`: バウンスバッファから ≤ `n`バイトを、この呼出に受渡され**再検証した**拡張のバッファにコピーする。`CANCEL`: 手順1–4 | チャネル、バウンスバッファ、完了したバイト数 | 手順1–4。未回収のRXは破棄します |
| Time | `NOW_US()`、`BUDGET_LEFT_US()` | なし | なし | なし |
| Memory | `SCRATCH(len) -> ptr` | なし（拡張自身のリージョンのバンプポインタ） | なし | バンプポインタは呼出の終了時にリセットします |

したがって、`DMA_RX`が完了時に拡張のメモリに書込することはありません。コントローラはDAGONのバウンスバッファに書込し、拡張は`DMA_STATUS`（または能力のキューのフラグ）で完了を把握し、`DMA_COLLECT`が、拡張が指定しDAGONがその時点で検証したバッファにコピーします。拡張のポインタが、受渡した呼出の終了後も存続することはありません。

#### 取消の境界での適合試験

取消の列に項目がある全サービスについて、マイコン定義のコードによる適合試験のビルドは、各境界（ミューテックスの前、ミューテックスの保持中、転送の開始後、完了後かつ回収前、回収中、回収後）でタイマが発火するようにして呼出を実行します。検証する事項は、DAGONによる同一のバスやチャネルの次回の使用が成功すること、呼出の終了後に拡張のメモリに1バイトも到達しないこと、バウンスバッファが回収されること（次回の呼出の再利用で旧データが参照不可能なこと）、能力の割込がマスクされ次回の呼出だけが再び許可すること、割当時間の超過が最大でも`np_max_us`であることの5点です。実行可能なモデル（5.5）は、ブートのシナリオで同一の境界を実行します。

### 4.3 プリエンプション、フォールト、アンワインド

サンドボックスのタイマ（`[isolation.capabilities] timer`: Cortex-MではSysTick、RISC-Vでは`mtimecmp`かプラットフォームのタイマ、Xtensaではタイマグループのアラーム）は、各呼出に先行して、その割当時間で、設定可能な最高の優先度（どの能力の割込よりも、フォールト以外のDAGONのどのハンドラよりも上位）で有効にします。サービスのプリエンプト不可能な部分は、種類のクリティカルセクションの基本要素で最大`np_max_us`の間このタイマをマスクします（4.2）。タイマのハンドラ、フォールトのハンドラ、`RETURN`のサービスは、いずれも同様に呼出を終了します。手順はどのアーキテクチャでも同一で、関係するレジスタは種類のアンワインドのモジュール（`[isolation.capabilities] unwind`: `cortex-m-exception`、`riscv-trap`、`xtensa-world`、`armv8m-tz`）に由来します。

1. **スタックされたフレームの使用可否を決定します。** Cortex-MのARMv7-MとARMv8-M: `MSTKERR`/`STKERR`がセット済なら使用不可能です（入口のスタッキングがフォールトしました）。ARMv6-M: プロセスのスタックポインタが拡張のスタックリージョン外か、アラインされていなければ使用不可能です。RISC-V: トラップはスタックしないので（レジスタは無変更で、`mepc`/`mcause`/`mtval`はCSRです）、フレームは稼働中のレジスタファイルで、常に使用可能です。Xtensa: World 1のスタックポインタがリージョン外なら使用不可能です。手順は使用不可能なフレームを決して読込せず、それ以外の場合もフレームに依存しません。継続先は、拡張が到達不可能な特権のRAMにある`CallFrame`に由来します。
2. **帰属を決定します。** 呼出の拡張のidは`CallFrame`にあります。フォールトの種類と、アーキテクチャが報告する場合はフォールトしたデータのアドレスを追加します（ARMv7-MとARMv8-M: `CFSR`と、`MMARVALID`がセット済の場合だけ`MMFAR`、それ以外はアドレスなし。RISC-V: `mcause`と`mtval`。Xtensa: `EXCCAUSE`と`EXCVADDR`）。ARMv6-Mにはフォールトのステータスやアドレスのレジスタがありません（全フォールトがHardFaultです）。そのため、記録はフレームが使用可能な場合はスタックされた`PC`とその所属リージョンを、それ以外は`PC: unknown`を保持します。`CallFrame`がオープン中の場合、フォールトは常に拡張に帰属させます。
3. **取消します。**`OpState`について4.2の取消の手順を実行します（DMAのアイドル、割込のマスク、ペリフェラルの静止、バッファの回収、その後にミューテックスの解放）。
4. **アンワインドします。** ハンドラ自身の（特権の）スタック上で実施します。`CallFrame`から呼出先が保存するレジスタを復元し、特権のスタックポインタを保存値に設定し、DAGONの特権の保護マップ（バックグラウンドのマップで、拡張のリージョンは全部無効）を読込し、復帰先の文脈の特権モードを復元し、中断した呼出の遅延保存された浮動小数点の文脈を破棄します（Cortex-M4F/M7: `FPCCR.LSPACT`をクリアし、基本フレームで復帰します。ブートローダはFPUを無効にして動作するので、拡張のFP命令は`NOCP`のフォールトです。拡張は、どのレイヤーと種類でもソフトフロートでコンパイルします）。
5. **復帰します。** 例外からDAGONの継続先へ復帰します（Cortex-M: メインスタック上の合成した基本の例外フレームと、特権のThreadモードの`EXC_RETURN`。RISC-V: `mepc`を継続先に、`mstatus.MPP`をMモードに設定して`mret`。Xtensa: 継続先でWorld 0へ復帰します。ARMv8-M TrustZone: Secureのスタック経由でSecureの継続先へ）。継続先は呼出の結果（`Returned(result) | Overran |
   Faulted(kind) | Disabled`）を`CallFrame`から読込して処理を継続するDAGONのコードです。未復帰のポートの呼出はポートのエラー値を返却し、未復帰の通知はその時点で終了です。
6. **記録します。** 手順2の帰属とともに、`extension_ran | extension_overran | extension_faulted`を記録します。

タイマとフォールトが同時の場合: どちらのハンドラも、`CallFrame`の状態で保護した1個のアンワインドの処理を経由します。後から動作したハンドラはフレームのクローズを検出し、アンワインドせずに復帰します。

#### ロックアップとヒューズ

拡張がスタックポインタをリージョン外に移動した場合（スタックの演算は非特権です。システム命令でスタックポインタのレジスタに書込するのは、対応するどの種類でも特権だけです）の、タイマやフォールトの例外の入口でのフォールト: ARMv7-M/ARMv8-Mでは、プロセッサはスタッキングのエラーを提示してハンドラに進入し、前述の手順が適用されます。RISC-Vではハードウェアは何もスタックしないので、手順が適用されます。ARMv6-Mでは、昇格したHardFaultが同一の不正なポインタにスタックし、コアはロックアップします。Xtensaでは、不正なスタックでのWorld切替の例外は特権のスタックで処理します。ロックアップはウォッチドッグが終了させ（ブート時は拡張の動作開始前に有効化、4.1。アプリではアプリのガード）、4.2のリセット時の規約が、呼出がアクセスした対象を再初期化します。

ロックアップがリセットのたびに反復しないように、DAGONは保持メモリに**ロックアップヒューズ**を保持します。前回の実行が異常終了した時点で、サンドボックスの呼出がどのイメージのどの拡張を実行中だったかを明示する記録で、全レイヤーの起動が読込し、その拡張を無効にして動作するために使用します。ヒューズは保持メモリの状態で、その正当性はあらゆる時点のリセット、識別の衝突、イメージと拡張の新版の書込のもとでのバイト単位の動作に依存します。そのため、ブート記録と同様に、文章では規定せず、要件（5.5）と、要件に合格した形式を規範とする実行可能なモデルで規定します。設計がここで決定する事項は以下のとおりです。

- 全レイヤー（ブートローダ、アプリ、リレー）の全サンドボックスの呼出を網羅します。
- 確定済のアプリの拡張がコアをロックアップさせた場合は、その拡張を無効にして復帰し、応答を継続します。`commit`後のブートローダのロックアップで失うのはそのブートの試行だけで、ヒューズによって、失う試行は3回ではなく1回です。
- ヒューズの読込と書込を実施するのはレイヤーの起動だけです。ブート記録ではなく、`plan`、`commit`、`self_check`、`confirm`の入力でもありません。
- トリップしたヒューズは、VERSION（0x07）と`dagon status`（"extension <id> disabled after a lockup"）で報告し、ヒューズが記録する識別が稼働中のイメージに存在しなくなった時点でクリアします。クリアする要求はありません。プロトコルにもCLIにもないためです（5.5）。

DAGON自身のフォールトハンドラ内のフォールトは、診断コードを伴うDAGONの停止です。

### 4.4 保護の種類、定義のスキーマ、生成されるマップ

マイコン定義は保護ハードウェアをデータとして宣言します。

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

記述方法: 定義の値は`a-z 0-9 -`のケバブケースの語です（そのため`id-cache`です）。プロジェクトの定義にある、単純な名称ではない文字列は、PC外への送出前に置換するからです（[マイコンとボードの定義](/reference/definitions/)）。

能力の記録は2回検証します。`dagon gen`は定義の数値に準拠して計画し、種類が許可する集合（後述）の外の数値を保持する定義を拒否します。ブート時と適合試験のビルドでは、マイコン定義のコードがユニット自身のレジスタ（Cortex-Mの`MPU_TYPE`、SAUのタイプレジスタ、RISC-Vの書込可能な`pmpcfg`のエントリ数と再読出可能な最小のNAPOTのサイズ、PMSのバージョンのレジスタ）を読込し、ハードウェアが記録を裏付けないレベルを有効にしません（`boot.isolation_mismatch`、レベルは`none`と報告します）。

検査器は、`kind`が`none`以外の全定義に以下の4点を要求します。1点目は、`flash`、`watchdog`、`clock`、`reset`、`irq-routing`のそれぞれについて`privileged_why`を保持する`[[peripheral]]`が最低1個あることです。2点目は、`bus_master = true`の項目が全部`window_eligible = false`であることです。3点目は、`[[core]]`が複数あるマイコンでの`core-control`のブロックです。4点目は、保護の設定のレジスタブロックを保持する種類（`esp-pms`、`riscv-epmp`）での`protection`のブロックです。いずれかが欠落した定義は、欠落した区分の名称を提示して拒否します。DMAコントローラの位置を定義が明示しないマイコンに、`none`より上のレベルは付与されません。

マップの生成器と検査器は`kind`についてジェネリックです。種類ごとに1個のモジュールが1個のトレイトの背後にあり、`dagon gen`とテストが使用します。

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

`plan`は、`.ext.text`、`.ext.data`/`.ext.bss`、スタックを、種類のアラインメントの規則が許可するアドレスに配置し（リンカスクリプトは同一の結果から生成します）、宣言されたウィンドウを割当て、エントリが収容不可能な場合は明確なメッセージで拒否します（`extension
uds-can needs 5 windows; 3 fit after code, data and stack on a
8-region armv6m-mpu`）。`check`は、マイコン定義のコードのテストで、生成された表と定義に照合して動作します。適合試験のビルドは、デバイス上の拡張から各特権のブロックへの書込を実行し、フォールトのイベントを期待します。既知の種類に属するマイコンの追加時にコードは変更しません。種類の追加時はモジュールを1個追加します。

種類ごとに、ハードウェアが付与する機能と生成器の処理は以下のとおりです。

| 種類 | ユニット | 能力の記録と形式 | フォールトの報告 | 保証する事項 | 不可能な事項 |
|---|---|---|---|---|---|
| `armv6m-mpu` | ARMv6-M PMSAv6 MPU | `mpu_regions`（実装の選択で、`MPU_TYPE`から再読出します。RP2040では8）、2のべき乗のサイズ、サイズに整列したベース、実装の`min_region`（RP2040では256 B）、256 Bからの8サブリージョン、`PRIVDEFENA`。`critical_section = primask`（このアーキテクチャに`BASEPRI`はないので、プリエンプト不可能な部分は全割込をマスクし、`np_max_us`がレイヤーの割込遅延の上限です） | 全フォールトがHardFault。ステータスやアドレスのレジスタはありません。帰属は`CallFrame`とスタックされたPC（4.3） | 有効なリージョン外の全対象への非特権のアクセスの拒否。リージョンごとのXN | 例外のスタッキング中のフォールトでコアがロックアップします（ロックアップヒューズ、4.3）。ウィンドウは`min_region`以上の整列したブロックが必要です |
| `armv7m-mpu` | ARMv7-M PMSAv7 MPU | 実装の`mpu_regions` ∈ {8, 16}（Cortex-M3/M4: 8。Cortex-M7: 8か16、再読出します）、2のべき乗、サイズに整列したベース、最小32 B、256 Bからのサブリージョン、`PRIVDEFENA`。`critical_section = basepri` | MemManageと`CFSR`、`MMARVALID`の場合は`MMFAR`。スタッキングのエラーは提示され、ロックアップしません | 前行と同一で、データのフォールトのアドレスが付属します | ウィンドウは2のべき乗のブロックです。キャッシュのあるコアでは4.10の規則が適用されます |
| `armv8m-mpu` | ARMv8-M PMSAv8 MPU | 実装の`mpu_regions` ∈ {0, 4, 8, 12, 16}（再読出します）、32 Bの粒度のベースとリミットの組（2のべき乗の規則もサブリージョンもありません）、`PRIVDEFENA` | MemManageと、有効な場合は`MMFAR` | `armv7m-mpu`と同一で、ウィンドウは32 Bに整列した任意の範囲です | リージョン0は`none`です |
| `armv8m-tz` | Security Extensionを搭載したARMv8-M | 独立した3個の記録: `sau_regions` ∈ {0, 4, 8}、`s_mpu_regions`と`ns_mpu_regions`はそれぞれ ∈ {0, 4, 8, 12, 16}、全部再読出します。そしてマイコン固有の固定帰属を記述した`idau`の表と、定義が列挙する`mpc`/`ppc`のユニット。DAGONはSecure、拡張はNS-MPUの下でNon-secureかつ非特権です。サービスのABIはSecureのゲートウェイです（NSCリージョンにある`SG`のベニヤ） | SecureとNon-secureのフォールト、`SecureFault` | 最も強力です。Secureのメモリは、Non-secure状態で特権であっても拡張からは参照不可能です。バスマスタはインターコネクトがSecure/Non-secureに帰属させ、定義が各マスタの帰属を列挙します | IDAU/MPC/PPCの設定はマイコン固有のデータです。インターコネクトが帰属不可能なマスタはSecureに限定します。帰属はIDAUだけでも（完全な`idau`の表を保持する`sau_regions = 0`は有効な`tz`の定義です）、IDAUとSAUでも可能です。レベルが`none`になるのは、どちらも拡張のリージョンを帰属不可能な場合だけです |
| `riscv-pmp` | RISC-V PMP | 実装の`pmp_entries` ∈ {0, 16, 64}（書込可能な`pmpcfg`のエントリの検査で再読出します）、アドレスモードNA4（4 B、`na4`の場合）、NAPOT（8 Bから、実装の`grain`を再読出します）、TOR（粒度以上の任意の範囲、`tor`の場合）。拡張はUモード、DAGONはMモード。`critical_section = mstatus-mie` | `mcause`、`mtval`（アドレス）、ハードウェアのスタッキングなし | Uモードに対する、未付与の全対象の拒否。エントリごとのXN | ePMPがないと、MモードはPMPで制限されません。エントリは少数で、TORは境界ごとに項目を消費します。エントリ0は`none`です |
| `riscv-epmp` | RISC-V ePMP（Smepmp） | PMPと同一で、`mseccfg`のマシンモードのロックダウンと共有リージョンが追加されます | PMPと同一 | PMPと同一で、DAGONはMモードによる拡張のメモリからの実行を禁止可能です | |
| `esp-pms` | EspressifのPermission Control + World Controller | 実装の`worlds`、`memory_regions`、`peripheral_slots`、`gdma_channels`（PMSのバージョンとリージョンのレジスタから再読出します）。`critical_section = xtensa-level`。内部メモリのリージョン、MMU経由の外部メモリ、各ペリフェラル、各GDMAチャネルについてのWorldごと（World 0はDAGON、World 1は拡張）の権限。コアごとに適用し、ロック可能です | `EXCCAUSE`/`EXCVADDR`、World切替の例外 | World 1に対する、未付与の全対象の拒否。全コアで、GDMAも対象です | PMS/WCLのレジスタはDAGONのマイコン定義のコードがプログラムする必要があります（HALのドライバはありません） |
| `none` | 使用可能なユニットがない | | | 時間の測定、イベントの記録 | メモリ、フラッシュ、ウォッチドッグ、ハング（4.5） |

定義が主張可能なレベルは種類から決定します。MPUとPMPの種類は`mpu`、`esp-pms`は`pms`、`armv8m-tz`は`tz`、それ以外は`none`です。

### 4.5 マイコンごとの隔離のレベルと3個の例

`dagon-mcu.toml`は、そのマイコンでDAGONが強制するレベルを宣言します。VERSIONがそのレベルを報告するので（5.4）、ホストはノードの隣に表示します。

| レベル | 保証 | 保証しない事項（文書、VERSION、UIで明示します） |
|---|---|---|
| `mpu` | 4.1–4.4の全項目: 非特権の実行、DAGONのコードとデータの拒否、サービス呼出の検証、能力のウィンドウ、特権のDMAと割込、タイマのプリエンプションとアンワインド、フォールトからの回復、ロックアップヒューズ | 宣言された能力の物理的な効果（4.6） |
| `pms` | パーミッションコントローラとWorldの切替で同一の保証を、全コアで、チャネルごとのGDMAの拒否とともに提供 | 同一 |
| `tz` | Security Extensionで同一の保証を提供。さらに、DAGONのメモリはどのNon-secureのソフトウェアからも参照不可能です | 同一 |
| `none` | 時間の上限を計測して報告しますが（`extension_overran`）、プリエンプトしません。能力のハンドルは宣言どおりに受渡します。イベントは記録します。ハードウェアの拒否はありません。拡張は、不具合や意図によって、フラッシュへの書込、ウォッチドッグやDAGONのメモリへのアクセス、ウォッチドッグがチップをリセットするまでのハングが可能です（ブート時は、`commit`の後に発生した場合だけ、試行を失います） | メモリとフラッシュの拒否、時間の上限、誤動作する拡張に対する判断、署名、ロールバック、ウォッチドッグの規則。`dagon status`とページは、そのノードに"extensions unguarded"と表示し、`dagon doctor`は、プロジェクトがそのノードに信頼されていない拡張をリンクすると警告します |

DAGONが定義を配布している3個のマイコンを、スキーマの例として列挙します（規範となるマップは、`targets/mcu/*/dagon-mcu.toml`の定義から`dagon gen`が生成するマップです。以下の事実はベンダーのリファレンスマニュアルから取得し、起動処理時にユニットのタイプレジスタから再読出します）。

**STM32F401RE**: `kind = "armv7m-mpu"`、`mpu_regions = 8`（`MPU_TYPE`から再読出します）、`min_region = 32`、`subregions_from = 256`、`fault_address = true`、`unwind =
"cortex-m-exception"`、ブート時とアプリで`level = "mpu"`です（ST PM0214 4.2節MPU、ブロックのアドレスはST RM0368 2節のメモリマップ）。ウィンドウ2個の拡張1個について生成されるマップは以下のとおりです。

| リージョン | ベース | サイズ | 属性 | 目的 |
|---|---|---|---|---|
| 0 | イメージの本体内の`.ext.text`、サイズに整列 | 2 KiB … 32 KiB | 非特権RO、X | 拡張のコードと定数 |
| 1 | RAMの`.ext.data` + `.ext.bss`、整列 | 1 KiB … 16 KiB | 非特権RW、XN | 拡張のデータ |
| 2 | 拡張のスタック、整列 | 1 KiB … 8 KiB | 非特権RW、XN。最下位のサブリージョンは無効 | ガードを設けたスタック |
| 3–4 | 能力のウィンドウ: それぞれ1 KiBのAPB/AHBのブロック1個 | 1 KiB | 非特権RW、XN | 宣言されたペリフェラル（ウィンドウ化可能なもの） |
| 5–6 | ウィンドウの追加用に未使用 | | | |
| 7 | `ram_bytes`が2のべき乗ではない場合に使用する、2個目のデータリージョン用の予約 | | | |

特権（`window_eligible = false`）: フラッシュインターフェース`0x4002_3C00`（`flash`）、IWDG `0x4000_3000`とWWDG（`watchdog`）、RTCとバックアップ`0x4000_2800`（`power`）、RCC `0x4002_3800`（`clock`、`reset`）、PWR、SYSCFGとEXTI `0x4001_3800`/`0x4001_3C00`（`irq-routing`、`pinmux`）、DMA1 `0x4002_6000`とDMA2 `0x4002_6400`（`dma`、バスマスタ）、USB OTG FS `0x5000_0000`（DAGONのリンクと共有）、CRC、システムメモリ`0x1FFF_0000`（`rom`）、オプションバイト、PPB。

**RP2040**: `kind = "armv6m-mpu"`、`mpu_regions = 8`（`MPU_TYPE`から再読出します）、`min_region = 256`、`subregions_from = 256`、`fault_address = false`、`unwind =
"cortex-m-exception"`、2個の`[[core]]`、HardFaultの帰属とロックアップヒューズを伴い、ブート時とアプリで`level = "mpu"`です（RP2040データシート2.4節のCortex-M0+の機能とMPUのタイプレジスタ、Arm ARMv6-M Architecture Reference Manualの例外モデル）。生成されるマップは以下のとおりです。

| リージョン | ベース | サイズ | 属性 | 目的 |
|---|---|---|---|---|
| 0 | イメージのスロット本体内にある、XIP領域の`.ext.text`、サイズに整列 | 1 KiB … 32 KiB | 非特権RO、X | 拡張のコードと定数 |
| 1 | SRAMの`.ext.data` + `.ext.bss`、整列 | 256 B … 16 KiB | 非特権RW、XN | 拡張のデータ |
| 2 | 拡張のスタック、整列 | 1 KiB … 8 KiB | 非特権RW、XN。最下位のサブリージョンは無効 | ガードを設けたスタック |
| 3–5 | 能力のウィンドウ: それぞれ16 KiBのAPBのブロック1個（UART、SPI、I2C、PWM、ADC、PIO） | 16 KiB | 非特権RW、XN | 宣言されたペリフェラル（ウィンドウ化可能なもの） |
| 6 | 2個目のデータリージョン用の予約 | | | |
| 7 | ウィンドウ内の部分ブロックの拒否のために予約 | | アクセス不可 | |

特権: ブートROM `0x0000_0000`（`rom`。そのフラッシュのルーチンにはROMを実行しないと到達不可能で、ROMは全体が拒否されます）、XIPの制御`0x1400_0000`とSSI `0x1800_0000`（`flash`）、DMA `0x5000_0000`（`dma`、バスマスタ）、USB `0x5010_0000`（自身のDPRAMへのバスマスタで、DAGONのリンク）、WATCHDOG `0x4005_8000`（`watchdog`）、CLOCKS `0x4000_8000`（`clock`）、RESETS `0x4000_C000`（`reset`）、PSM `0x4001_0000`とSIO `0xD000_0000`（`core-control`: 2個目のコアをDAGONは起動せず、起動可能なブロックは特権なので、1コアのマップで十分です。GPIOの出力はSIOにあり、サービスです）、IO_BANK0/QSPIとPADS `0x4001_4000`–`0x4001_C000`（`pinmux`）、XOSC/PLL/ROSC、VREG（`power`）、TIMER、RTC、PPB。

**ESP32-S3**: `kind = "esp-pms"`、2個の`[[core]]`、`boot_extensions =
false`（ベンダーのstage 2で、DAGONのブート段階はありません）、`fault_address = true`、`unwind = "xtensa-world"`です。アプリで`level = "pms"`です。HALはドライバを公開していないので、マイコン定義のコードがPMSとWCLのレジスタをプログラムします（Espressif ESP32-S3 TRMの"World Controller"と"Permission Control"の章）。`pms`で生成されるマップは以下のとおりです。

| 資源 | World 1（拡張） | 補足 |
|---|---|---|
| 内部SRAM（またはMMU経由のIROM）の`.ext.text`のリージョン | 読込、実行 | |
| `.ext.data`、スタックのリージョン | 読込、書込、実行不可 | スタックの下方の拒否されたリージョンがガードです |
| 宣言されたペリフェラル | 読込、書込 | 定義内のウィンドウ化可能な対象の一覧 |
| その他の全対象: DAGONのコードとデータ、ROM、SPI0/SPI1（キャッシュとフラッシュ、`flash`）、RTCとTIMGのウォッチドッグ（`watchdog`）、RTC_CNTL（`power`、`reset`）、システムと割込マトリクスのレジスタ（`irq-routing`、`clock`）、GDMA（`dma`、バスマスタ）、USB OTG（バスマスタ）、AES/SHA/RSAのエンジン（GDMA経由のバスマスタ）、両コアの制御レジスタ（`core-control`）、PMSとWCLのレジスタ（`protection`） | アクセス不可 | GDMAのパーミッション制御は、どのチャネルからのWorld 1のメモリアクセスも拒否します。マップは両コアに適用し、World 1への移行前にロックします |

### 4.6 サンドボックスが保証不可能な事項

どのレベルでも保証しないのは、宣言された能力が物理的な世界で実現可能な動作です。マニフェストが宣言したピンは、配線次第でボードの電源を切断可能です。宣言したデバイスの書込可能なレジスタへのバスの書込は、そのデバイスの状態を変更可能です。それぞれの役目は以下のとおりです。

- マニフェストが能力を宣言します。
- `dagon gen`が、プロジェクトの全拡張の宣言された能力を表示します。
- ボードの定義がバスの能力をデバイスとレジスタの集合に、ピンを許可するレベルに限定します。
- 配線はボードの作者の責任です。

ペリフェラルのウィンドウ化可否は、メモリに書込不可能で、割込やクロックを経路付け不可能で、DAGONと状態を共有しないという、マイコン定義の表明です（11）。バスマスタをウィンドウ化可能とマークした定義を、検査器は拒否します。

保証の範囲はDAGONの公開APIとサンドボックスです。特権でリンクされるコード（DAGON自身、信頼された拡張）は、構造上その範囲外です。

### 4.7 信頼された拡張と認証のチェーン

信頼された拡張に限定したポート（2.1）を提供する拡張は、安全の規則を構成する部分を置換します。受理するのは、以下の条件を全部満たす場合だけです。

1. マニフェストが`trusted = true`と宣言し、提供する信頼されたポートを列挙します。
2. プロジェクトの`dagon.toml`が、ノードの`[extensions.nodes.<id>] trusted`の下に、idとcrateのソースのハッシュで列挙します。ハッシュはcrateのフォルダの全ファイルをパス（`/`区切り）の順に配列し、ファイルごとにパス、ゼロのバイト、レングス（リトルエンディアンのu64）、バイト列（CRLFはLFとして読込）を連結したデータのSHA-256です。`include!`と`#[path]`でコンパイラがどのファイルでも読込可能なので、`target/`と非表示のフォルダも対象で、除外するファイルはありません。そのため、拡張のフォルダ内でcargoを実行すると、ハッシュが変化します。ハッシュの対象がソースなのは、`dagon gen`の実行時にリンクされたバイト列が存在しないからです。

   `dagon gen`はハッシュを検証し（相違する場合は検出した値を提示します）、次にホストでブート用crateの依存グラフを検証します。cargoが全フィーチャ、全ターゲットで解決し（`Cargo.lock`の生成後は`--locked`）、これはどの1回のビルドよりも広い集合です。組込まれる全crate（通常とビルドの依存）は、DAGONのcrate、宣言された信頼された拡張のフォルダ内のcrate（つまりそのハッシュの対象）、またはブート用crateの`Cargo.lock`にチェックサムがあるレジストリのcrateである必要があります。

   確認に合格すると、genは`graph.stamp`を出力します。ブート用crateの`Cargo.toml`、`Cargo.lock`、各信頼された拡張のソースハッシュから計算したSHA-256です。ブートローダのビルドはこのスタンプを再計算し、スタンプがない場合や相違する場合はビルドしません。そのため、最新の入力についてグラフを未検証のブート用crateがビルドされることはありません。グラフをビルドスクリプトではなくホストで検証するのは、ビルドスクリプトからは外側のビルドのフィーチャも、オフラインではcargoが解決する全crateも参照不可能だからです。

   対象外の事項: 拡張が依存するレジストリのcrate（チェックサムで固定しますが、レビューはしません）と、拡張が自身のフォルダの外部から取込むソース（外部を指すパスの`include!`や`#[path]`）です。これらは特権で動作するコードとしてレビューします。
3. その拡張を包含するイメージがプロジェクトのキーで署名され、署名が拡張のバイト列を網羅し、イメージの動作前にデバイスがその署名を検証します（後述）。
4. `dagon gen`、`dagon status`、`dagon doctor`、ページが、各ノードの信頼された拡張をidで表示し、VERSIONがそのidとソースハッシュの最初のバイトを表示用に（0x08）、ハッシュ全体を識別用に（0x09）報告します。

拡張はポートを提供せずに信頼されることも可能です。マニフェストが`trusted = true`と宣言し、イベントを観測するだけの観測者です。前述の項目2から4のもとで受理し、判断点には応答せず、ハンドラの呼出はそれぞれ、その他の拡張と同様にマニフェストの`max_run_us`と照合して計測します（超過した呼出は`<layer>.extension_overran`と報告し、停止しません）。理由は、プロジェクト自身のブート時のコード（ブートをディスプレイに表示する、自身のハードウェアを動作させる）が、サンドボックスが拡張に禁止する対象、例えばDAGONのマップが開放しないバスやペリフェラルを、まさに必要とする場合が頻繁にあるからです。信頼されたと宣言すると、全プロジェクトのサンドボックスを弱体化させる代替として、そのプロジェクトでDAGONの一部になります。

信頼された拡張は特権で、DAGON自身のリージョン内で動作します。そのプロジェクトではDAGONのその部分です。4.1–4.6のいずれも、信頼された拡張には適用されません。文書は信頼された拡張に言及する箇所では例外なくその旨を明示します。

チェーンの全検証器（`ImageVerifier`と`Digest`のポートで、既定がこの規則です）が使用する認証の規則は、以下の1個です。

- **署名するバイト列。** イメージのヘッダの署名は、ヘッダのオフセット0から署名の欄の先頭までのバイトを網羅します。その範囲には、形式番号、イメージの種類、マイコンの識別、ターゲットのタグ、スロットのバインディング、バージョン、本体のレングス、本体のSHA-256があります。本体はそのダイジェストで認証され、本体は全`.ext.*`のセクションを包含するので、拡張のバイト列も網羅されます。
- **キーの選択。** 検証器が受理するのは、ヘッダが指定するidのキーだけで、そのidがビルド時に検証するイメージに埋込まれたプロジェクトのキーの場合だけです（`DAGON_PUBKEY`。公開のテストキーは、そのキーを選択したビルドだけ）。デバイスにキーの一覧はなく、予備のキーもありません。
- **誰が何を検証するか。** エージェントは`written`に先行してイメージを検証し、ブートローダは全ブートで`chosen`に先行して検証します。待機のスロットと確定済のスロットの両方が対象です。状態ログにはキーがないので、記録だけでイメージを信頼することはありません。署名の検査は、ブートごとに起動するスロットの1回です（待機のスロットを拒否して確定済のスロットを検証する場合は2回です）。署名が検証不可能なスロットは決して起動せず、ログが指定しないスロットを起動することもありません。ブートローダのイメージは、エージェントがこの規則で`written`と`pending_set`の各時点の直前に再度検証します（SLOT_PENDINGの記録がそのヘッダのダイジェストを指定します）。

  stage0のあるself-bootのマイコンでは、stage0はダイジェストとともに、マイコン定義が指定する位置（`[stage0] key`。位置は、stage0自身のフラッシュに組込んだ定数の`flash`だけです）にあるプロジェクトのキーで、ブートローダのヘッダの署名を検証します。コアを起動しうる全経路で検証します。待機のコアと確定済のコアです。起動可能なコアがない場合に起動する確定済のアプリも、同一のキーで検証します（1.2）。署名が検証不可能なコアは決して起動しないので、署名のキーなしでフラッシュに書込可能なコードが、自身のコアをstage0に起動させることは不可能です。stage0の検証器（`Ed25519Small`）が受理する対象は、ホストとエージェントが受理する対象と完全に同一です。キーはstage0の一部で、stage0に現場で書込することはありません。プロジェクトの署名のキーを交換するには、新規キーでstage0を再ビルドし、マイコンのROMローダで書込します。新規キーでビルドしたブートローダは、旧キーで署名したアプリを起動しません。そのアプリは、新規キーで署名して再書込するまで起動しません。

  stage0が起動するのは、ログが指定するコアだけです。起動回数が残存する待機のコアか、それ以外は確定済のコアです。待機のコアが検証不可能な場合（破損したスロット、コアのないスロットを指定する記録、または検証不可能な署名）は確定済のコアを選択し、それも検証不可能な場合や、起動可能なコアをログが指定しない場合は、確定済のアプリを起動します（1.2）。記録のないスロットを試行することはありません。そのアプリも不在の場合は、何も起動せずにROMローダでの復旧を待機し、その理由をノードの診断の出力に通知します。定義の`[boot]`にLEDと点滅する停止のパターンがあるボードでは、LEDが理由の番号（ヘッダが1、ヘッダのダイジェストが2、サイズが3、イメージの種類・マイコン・スロットが4、ロードアドレスが5、本体のダイジェストが6、署名が7、起動可能なコアをログが指定しない場合が8）の回数だけ、そのパターンの5倍の速度で点滅し、1周期の間消灯し、それを反復します（そのパターンの周期は10 ms以上です。周期がそれより短時間のボードは読込時に拒否します）。ボードのないノードや、そのLEDがないボード、停止のパターンが消灯のボードでは何も表示されません。どちらでもstage0の判断は同一です。

  stage0のないself-bootのマイコンでは、ブートローダがルートで、書込前にホストが、書込の経路で書込時はエージェントが検証します。
- **stage0の上限。** マイコン定義の`[stage0] region_bytes`（`[layout] stage0`のサイズ）はstage0の全体です（ベクタテーブル、ログの読込側、検証器を使用する検査、試行のセル、診断の出力）。イメージが領域の1/16を未使用領域として確保しないと、リンカスクリプトがリンクを失敗させます。RP2040: boot2（ブートROMの256バイトの第2段階）を包含して16 KiB。STM32F401RE: 16 KiB、コアがリセットベクタを取得するセクタ0の全部（リセットベクタを包含する最小の消去単位なので、どの書込もstage0を消去しません）。`scripts/stage0_size.py`は、ボードのないビルドと、定義がそのマイコンを指定するボードごとに1個をビルドします。ボードは診断の出力をstage0に追加するからです。さらに、イメージの部分（検証器、SHA-2、dagon-core、dagon-format、マイコンのコード、その残余）のそれぞれを、パッケージの`stage0/budget.toml`が付与する最大のバイト数とリンカのマップで比較します。stage0に現場で書込することはありませんが、新規ノードごとにそのビルドのツールチェーンでビルドします。そのため、余裕のないイメージは、ビルド不可能な最初の書込になります。どちらのstage0もコンパクトなSHA-256のラウンドを使用します。展開したラウンドはRP2040の領域に収容不可能なためです。最初の公開リリース以降、stage0の領域は固定です。領域を拡大したstage0は新規の形式番号で、既存のノードはROMローダによる最初の書込（直接書込）で移行します。通常の書込経路はstage0の書換の対象外のためです（電断時の安全性）。
- **stage0のクロックと検証時間。** stage0は、定義がノードに選択するクロックで検査します。マイコン定義の`[[stage0.clock]]`のうちボードの`[clock] source`に一致する設定（`out_hz`が最大のもの）で、一致する設定がなければ（ボードのないノード、`[clock]`のないボード、どの設定にも一致しない供給元）マイコンの内部の発振器です。ノードのstage0のビルドが、選択した設定をstage0に書込します。stage0はコアの起動前に、クロックをチップの起動時の状態に復元します。そのため、どのボードでもブートローダは同一のクロックから開始します。診断の出力と試行のウォッチドッグは、それぞれが動作するクロックで時間を計測します。`Ed25519Small`による署名の検査は、Cortex-M0+で約5,000万コアサイクルです（`crates/dagon-ed25519/tests/m0_cycles.rs`がARMv6-Mのインタプリタで、フラッシュの待機なしで実行し、5,500万を超過すると失敗します）。RP2040では、公称6.5MHzのリングオシレータで約7.7秒（その1.8から12MHzの幅で4.2秒から28秒）、ボードの12MHzの水晶から125MHzで約0.4秒です。ブートローダも同一の`Ed25519Small`で署名を検証し、SHA-2はコンパクトな手順で計算します。書込のセッションの領域を確保するためです。ブートローダは毎回のブートで起動するアプリのヘッダを検証するので、ブートごとにこの検査が1回追加されます。検査の各段階では、ウォッチドッグをキックします。`verify-time`の機能を付与してビルドしたstage0は、この時間をノード上で計測します。検査のそれぞれをコアのクロックのサイクルで計測し（SysTick）、最長の値を`DAGON_VERIFY_CYCLES`に、その回数を`DAGON_VERIFY_COUNT`に記録してデバッグプローブから読込可能にし、最長の値を0.1秒単位で切上げて診断の出力で点滅回数として提示し、コアは起動しません。計測後は、そのノードに通常のstage0を再書込します。ブートローダとアプリも同様にクロックを取得します。マイコン定義の`[[bootcore.clock]]`と`[[app.clock]]`のうち、ボードの`[clock] source`に一致する設定で、それぞれのHALが選択した設定を構成します。ボードが記述していない水晶を前提にする段階はありません。RP2040では水晶だけがアプリとブートローダにUSBのクロックを付与するので、上りのリンクがUSBのノードは、水晶のないボードではビルド不可能で、そのボードのブートローダは、ボードのDAGONのI2CとUARTのバスでだけ受信します。構成の確認（V-26）は、ノードごとにアプリが動作するクロックからUARTの速度を算出し、定義が許容誤差を記述していない内部の発振器で動作するUARTリンクを拒否します。
- **試行のウォッチドッグの下で消去しない。** stage0はフラッシュに書込しません。ブートローダは記録（BOOT_ATTEMPT、ROLLBACK、ブートローダのCONFIRMEDかROLLBACK）を、ログに既存の領域にだけ書込します（`Log::append_in_place`）。エージェントは、イメージを待機にする時点で、その試行用の領域を予約し（`Log::reserve`、`trial_room`）、電断で記録が途中で途切れた後は、アプリのエージェントが起動時にその領域を復元します（`SlotStore::keep_room`）。消去はアプリの内部だけで、定義の確認が十分と証明したウォッチドッグの周期の下で実施します。`[watchdog]`の両方の周期は、`[execution]
  erase_regions`のどの領域の`unit_max_ms`と比較しても2倍以上です。
- **試行のセル。** 試行のセルが書込値を保持しないノード（`bootcore::cell_holds`、アプリのストアとブートローダが検証します）は、ブートローダを受信しません（XFERをunsupportedとして拒否します）。セルの破損を検出した待機中のコアは自身のロールバックを記録します。そのため、破損したコアを回数の制限なしで再試行することはありません。

- **ルートイメージ**（あればstage0、なければブートローダ）は、準備時にマイコンのROMローダで書込します。ROMローダはDAGONの要素を何も検証しません。ROMローダを実行可能な者はルートを置換可能で、同時に全規則を置換可能です。そのため、このチェーンが成立するのは、DAGONが提供する全書込経路（リンク、UDSのような置換した転送、コーディネータのリレー）に対してで、ROM経由の物理的な再書込に対してではありません。マイコンがルートイメージの保護を保持する場合は、定義がそのレベルを列挙し（`[isolation]
  root_protection`: STM32では、`rdp1`は全消去で解除可能、`rdp2`は解除不可能で、セクタの書込保護`wrp`もあります。RP2040には何もなく、ブートROMは常にBOOTSELに従います。ESP32-S3にはセキュアブートとフラッシュの暗号化のeFuseがあります）、`dagon provision`がレベルの設定を提案して解除可能なレベルを明示し、VERSIONが正確なレベルと保護されたセクタを報告します（0x07）。そのためホストは、ルートが置換可能なノード、解除可能な形式で保護されたノード、解除不可能な形式で保護されたノードを明示します。

### 4.8 アプリ、リレー、ホスト

サンドボックスのアプリの拡張（レベル`mpu`/`pms`/`tz`）は、ハンドラについて4.1–4.4の保証を保持します。レベル`none`のアプリの拡張が保持するのは、APIの形式の保証だけです。

| ルール | 強制するもの |
|---|---|
| ストアにアクセスしない | 拡張は、自身のリージョンにコピーされた読取専用のペイロードを受信します。アプリケーションは`ImageView`を取得します（2.3）。スロットのストアの変更メソッドには、イメージのライフサイクルの状態機械経由でだけ到達します。 |
| 検証と確定の順序 | 状態機械の遷移だけが状態を変更します。`self_check`に照会するのは、「試行中」のガード通過後、`confirmed`の遷移内だけです。`accept_update`はDAGONのガードの後だけです。拡張の応答は検証された`enum`（`RETURN`）で、DAGONが実行するクロージャではありません。プロトコルの拡張はトリガーを作成し、セッションの状態機械がガードを実行します。 |
| 時間 | 判断の上限は3に記載しています。通知にも同一の上限があり、ハードウェアのレベルでは割込、`none`では計測です。 |
| ウォッチドッグ | どのサービスもアクセスしません。 |
| 再起動 | 3.2: 遅延に上限があり、拒否不可能です。 |

リレーの拡張はアプリの拡張です。ホストの拡張は利用者の権限で利用者のOS内で動作します。データで定義するもの（アクション、エージェント、プログラムで指定する転送）は、共有するcommand転送の上限のもとでプロセスの外部で動作します。ホストの拡張は判断に応答しません（3.4）。

### 4.9 適合試験のケース

マイコン定義のコードによる適合試験のビルドは、デバイス上の拡張から以下の操作を実行し、記録された結果を期待します。

- 定義の各特権のブロックへの書込（フォールト）
- DAGONのコードの読出（フォールト）
- DAGONのコードへのジャンプ（フォールト）
- 偽造したサービスのid（フォールト）
- リージョン外のサービスのポインタとオーバーフローするレングス（拒否）
- サービス呼出前の不正な非特権のスタックポインタ（種類の文書にある結果: スタッキングのエラーとアンワインド、またはロックアップ、ウォッチドッグによるリセット、ヒューズのセットと次回ブートでの拡張の無効化）
- 割当時間を超過するループ（超過）
- 宣言されたペリフェラルからの割込の嵐（割込元の無効化）
- ウィンドウ化不可能なウィンドウ経由のDMAの要求（生成時に拒否）
- ネストしたフォールト（アンワインド1回）
- 同一の呼出のタイマとフォールト（アンワインド1回）
- マイコンにある別コアの起動の試行（フォールト）
- `pms`のマイコンではWorld 1からのGDMAのアクセス（拒否）
- サービスについて: 4.2にある取消の境界のケースと、リセット時の規約のケース（各プリエンプト不可能な部分の内部でのウォッチドッグによるリセットの後、最初の使用が成功する）
- ヒューズについて: そのモデルの要件（5.5）のシナリオをデバイスで実行します

ケースは定義から生成するので（特権のブロックごと、コアごと、バスマスタごと、サービスの取消の境界ごとに1個）、新規マイコンはその定義からケースを取得します。

各ケースは実行可能なモデル（5.5）のシナリオでもあり、ブートが拡張なしの場合と同一の判断に到達することを検証します。

### 4.10 同一モデルでのその他のアーキテクチャ

モデルのマイコン固有の部分は例外なく定義のデータで、それぞれトレイトの背後に小規模で有限な機構の集合があります。

| 部分 | 定義のデータ | 機構（それぞれモジュール1個） |
|---|---|---|
| 保護マップ | `[isolation] kind`とパラメータ、`[[peripheral]]`、`[[core]]` | `ProtectionKind`（4.4） |
| アンワインド | `[isolation.capabilities] unwind` | `cortex-m-exception`、`riscv-trap`、`xtensa-world`、`armv8m-tz` |
| サンドボックスのタイマ | `[isolation.capabilities] timer` | `systick`、`riscv-mtime`、`timer-group` |
| ウォッチドッグ | `[watchdog] kind, trial_ms, confirmed_ms` | 種類ごとの`WatchdogDriver`のポートの既定（`iwdg`、`rp-watchdog`、`rtc-wdt`など） |
| ブート記録の配置 | `[boot.record] kind, at, survives, rings, entropy`（entropy: 起動のたびに異なる起動時のワードの出所。ブート記録には必須） | `noinit-ram`、`retained-registers`（5.5） |
| ロックアップヒューズ | `[boot.record] fuse`、`[watchdog] flag`（なし、またはリセットをまたいで残存し、起動が確認するウォッチドッグによるリセットのフラグの、レジスタかマイコン定義のコードの出所） | ヒューズが使用可能なのはフラグがある場合だけです。それ以外は読込が使用不可能と報告します（5.5） |
| ルートの保護 | `[isolation] root_protection` | マイコンのファミリごと、`dagon provision`が適用します |
| キャッシュとTCM | `[isolation] cache, tcm` | ウィンドウの属性とバウンスバッファの配置（下記） |

最初の3個のマイコンにはないものの、同一のスキーマに収容可能な種類は以下のとおりです。

- **TrustZone-Mを搭載したARMv8-M**（Cortex-M23/M33/M55/M85）: 種類の表（4.4）の`armv8m-tz`の行が、その能力と保証の全部です。表以外にこのファミリに固有なのは、バスマスタがインターコネクトの帰属のユニットでSecureかNon-secureになることで、定義がマスタごとに列挙します（`bus_master_attribution = "secure" | "configurable"`）。拡張の入出力で使用する設定可能なマスタは、DAGONのDMAのサービスをSecureとして経由します。
- **RISC-V PMP/ePMP**（ESP32-C/Hシリーズ、RP2350のHazard3、その他多数）: 種類の表の`riscv-pmp`と`riscv-epmp`の行が、項目、アドレスモード、保証を提示します。表以外に固有なのは、ESP32-C/Hの部品がペリフェラルとGDMAのためのEspressifのPMSも保持することで、その定義はメモリの`riscv-pmp`と`esp-pms`のペリフェラルとDMAの制御を組合せます（2個を統合するモジュールの種類`riscv-pmp+esp-pms`）。
- **Cortex-M7とその他のキャッシュのあるコア**: 種類の表にある`armv7m-mpu`の行です。定義は`cache = "id-cache"`とTCMの範囲を明示します。生成器は能力のウィンドウをDevice（キャッシュなし）、コード、データ、スタックをNormalとマークし、DMAのバウンスバッファをキャッシュ対象外のリージョン（DTCMの範囲か、キャッシュなしの属性のMPUのリージョン）に配置します。DMAのサービスは、バッファがキャッシュ対象の場合は送信前にクリーンし、受信後に無効化します。`.ext.text`はフラッシュかITCMに配置可能で、アンワインドは変化しません。バウンスバッファやTCMのリージョンを配置しない生成器は、データキャッシュやTCMを保持する定義を、無視せず、`plan`で欄の名称を提示して拒否します。
- **DAGONが1個のコアを使用するデュアルコアのマイコン**: 他方のコアは`[[core]]`の下に列挙し、その起動とコア間のブロックは`core-control`です。マップは、種類がコアごとに設定する場合（`esp-pms`）は全コアに適用します。ユニットがコアごとで2個目のコアを起動しない場合は、検査器が起動のブロックを特権にすることを要求します。

どの種類も保証しない事項: 宣言された能力の物理的な効果（4.6）、特権のコードにあるDAGON自身の不具合、ROMローダが再書込可能な対象（4.7）。

## 5. ボードの値、アクションプログラム、共有の資源、VERSION、モデルで決定する形式

### 5.1 ボードの値の出所は定義で、マイコンのコードではありません

`dagon-board.toml`は`[boot]`のテーブルを保持します。`led`（ピンの名称かGPIOの番号）、`led_active = "high" | "low"`、`halt = { pattern = "blink" |
"code" | "off", period_ms }`、任意の`power_hold = { pin, level }`、`[[boot.action]]`の項目（5.2）です。そして、拡張の能力を照合するテーブル`[[pin]]`、`[[bus]]`、`[[peripheral]]`、`[[device]] { bus, addr, writable = [regs] }`も保持します。定義の読込側は未知のキーを拒否するので、これらのテーブルを使用する定義は、それらを保持する`definitions`の形式を明示します（6）。`dagon gen`はブートローダのビルドのために`boot_wiring.rs`を出力します（`LED`、`HALT`、`PROGRAM`、`RECORD`、拡張のチェーンの型とその保護マップ）。ボードの定義は、ブートローダのビルドが同様に読込します。マイコンのcrateはこれらの定数を受信し、自身ではピン、パターン、環境変数の既定を保持しません。

`dagon-mcu.toml`は、ボードではなくマイコンが決定する値を保持します。`[watchdog] trial_ms, confirmed_ms`、`[isolation]`（4.4）、`boot_capabilities`（5.2）、`[boot.record]`（5.5）です。

### 5.2 ブートのアクションプログラム: 最も単純な拡張の種類

LEDのパターンや電源保持の線だけが必要でコードが不要なボードのために、ボードの定義はDAGONが解釈する宣言的で有界のプログラムを保持します。チェーンのその他の要素と同一の拡張で（ボードの定義の位置）、アクションに使用可能なイベントを観測し、同一の規則で動作します。`WatchdogArmed`以降でだけ、宣言されたピン、バス、デバイスでだけ動作します。

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

操作の集合（`boot-program`形式1）は以下のとおりです。

| 操作 | 欄 | コスト | 補足 |
|---|---|---|---|
| `led` | `state: on \| off \| toggle` | ~0 | `[boot] led`のピン |
| `gpio` | `pin`、`level: high \| low \| input` | ~0 | ピンはボードのピンの表にあり、`boot = true`のマークが必要です。設定して無変更で維持します。宣言された`power_hold`のピンでは、保持のレベルだけを受理します |
| `delay` | `ms ≤ 20` | `ms` | イベントごとの合計 ≤ 50 ms |
| `bus_write` | `device`（`[[device]]`の項目）、`reg`（その`writable`の一覧にあるもの）、`bytes ≤ 8` | タイムアウト5 ms | マイコン定義が`boot_capabilities = ["i2c"]`を宣言する場合だけ。任意のアドレスやレジスタは検証が拒否します |

上限（`dagon gen`が検証し、インタプリタが再検証します）は、イベントごとに`ops ≤
32`、イベントごとに`delay total ≤ 50 ms`です（バスの操作はタイムアウトで計数します）。ループ、分岐、読出、入力の待機、フラッシュや状態へのアクセス、クロックやウォッチドッグの制御はありません。未知の操作を検出したブートローダは、プログラム全体を無効として扱います（`boot.program_skipped`）。

### 5.3 共有の資源

複数の主体が必要とするボードの資源は、所有物ではなく共有です。

- ボードの定義が起動処理と復旧に必要とし、アプリが自身のデバイスに必要とするバスは、`pub static SYS_I2C:
  Shared<I2c<'static, Blocking>>`（`dagon_events::Shared<T>`。`try_lock`で取得する`embassy_sync::mutex::Mutex`）として公開し、`with(|bus| ..)`で使用します。これは貸出で、クリティカルセクションではありません。貸出の間、割込は許可のままです。所要時間に上限のない操作を割込をマスクして実行せず（実行モデル）、バスのドライバが自身の割込を待機する場合があるからです。ネストや並行の`with`は、待機せずにビジーを返却します。DAGONの起動処理と復旧は`with`で取得し、アプリも常に同様にします。サンドボックスの拡張は、宣言したデバイスでバスのポートの呼出を経由して到達します。各デバイスはボードの定義の`[[device]]`で、書込可能なレジスタと`max_transfer`（デバイスのデータシートから）を保持します。ポートはその他のレジスタや、上限を超過する転送を拒否します。ボードの定義はデバイスの定数とレジスタのレベルの関数を公開するので、アプリは何もコピーしません。
- ボードの起動処理は、`app.bring_up`でのボードの定義の拡張です（`BoardBringUp`のポート）。利用者のステップは、同一イベントでのチェーンの後方の拡張か、`Latest.phase ==
  Serving`を待機するタスクです。
- ボードの定義が保持するのはボードで、その上に搭載するものではありません。搭載するペリフェラルのドライバは、そのドライバを使用するアプリ側にあり、ボードの定義はバスのピンを単なるペリフェラルとして受渡します。

### 5.4 VERSIONの照会

コマンド`Cmd::Version`で、リンクの規約の一部です。実装していないノード（復旧用のエージェント、最小のイメージ）は、未知のコマンドの規則で`E_UNSUPPORTED`を返却し、ホストはそれを"versions
unknown"と表示します。リクエスト: 要約の集合なら欄はなく、ブート記録のページなら必須の欄0x12（`offset u16 LE, len u8`）を付与します。ブート記録を保持しないノードはその欄を認識しないので、番号を添えて`E_FIELD`を返却します。レスポンス: 値ごとに1個の欄で、形式は全ペイロードと同一です（`key, len, bytes`。keyは以下の番号の2倍で、番号はレスポンス内で増加します）。読込側は未知の番号をスキップします。デバイスが決定不可能な値は省略し、番兵の値は使用しません。各値は最大255バイトで、レスポンス全体が最小のリンクペイロードに収容されます（10）。

| 番号 | 値 |
|---|---|
| 0x01 | ブートローダのバージョン（`Version`、LE u32） |
| 0x02 | インストールしたブートイメージの識別: そのSHA-256の最初の8バイト |
| 0x03 | エージェント（`dagon-agent`）のバージョン |
| 0x04 | マイコンの定義のバージョン |
| 0x05 | ボードの定義のバージョン |
| 0x06 | マイコンの識別（5.5）: イメージのヘッダの`mcu_id`、u16 LE |
| 0x07 | 隔離（4.5）: `level u8`、`kind u8`、`boot_extensions u8`、`root_protection`（定義が列挙する種類と正確なレベル。例えば`rdp1`/`rdp2`、書込保護されたセクタ、レベルの解除可否）、`fuse`（Clearか、モデルの符号化による無効化した拡張の識別を伴うTripped、5.5） |
| 0x08 | 拡張、表示用: パック済の配列で、リンクされた拡張ごとに`id`（32バイト、0でパディング）、`trusted u8`、ソースハッシュの最初の8バイト（4.7） |
| 0x09 | 拡張、識別用: パック済の配列で、信頼された拡張ごとに32バイトのソースハッシュ全体（4.7） |
| 0x0A | stage0のバージョン: stage0が領域に保持するバージョンのマーク（`dagon_format::stage0`。`Version`、LE u32）。マークのないstage0とvendor-stage2のマイコンでは省略します |
| 0x0B | ブートローダのスロットA、B: `active u8`（稼働中のスロット）、次にスロットごとに`state u8`（STATUSのスロットの状態）と`version`（LE u32） |
| 0x0C | アプリのスロットA、B、factory。0x0Bと同一の形式です |
| 0x0D | エージェントが通信に使用するリンクのバージョン（`PROTO_VERSION`、u8） |
| 0x10 | 形式: `(format id u8, number u8)`の組: wire、image-header、state-log、event、definitions、boot-program、boot-record、extension-manifest |
| 0x11 | ブート記録の要約（5.5）: 記録に存在するブートと、最新のブートの判断と理由。レイアウトはモデルが決定します |
| 0x12 | ブート記録の未加工の領域から1ページ: `offset u16`と、その後に`len`バイト（`len ≤ RECORD_PAGE`、10）。ホストは領域をページごとに読込し、モデルの読込側で解読します |

稼働中のブートローダは、アプリのコンパイル時の対応物ではなく、フラッシュから識別します。

- **ブートローダのスロットとインラインのヘッダを保持するself-boot**: アプリはブートローダのログを読込してどのブートローダのスロットが起動したかを把握し、次にそのスロットのヘッダ（0x01）を読込し、イメージをハッシュします（0x02）。
- **トレーラのヘッダを保持するself-boot**: アプリはトレーラ（0x01）を読込し、本体をハッシュします（0x02）。
- **vendor-stage2**: インストールされたブートローダにDAGONのヘッダはありません。アプリはベンダーのブートローダのオフセットでフラッシュから読込し、ベンダーイメージのセグメント表を走査してレングスを決定し、ハッシュし（0x02）、0x01を省略します。ホストは0x02を、準備時に記録した識別（`host.provision.recorded`、プロジェクトに保存）と、マイコン定義のベンダーの表と照合し、バージョンか"unknown vendor stage (hash …)"を表示します。

`dagon status`は、ノードごとに1行で、これらの値、隔離のレベル（"guarded" / "unguarded extensions"）、信頼された拡張、最後のブートの要約を出力します。`dagon doctor`は形式番号を`compat.toml`と比較し、CLIが読込しない番号と、レベル`none`のノードにリンクされた信頼されていない拡張について警告します。

### 5.5 ブート記録、マイコンの識別、コンパイル済の構成: 要件と実行可能なモデル

このモデルの3個の形式は、リセット、電断、部分的な書込、バージョンの不一致のもとでのバイト単位の動作に正当性が依存する、保持や永続のデータに関係します。保持メモリのブート記録、その隣のロックアップヒューズ、イメージのヘッダ、識別の返答、VERSION、コンパイル済構成のノードのレイアウトが保持するマイコンの識別です。そのような形式が故障のどんな組合せにも耐えることを、文章では証明不可能です。実行可能なモデルなら証明可能です。そのため、これらの形式はこの文書では規定しません。ランダムな故障の注入を伴うホスト側のRustのモデルとして設計し、性質に合格した形式が各形式の成文の仕様になり、モデルのテストを証明として公開します。モデルはサンドボックスの観測可能な規約（ブートのどのステップでも超過、フォールト、能力の誤用をする拡張）も、ブートの状態機械の継続に対して試験します。

この節は、それらの形式が満たすべき要件を提示します。モデルを受理するのは、以下の全性質が故障の注入のもとで成立する場合だけです。合格した形式はモデルの性質の一覧と各主張の限界とともに、[retained-formats.md](/explanation/retained-formats/)で規定します。

#### ブート記録

- **観測可能性。** 全ブート段階（stage0、ブートローダ）の全ブートの遷移を記録し、アプリのエージェントが、起動時に破損していない記録済のイベントを全部、カタログのイベントとして、`replayed: true`と、ブート、書込側、位置を区別する識別子を付与してリプレイします。停止した、ハングした、アプリに未到達のブートは、その後に最初に動作するアプリがリプレイします。イベントの欠落（容量）もそれ自体を記録してリプレイします。
- **途中で途切れた書込への安全。** どのバイトで中断された書込も、以前に有効だったスロットの上書きの場合も同様に、失うのは最大で書込中のイベントで、読込側が破損なしと受理する別のイベントを生成しません。欄をその場で変更することはなく、読込側が受理するものが2回の書込の産物であることはありません。
- **書込側の独立。** 書込側は書込の位置を、マイコン定義の定数と自身が検証したデータだけから導出します。その他の書込側の内容を解釈しないので、段階は他方の前後を問わず、どの順でも新版に置換可能で、不正な保持メモリが書込を記録の外に誘導することはありません。識別子はどちらの順で段階を新版に置換しても一意で、それ以前に何も書込していない最初の段階の場合も一意です。
- **順序。** ブートはシーケンスの一周をまたいで時系列順にリプレイします。1個の書込側がシーケンスの空間の半分を超過する期間沈黙した場合も対象です。読込側は後のブートを前のブートより先行して報告しません。順序付け不可能な組は、不確実とマークします。両方のイベントがローカルにある間、識別子が反復することはありません。
- **判断の入力にならないこと。**`plan`、`commit`、`self_check`、`confirm`がこの記録を読込することはありません（4.1）。モデルのAPIは、ブート段階に`append`だけを、エージェントに`events`だけを付与します。拡張は到達不可能です（拒否されたリージョン）。
- **バージョン管理。** 保持のレイアウトは、読込側が読込不可能なレイアウトを拒否するために必要な情報を保持します（6）。旧版か新版のレイアウトで記録した保持のバイトを、読込側のレイアウトの有効なイベントとして受理することはありません。
- **サイズ。** 領域は最大`RECORD_MAX`バイトで、`RECORD_PAGE(link)`バイトのページで読込します（10）。位置と書込側ごとのサイズは、マイコン定義の`[boot.record]`で宣言し、1個の経路（マイコンのcrateのビルドスクリプト）で全イメージの`memory.x`に反映されます。イメージの一致をテストが検証します。
- **リプレイの意味。** 最低1回です。記録にはリプレイ済の内容がなく、受信側は識別子で重複を除去し、エージェントは1回の実行で同一の識別子を2回リプレイしません。
- **状態ログからの独立。**`app.trial_started`と`app.rolled_back`は、記録にかかわらず状態ログに由来します。

#### ロックアップヒューズ

- **所有者。** ヒューズに書込するのは、サンドボックスの呼出を実行しようとしているレイヤー（ブートローダ、アプリ、リレー）だけで、読込するのは全レイヤーの起動です。その他のコードは読込も書込もせず、ブートの判断、確定の規則、自己検査の入力にはなりません。
- **識別。** ヒューズはイメージ（スロットとイメージのヘッダのダイジェスト）、拡張（イメージのチェーンの位置と、成果物のハッシュ全体）、レイヤーを明示し、どのイメージのどの拡張のその他のビルドとも一致しないほど完全です。モデルは衝突が発生しないことを構造として試験し、符号化が拡張やスロットの数に上限を設けることはありません。
- **誤ったトリップの不在。** 有効化、呼出、復帰、クリアのどの時点のリセットも、復帰済の呼出を指定するヒューズを作成しません。ロックアップした呼出内のリセットは、常にその呼出を指定するヒューズを作成します（リセットをまたいだ残存はマイコン定義の宣言のとおりで、電断はヒューズを失う場合がありますが、偽造はしません）。
- **引渡。** アプリやリレーの呼出が有効にしたヒューズは、ブートローダの起動を無変更で通過し（ブートローダはそのヒューズを記録し、その他のレイヤーのヒューズをクリアせず、そのヒューズを根拠に動作しません）、アプリの起動が読込し、指定された拡張をその実行の間無効にします。ブートローダの呼出が有効にしたヒューズは、ブートローダの次回起動がそのヒューズを根拠に動作します。
- **失効と新版。** 識別がインストール済のイメージのどの拡張とも一致しないヒューズ（イメージか拡張が置換された）は、失効として記録し、そのヒューズを根拠に動作すべきレイヤーがクリアします。その他のレイヤーはクリアしません。成果物のハッシュが変化した拡張は新規の識別で、再度動作します。
- **クリア。** DAGONには、ヒューズを消去する要求はありません。プロトコルにもCLIにもないので、識別が存在するヒューズはトリップしたままです。retained-formats.mdの3.6は、形式のモデルのクリアです。
- **バージョン管理。** ヒューズは、読込側が読込不可能なレイアウトを拒否するために必要な情報を保持します（6）。別のレイアウトのバイトを根拠に動作することはありません。
- **サイズと配置。** マイコン定義のブート記録の隣で宣言し（`[boot.record] fuse`）、同一のリンカの経路で全イメージに反映されます（イメージの一致を検証するテストが1個）。
- **ウォッチドッグのフラグ。** ヒューズには、電源投入以外の全リセットをまたいで残存し、起動の確認時だけクリアされるウォッチドッグによるリセットのフラグが必要です。マイコン定義は、マイコンがそのフラグを保持するかどうかと出所（ハードウェアのレジスタか、マイコン定義のコードが用意するフラグ）を宣言します。フラグがないと、そのマイコンではヒューズを使用不可能で、定義の読込がその旨を明示します。

#### マイコンの識別

- マイコンは手動で割当てた番号ではなく、定義の`id`から導出した値で識別します。マイコンの追加は定義の追加で、表の編集ではなく、数値のマイコンの表はどこにもありません（テストがソースを調査します）。
- 識別は一意です。定義の読込は識別を保持するどの欄の幅でも衝突する2個の定義を、両方のidを提示して拒否します。読込したどの定義とも一致しない識別のデバイスは不明と表示し、別の定義に一致させることはありません。
- イメージのヘッダ、識別の返答、VERSIONは、ホストとコーディネータにとって曖昧ではないとモデルが証明する形式でこの識別を保持します。位置ごとの幅とハッシュ関数は、モデルの出力です。

#### コンパイル済の構成

- 本体の形式（`topology-bin`）は、リリースのパッケージに格納され、ホストが読込します。ノードは構成を保存しません。形式は[retained-formats.md](/explanation/retained-formats/)の5で規定します。

#### 文章で規定する形式

この文書のその他のバイト単位の構造は、以下のいずれかです。ビルド時に生成してデバイスへの到達前に検証するもの（ブートのアクションプログラムの表、`boot_wiring.rs`の定数、拡張の保護マップ）、永続を伴わないリクエストとレスポンスの符号化（VERSIONの欄、serveのJSONのメッセージ）、このモデルが変更しない既存の規約（イメージのヘッダの署名とダイジェストの欄、状態ログのスロットの記録、ベンダーの選択データ、リリースのバンドル）です。いずれも2回の書込も、故障後の再読出もないので、モデルは不要です。モデルのハーネスが、VERSIONのページの読出を全リンクのサイズで試験することはあります。状態ログのキー付記録と、その記録が収容するリンク診断の記録は、モデルの対象です（[保持と永続の形式](/explanation/retained-formats/)の8節）。

## 6. バージョン管理の規則

番号と部品は[バージョンとリリース](/explanation/versioning/)に従い、`compat.toml`に登録します。このモデルに固有の規則は以下のとおりです。

| 規約 | 番号の変更時期 | 読込側の規則 |
|---|---|---|
| `event`（`EVENT_FORMAT`） | 種類の名称の変更、削除、またはペイロードの意味の変更時。種類やペイロードのフィールドの追加では不変です | 読込側は未知の種類とフィールドをスキップし、読込不可能な番号を拒否します |
| `extension-manifest` | マニフェストにあるキーの意味の変更時、または読込側が拒否するキーの追加時 | `dagon gen`は読込不可能な番号のマニフェストを、両方の番号を提示して拒否します |
| `boot-program` | 操作の追加、削除、またはその欄の変更時 | 未知の操作を検出したブートローダはプログラム全体を無効として扱い、`boot.program_skipped`を発行します。`dagon gen`は、番号を読込不可能なブートローダのためにプログラムを生成しません |
| `image-header`、`topology-bin` | マイコンの識別の欄やノードのレイアウトの変更時（5.5） | ブートローダとエージェントは`compat.toml`が列挙する`image-header`の番号を、ホストは`topology-bin`の番号を受理し、その他の番号を拒否します。ノードは`topology-bin`を読込しません |
| `definitions` | 読込側が拒否するキーの追加時（読込側は未知のキーを拒否します）、またはキーの意味の変更時 | 定義は必要な番号を明示します。CLIは対応する全番号を読込し、拒否する場合は両方の番号を提示します |
| `wire` | コマンドのリクエストまたはレスポンスの意味の変更時。欄とコマンドの追加では不変です。デバイスが要求外のフレームを送信することはありません（リンクはリレー経由のリクエストとレスポンスです） | 受信側は、未知の必須の欄に、その番号を明示する`E_FIELD`で応答し、未知の任意の欄（レスポンスの欄はすべて任意）を無視します。未知のコマンドには`E_UNSUPPORTED`で応答し、ホストはそれを不明と表示します |
| `serve-api` | 追加の`event`のメッセージでは不変です | 受信側は未知のメッセージの型をスキップします |

番号の加算は、旧番号の読込側が新データを誤読する変更、または拒否すべき変更の場合だけです（[バージョンとリリース](/explanation/versioning/)）。上の表は、この規則の各規約への適用です。

部品の種類: crateの公開APIの互換性のない変更は、その部品の破壊的な変更です。既定を保持する新規のイベント、ポート、操作、VERSIONの欄、定義のキーは機能の追加です。その他は一般の規則に従います。ポートのトレイトがメソッドを追加するのは既定がある場合だけです。ポートのメソッドの削除または変更は、レイヤーの部品の破壊的な変更です。

廃止: このモデルが置換する公開のトレイトや関数（ストアのアクセサ、起動の引数）は、その部品でマイナーリリース1回の間、置換先を明示する`#[deprecated]`を付与して残存させ、次回の破壊的なリリースで削除します。`CATALOG`の項目は1個の形式番号の範囲内では削除しません。発行を終了した種類は、次回の`event`の番号まで`retired`のマークを付与してカタログに残存させます。

## 7. アプリへのDAGONの導入と除去

ブートローダが要求するのは、このマイコン（その識別で、5.5）、ターゲットのタグ、スロットの署名付きのヘッダを保持するスロット、ダイジェストが一致する本体、本体の先頭のベクタテーブル、そしてブートローダが有効にしたウォッチドッグをキックするアプリです（試行では: 試行の周期内で確定するか、ロールバックされます）。

DAGONをリンクしないアプリも、ROMの経路でインストール可能で、署名の後に動作します。確定はしないので、`dagon provision`が確定としてインストールしない限り、`max_attempts`の後にロールバックします。

DAGONの導入手順: マイコンとボードの定義を追加し、拡張のチェーン（`Nop`も有効です）で`start`を呼出し、返却される`Reader`で公開のイベントを読込し、ガード経由でキックを継続します。

除去の手順: `start`の呼出を中止します。ブートローダは継続して検証と試行の計数を実施し、自身の拡張を実行し、ブート記録に書込します。ホストは識別の返答を受信せず、`dagon status`がその旨を表示します。

宣言した信頼された拡張（4.7）は例外で、それ以外のどの拡張、イベント、定義、ブートのアクションも、ブートローダの検証対象を変更しません。

## 8. 例

### 8.1 起動画面と書込の進捗のディスプレイ表示（アプリ）

DAGONは画面を描画しません。アプリには、描画用のデータだけを受渡します。ノードのイベントと最新の`AppSnapshot`（段階`phase`、実行中のバージョン`version`、受信したバイト数`progress`、ホストが通知した書込の%`percent`、試行中かどうか`trial`）と、`dagon gen`が出力する`wiring.rs`のノード自身の項目`NODE`およびリンクで到達するノードの一覧`REACHED`（番号、名称、リンク、アドレス）です。以下の例は、そのデータで描画するアプリです。

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
//       // I2C_ADDR: the node's I2C address from its wiring (LINK_ADDR)
//       let events = node::start::<Board, N>(spawner, dagon, &GUARD, tag, app, UPLINK, I2C_ADDR)?;
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

宣言されたI2Cデバイスの背後にある電源管理で、マイコン定義が`i2c`のブートの能力を宣言し、ボードがそのデバイスを宣言している場合は、以下のように記述します。

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

### 8.3 ブートを包含する全イベントのログ（アプリかリレー）

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

または、応答のタスクの時間に影響しない非同期の方法もあります。`Reader`から`events.recent()`を取得するタスクです（所有された`EventRecord`、2.4）。

### 8.4 書込の転送とプロトコルの置換: CAN上のUDS

車載のノードには、シリアルリンク上のDAGONのリンクではなく、ISO-TP（ISO 15765-2）を使用するCAN上のUDS（ISO 14229）で通信する診断テスタが書込します。1個の拡張が2個のポートを提供し、ブート時とアプリで動作します。

マニフェストは以下のとおりです。

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

ブート段階では、ブートローダは拡張を`.ext.*`にリンクし、生成されたマップ（4.4）が、マイコン定義でウィンドウ化可能なCAN1のレジスタブロックと、拡張自身のメモリを付与します。フラッシュコントローラ、状態ログ、ウォッチドッグ、DAGONのRAMは拒否します。`WatchdogArmed`以降、ブートの状態機械は、ステップの間に限定的な時間枠（プロジェクトの`[boot] service_window_ms`。試行の周期が上限です）の間`UpdateTransport`のポートをポーリングします。`DiagnosticSessionControl(programmingSession)`を送信するテスタはブートローダを復旧の経路に維持し（`boot.recovery { path: Service }`）、拡張の`UpdateProtocol`が以下のとおり対応付けます。`RequestDownload` → `update_offered`、`TransferData` → `header_chunk`/`data_chunk`、`RequestTransferExit` → `transfer_end`、`RoutineControl(switchSlot)` → `switch`、`ECUReset` → `reboot`です。セッションの状態機械は、DAGONのリンクと完全に同様に、トリガーごとにDAGONの全ガードと検証器のポートを実行します。そのため、テスタが署名をスキップすることも、稼働中のスロットに書込することも不可能です。拡張の`Diagnostics`のポートは、宣言した識別子について`ReadDataByIdentifier`に応答します（VERSIONが保持する値と同一の値に由来するバージョンと隔離のレベル）。各ハンドラの呼出は`max_run_us`の制限下で動作します。停止したCANコントローラは超過のイベントになり、ブートは時間枠なしで判断に進行します。

アプリの段階では、同一のcrateが、アプリのチェーン内で、同一のポートをエージェントに提供します。応答のタスクが`UpdateTransport`からフレームを読込し（ISO-TPの組立は拡張の内部）、`UpdateProtocol`がUDSのサービスをセッションのトリガーに変換し、エージェントの返答が同一の経路で返送されます。`accept_update`と`self_check`は、ボードの定義、ライブラリ、アプリが担当します（ここでは`decides = []`）。レベル`mpu`では、ハンドラはCANのウィンドウだけをペリフェラルとして非特権で動作します。レベル`none`では同一のコードが動作し、`dagon status`がそのノードを"extensions unguarded"と表示します。

拡張がどのレベルでもAPI経由で実行不可能な操作は、スロットのストアやログへの到達（ハンドルがありません）、スロットの確定（宣言する判断ではありません）、ブートローダの起動対象の変更です。`mpu`/`pms`で、コードの内容にかかわらず実行不可能な操作は、フラッシュへの書込、ウォッチドッグへのアクセス、DAGONのメモリの読込、ブートのハングです。

### 8.5 デプロイの結果に対するホストのアクション

2.5の例と同様の`~/.dagon/actions/on-result.toml`です。その後`dagon apply`が、ノードごとに`host.node.result`を、プログラムの終了後に`host.action.ran`を発行します。同一のイベントが`/v1/events`と、`--events`でstdoutに出力されます。

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

## 9. 不採用の案

| 案 | 不採用の理由 |
|---|---|
| DAGONが解釈する固定の操作の集合だけをブート段階の拡張にする | ニーズには限りがなく（UDSのような診断の転送、追加のセキュリティ処理、ディスプレイ、別のストレージ）、新規ニーズのたびにDAGONの変更が必要になります。コード不要の用途のための最も単純な拡張の種類として残存させ、一般のニーズは、ハードウェアが強制するサンドボックス内で、ポートの背後のコードとして実現します。 |
| lint、依存の許可リスト、適合のスキャンを防壁にした、ブート時の任意のコード | それらが調査するのはソースで、実行時の動作ではありません。コールバックは、ウォッチドッグがチップをリセットするまでループ可能で、どのレジスタやフラッシュにも書込可能です。防壁はマイコンの保護ユニットとプリエンプトするタイマ（4.1）で、どちらもないマイコンでは、定義が`none`と宣言し、製品がその旨を明示します。 |
| 拡張が応答する、ブート時の拒否権 | `Boot`をロールバックに変更する信頼されていない応答は、故障した拡張が有効なイメージを永久に起動させない事態を許容します。ブートの方針はDAGONの`DecisionPolicy`のポートで、置換可能なのは宣言され、署名され、表示される信頼された拡張（4.7）だけです。 |
| スロットのストアの変更メソッドを公開し、エージェントをその利用者の1個にする | 永続のイメージの状態が、イベントも確定の規則もなしに変化する可能性があります。イメージのライフサイクルの状態機械が唯一の書込側です。 |
| 新規タイミングごとにメソッドを追加する固定のフックのトレイト | 合成がなく、ディスプレイのためのペイロードのストリームもなく、部分の置換もなく、想定外のタイミングのたびに一覧の修正が必要になります。 |
| 手動で保守するカタログと照合する、手動で保守するイベントの列挙型 | 2個の一覧が一致しても、遷移が何も発行しない可能性があります。イベントの発行なしでは状態を変更不可能な基本要素だけが、カタログを派生物にします。 |
| リンカのセクションの登録簿（`linkme`） | 項目の順序が不定で、ライブラリによる不可視の登録があり、`dyn`のディスパッチのコストとサイズが発生します。 |
| デバイスの`alloc`を使用する`dyn Extension`の一覧 | ホストより下層にallocはありません。静的なスライスは登録簿の問題を再び持込みます。 |
| 全部をチャネルにする | 判断とポートには同期の応答が必要です。キューが適切な道具なのは公開の状態だけです。 |
| 待機可能なリングに借用のペイロードを残存させる | 静的なリングは作成側の存続期間を超過して参照を保持不可能です。生成される所有された短縮形（2.4）は有界で、表示やログのタスクに十分です。 |
| デバイスからホストへの要求外のイベントのフレーム | リンクは、コーディネータがホップごとに中継するリクエストとレスポンスです。誰も要求していないフレームには、経路も、再試行の連番も、リレーの無通信時間モデルにおける位置もありません。`EVENT`のフレームのフラグは予約で、使用しません。ホストはデバイスの全遷移を、返答、VERSION、ブート記録から導出します。 |
| ビルド時の環境変数でのボードの値 | 定義にない、形式を変更したマイコンの知識です。ボードの定義の`[boot]`に置換しました。 |
| アプリがコンパイル時に対象としたブートローダのバージョンを報告する | インストール済の対象を識別しません。VERSIONはインストールされたイメージをフラッシュから読込します。 |
| 再起動の遅延に対する固定の上限 | 1回決定した数値は、試行のウォッチドッグの周期が短期のマイコンでは誤りです。上限はマイコン定義の周期から導出します。 |
| CRCで網羅したヘッダをその場で上書きするブート記録 | その場で上書きする回数、状態、CRCは、リセットに対してアトミックにすることが不可能です。途中で途切れた上書きは、ヘッダが網羅する全イベントを無効にします。その他の書込側のセクションをヘッダで探索する引渡は、書込側の形式を結合させます。 |
| 保持メモリと識別の形式を文章で規定する | リセット、部分的な書込、バージョンの不一致のあらゆる組合せに正当性が依存する形式の正当性を証明するのは、文章ではなく、ランダムな故障の注入を伴う実行可能なモデルです（5.5）。 |

## 10. サイズの上限とその出所

どの上限にも唯一の出所があり、その上限に収容されるべき箇所を全部列挙し、テストが各組を検証します。

| 上限 | 値 | 唯一の出所 | 収容されるべき対象 |
|---|---|---|---|
| 種類ごとのリンクのペイロード | USB 1088、UART 1024、I2C 256バイト | `dagon_proto::max_payload` | 全リクエストと返答、`RECORD_PAGE`、VERSIONのレスポンス、表示の文字列、適合試験の詳細 |
| 最小のリンクのペイロード | `max_payload::MIN` = 前述の種類の最小値（導出値で、記述値ではありません） | `dagon-proto` | VERSIONのレスポンス全体（デバイスが応答する全TLVの集合。最大の0x08の集合、0x11、0x12の全ページで検証します） |
| VERSIONの値 | ≤ 255バイト | VERSIONの符号化（`dagon-proto`のコマンドの表） | 5.4の全番号、0x12のページ |
| VERSION 0x12の固定の費用 | `VERSION_PAGE_OVERHEAD = 6` = ステータス(1) + TLVの数(1) + id (1) + len (1) + offset (2) | 欄の幅の合計との一致を`const`のアサーションで検証する`dagon-proto`の定数 | `RECORD_PAGE` |
| リンクごとの`RECORD_PAGE` | `RECORD_PAGE(link) = min(255 − 2, max_payload(link) − VERSION_PAGE_OVERHEAD)`: I2C 250、UART 253、USB 253。`RECORD_PAGE_MIN = 250` | リンクの種類ごとに`const`のアサーションを保持する`dagon-proto`の`const fn` | VERSION 0x12。ホストのページの読込側が要求するのは最大`RECORD_PAGE_MIN` |
| ブート記録の領域 | `bytes ≤ RECORD_MAX = 4096`（0x12の`offset u16`が全体を指定可能です）。内部の上限はモデルの定数です（5.5） | `dagon_format::bootrecord::RECORD_MAX`、マイコン定義の`[boot.record]` | 定義の検証、全イメージの`memory.x`、エージェントの読込側 |
| サービスのプリエンプト不可能な部分 | サービスと種類ごとの`np_max_us`、`COPY_MAX`、各`[[device]]`の`max_transfer`、`dma_stop_max_us` | ポートのカタログと種類の能力の記録（4.4）。実装で最悪の場合を測定した値と照合して検証します | サービスの実装、タイマのマスク、4.2の検証5 |
| 拡張の割当時間 | `max_run_us ≤ BOOT_EXT_MAX_US`、`≤ APP_EXT_MAX_US`。1回のブートで`Σ (calls × budget)` ≤ `trial_ms / 4` | マイコン定義の`[watchdog]`に由来する`dagon-events`の`const fn`、`boot_wiring.rs` | `dagon gen`の検証、サンドボックスのタイマ、マニフェスト |
| 拡張のメモリ | `stack_bytes + ram_bytes ≤`マイコン定義の`[isolation] ext_ram_max`。エントリ数は、コード、データ、スタックの分を除外した種類の上限以下 | マイコン定義、`boot_wiring.rs`の保護マップ | `dagon gen`の検証、`memory.x` |
| 拡張の数とid | id ≤ 32バイト。0x08は拡張ごとに反復し、VERSIONの返答全体が最小のリンクに収容されること | マニフェストの形式、`max_payload::MIN` | `dagon gen`は、VERSIONの返答が収容不可能なプロジェクトを拒否します |
| ブートのアクションプログラム | イベントごとに`ops ≤ 32`、`delay total ≤ 50 ms`、`bus_write`ごとに`bytes ≤ 8`、`delay ≤ 20 ms` | `dagon_format::bootprogram`の定数 | `dagon gen`の検証、ブートローダのインタプリタ、`boot_wiring.rs`の表の型 |
| liveのイベントのペイロード | ブートのイベントは ≤ 16バイト（値で）。それ以上のアプリやホストのペイロードは参照で | `dagon-events`のサイズのアサーション | `Published`のスナップショット、短縮形 |
| `EventRecord` | 24バイト: `layer`、`kind`、`flags`、`dropped`、`seq u32`、`payload [u8; 16]` | `state_machine!`の`compact`属性（生成されたテストが、全種類の保持を検証します） | `Recent<N>`。ブート記録の短縮形は、ペイロードの最初の3バイトを再利用します |
| カタログ | 数値の上限なし。`kind`はレイヤーごとの`u8` | `state_machine!` | 記録の`kind u8`（生成されたテストが、レイヤーの256種類で失敗します）、serveのストリーム |
| 最近のリング | レイヤーのcrateが選択する`N` | `Published<S, N>` | レイヤーのcrateの`Published`。溢れると`dropped`を計数します |
| 判断の上限 | 50 ms、`REBOOT_DELAY_MAX = min(trial_ms, confirmed_ms) / 2` | `dagon-events`の定数、マイコン定義の`[watchdog]` | 応答のタスクのタイマ、`boot_wiring.rs` |

## 11. バスマスタと特権の制御ブロック: 定義のスキーマ

全マイコン定義は、マイコンの全レジスタブロック（`[[peripheral]]`、4.4）を`window_eligible`、`bus_master`、`privileged_why`とともに列挙します。検査器は、種類が`none`以外で、必要な区分のブロックが欠落した定義や、バスマスタをウィンドウ化可能とマークした定義を拒否します。生成されるマップ（4.4）と適合試験のケース（4.9）はこの一覧から派生し、以下の内容をその他の位置で手動で保守することはありません。ここの3個の表は、配布する3個の定義の一覧で、スキーマが定義の作成者に要求する判断の例です。判断の内容は、ブロックがメモリに書込可能か、設計がそのブロックをどのように特権に維持するか、です。

**STM32F401RE**

| ブロック | メモリへの書込 | 特権に維持する手段 |
|---|---|---|
| DMA1、DMA2（`0x4002_6000`、`0x4002_6400`） | はい（バスマスタ） | `bus_master = true`、`privileged_why = "dma"`。ウィンドウには決してしません。拡張のDMAは、バウンスバッファを使用する`DMA_*`のサービス経由の場合だけです |
| USB OTG FS（`0x5000_0000`） | この部品ではシステムRAMへのDMAはありません（FIFOベース）が、DAGONのリンクです | `shared`。ウィンドウには決してしません。エージェントが保持します |
| フラッシュインターフェース（`0x4002_3C00`） | いいえ（フラッシュに書込します） | `flash` |
| IWDG、WWDG | いいえ | `watchdog` |
| RCC、PWR、RTC/バックアップ | いいえ | `clock`、`reset`、`power` |
| SYSCFG、EXTI、NVIC/SCB（PPB） | いいえ | `irq-routing`、`pinmux`。PPBはアーキテクチャで特権です |
| CRC | いいえ | `shared`（DAGONが使用します） |
| システムメモリ（ROMローダ） | いいえ | `rom`: 非特権モードから実行不可能です（バックグラウンドのマップ） |
| ボードでDAGONが使用しないUSART/SPI/I2C/CAN/TIM/ADC/GPIOのブロック | いいえ | 自己完結している場合は`window_eligible = true`。GPIOはサービスです（DAGONのLEDと共有のバンク） |

**RP2040**

| ブロック | メモリへの書込 | 特権に維持する手段 |
|---|---|---|
| DMA（`0x5000_0000`） | はい | `dma`、バスマスタ。サービスだけ |
| USB（`0x5010_0000`） | 自身のDPRAMにだけで、システムRAMには書込しません。DAGONのリンクです | `shared`。ウィンドウには決してしません |
| XIPの制御、SSI（`0x1400_0000`、`0x1800_0000`） | いいえ（SSI経由でフラッシュに書込します） | `flash` |
| ブートROM（`0x0000_0000`） | そのフラッシュのルーチンがSSIとDMAを操作します | `rom`: 非特権モードからのROMの実行は全体を拒否します。ルーチンのアドレスは防壁に無関係です |
| WATCHDOG | いいえ | `watchdog` |
| CLOCKS、RESETS、XOSC、PLL、ROSC、VREG | いいえ | `clock`、`reset`、`power` |
| PSM、SIOのFIFOとスピンロック | コア1の起動とコア間 | `core-control`。2個目のコアは起動しません |
| SIOのGPIOのout/oe | いいえ | `shared`（DAGONのLED）。GPIOはサービスです |
| IO_BANK0、IO_QSPI、PADS | いいえ | `pinmux` |
| PIO0、PIO1 | いいえ（FIFO。PIOのプログラムはシステムメモリにアクセス不可能です） | ボードがDAGONのリンクに使用しない場合は`window_eligible = true`。それ以外は`shared` |
| UART0/1、SPI0/1、I2C0/1、PWM、ADC、TIMER | いいえ | DAGONが使用しないブロックはウィンドウ化可能です。TIMERは`shared`です |
| コア1 | コア0のMPUに制約されない2個目のCPU | `[[core]]`。起動しません。その起動のブロックは特権です |

**ESP32-S3**

| ブロック | メモリへの書込 | 特権に維持する手段 |
|---|---|---|
| GDMA | はい。SPI2/3、I2S、UHCI、LCD_CAM、ADC、AES、SHAの代理で | `dma`、バスマスタ。GDMAのパーミッション制御がチャネルごとにWorld 1を拒否します。サービスだけ |
| USB OTG | はい（自身のDMA） | `shared`、バスマスタ。ウィンドウには決してしません |
| USB Serial/JTAG | DMAなし。DAGONのリンクです | `shared` |
| SPI0/SPI1（キャッシュとフラッシュ）、MMU | いいえ（フラッシュに書込し、メモリを割当てます） | `flash`、`protection` |
| AES、SHA、RSA、HMAC、Digital Signatureのエンジン | GDMA経由（AES/SHA） | `shared`/バスマスタ。`CryptoProvider`は信頼されたポートです（2.1） |
| RTC WDT、TIMG WDT、スーパーウォッチドッグ | いいえ | `watchdog` |
| RTC_CNTL、システムレジスタ、割込マトリクス、IO MUX、GPIOマトリクス | いいえ | `power`、`reset`、`clock`、`irq-routing`、`pinmux` |
| PMSとWCLのレジスタ | いいえ | `protection`。World 1への移行前にロックします |
| Wi-FiとBluetoothのMAC | はい（バスマスタ） | バスマスタ。DAGONは有効にしません。特権です |
| コア1 | 2個目のCPU | `[[core]]`。マップは両コアに適用します |
| DAGONが使用しないUART1/2、DMAなしのSPI2/3、I2C、TWAI、PWM、RMT、PCNT | いいえ | 自己完結している場合はウィンドウ化可能です。TWAI（CAN）はUDSの転送に適したウィンドウです |

## 12. 実行モデル

DAGON自身のコードについて、各処理の実行位置、DAGONのコードが割込をマスク可能な時間長、フラッシュの操作が停止させる対象を、マイコンごとに提示します。この節は規範です。マイコン定義のコードはこの節に従って記述され、マイコン定義はそのハードウェアの事実（`[execution]`、`[watchdog]`）を保持し、以下に列挙する確認がコードとデータをこの節に従わせます。

### 12.1 ルール

1. **処理はハンドラではなくタスクで実行します。** DAGONの応答のループ、リレー、ウォッチドッグへのキックは、エグゼキュータ上のタスクとして動作します。DAGON自身の割込ハンドラは、タスクの起動かバイトの移動だけを実行します（リンクのドライバ、エグゼキュータ自身の割込）。待機、消去、フラッシュへの書込をする処理が、ハンドラ内で動作することはありません。
2. **マスク時間の上限。** DAGON自身のコードのクリティカルセクション（ウォッチドッグのガードの状態、ロックの状態）が割込をマスクするのは、最大でマイコン定義の`[execution] masked_max_us`です。フラッシュの消去や書込、その他のマイクロ秒単位の上限のない操作を、その中で実行することはありません。複数の利用者を保持するフラッシュドライバは、操作のためにマスクせずに*取得する*ロックで共有します（`dagon_mcu_common::shared::SharedFlash`、`try_lock`。取得済と判明したハンドルは即時にエラーを返却し、待機しません）。clippyが強制します。`targets/mcu/clippy.toml`は、マイコン定義とブートイメージ（stage0、ブートローダ）内の全マスクの基本要素（`critical_section::with`と`acquire`、`cortex_m::interrupt::free`と`disable`、クリティカルセクションの`blocking_mutex::Mutex::lock`）を、解決したパスで拒否するので、別名やラッパーでは隠蔽不可能です。マスクする関数（ウォッチドッグのガードにおける状態の変更）は、理由とともに`#[allow(clippy::disallowed_methods)]`を保持し、テストが一覧と例外をレビュー済の状態に維持します。
3. **フラッシュの操作はハードウェアの事実です。** 消去や書込が停止させる対象は、定義の`[execution] flash_op`です。
   - `fetch-stall`: コードが動作するシングルバンクのフラッシュ。命令の取得とフラッシュの読出は、操作の終了まで停止します。割込は許可のまま待機し、何も動作しません（DAGONはRAMにハンドラを配置しません）。
   - `xip-off-masked`: 操作はexecute-in-placeを無効にしてRAMから動作します。ハードウェアの要求どおり、フラッシュドライバはその1回の操作の間割込をマスクします（そして他方のコアを停止させます）。
   - `cache-off-masked`: 操作の間フラッシュのキャッシュは無効です。フラッシュドライバはその1回の操作の間割込をマスクします（内部RAMの外部のハンドラは動作不可能です）。

   どの場合も、DAGONは1回の操作で、消去する領域（`[execution]
   erase_regions`）の消去単位を1個発行し、所要時間の最大はその領域のデータシートの最大値`unit_max_ms`です。エージェントは全消去を定義の領域と`[[slot_layout]]`から計画し、スロットのストアは計画した操作だけを実行します。ホストは同一の計画を待機します（[消去と試行の待機時間](/reference/definitions/#消去と試行の待ち時間)）。
4. **ウォッチドッグの周期は1回の消去より長期です。**どの領域の`unit_max_ms`の2倍も、`[watchdog] trial_ms`と`confirmed_ms`以下です（定義の読込時に検証します）。どのリクエストも応答のループの`ServeHost::feed`（RP2040のリレーにおけるPC側のループでは、ガードの`progress`）経由の、条件付きのキックで開始します。複数の単位の消去では、単位の間にキックするか、進捗を報告します。待機中のイメージの試行は独立した期間です。その期間のキックの有無は、ガードの試行の方針が決定します（`app.watchdog_taken`が報告します）。
5. **停止中のリンク。** リクエストは新規リクエストに対してリンクをクローズして処理します（I2Cのターゲットは自身のアドレスにNACKし、ストリームのリンクの対向側は返答を待機します）。CPUの停止中に到達したデータは、消失するかハードウェアが保持します。要求側は同一の連番で再試行し、ノードは反復したリクエストに、作成済の返答で応答します。
6. **リセットは返答の出力後だけ。** 再起動は返答の書込とフラッシュの完了まで待機し、その後に`allow_reboot_now`が要求した遅延の間、応答のタスクをアイドルにして待機します（ガードはキックを継続します）。送信不可能だった返答では何もリセットせず、要求側の再試行は同一の返答を取得します。
7. **事実は検証の程度を明示します。** 定義のハードウェアの事実は、出所と照合して検証した数値（定義のコメントが文書と表を明示します）、`{ unverified = N }`（出所と未照合の値）、または`"unmeasured"`（値がない）です。ツールと文書は後者2種類を未検証と報告します。値が必要な確認はunverifiedの値を使用します。リリースがそのいずれかに依存することはありません。

### 12.2 マイコンごと

表はマイコン定義のコードの処理で、上限はそれぞれのマイコン定義の値です。

**STM32F401RE**（`flash_op = "fetch-stall"`、消去単位はセクタ1個。領域により16、64、128KiB）

| 処理 | 実行位置 | 優先度 |
|---|---|---|
| I2Cターゲットのリンクの応答（エージェント、CONF_RUN）、フラッシュの消去と書込 | DAGONの割込エグゼキュータ（USART6のベクタ） | P6、threadモードより上 |
| リンクのI2CとDMAの転送 | I2Cドライバの割込 | HALの既定 |
| タイマ（`embassy-time`） | そのタイマの割込 | HALの既定 |
| ウォッチドッグへのキック | アプリのキックのタスク、ガードが条件を検証します | threadモード |
| サンドボックスのタイマ（4.3） | SysTick | どの能力の割込よりも上位 |

消去中は、セクタの消去終了まで何も実行しません。IWDGはリクエストのキックから計測します。自身のI2Cアドレスはリクエスト全体の間NACKするので、PC側のノードのポーリングにはCPUなしでペリフェラルがNACKし、PC側のノードは自身の期限まで再試行します。

**RP2040**（`flash_op = "xip-off-masked"`、消去単位は4KiBのセクタ1個。ボードの`[flash]`が別に決定する場合はその値）

| 処理 | 実行位置 | 優先度 |
|---|---|---|
| PCへのリンクとエージェント、接続先のノードへのリンク（リレー） | DAGONの割込エグゼキュータ（SWI_IRQ_0） | P1 |
| USBデバイス | USBの割込 | HALの既定 |
| フラッシュの消去と書込 | PCのタスク、スロットのストアが保持する`SharedFlash`のハンドル経由 | 操作ごとにセクタかページ1個、ドライバがマスク |
| ウォッチドッグへのキック | キックのタスク、ガードが条件を検証します | threadモード |

フラッシュの操作はそれぞれ、セクタの消去1回かページの書込1回の間割込をマスクします。スロットのストアは、セクタ間でガードに進捗を報告します。その間USBコントローラはホストにNAKし、ホストは再試行します。

**ESP32-S3**（`flash_op = "cache-off-masked"`、消去単位は4KiBのセクタ1個。ボードの`[flash]`が別に決定する場合はその値）

| 処理 | 実行位置 | 優先度 |
|---|---|---|
| UARTとUSB Serial/JTAGの上りのリンク、エージェント、フラッシュの消去と書込 | DAGONの割込エグゼキュータ（FROM_CPU_INTR1） | Priority2、threadモードより上 |
| ウォッチドッグへのキック（確定後、動作の確認を条件に） | リンクのタスク | Priority2 |
| アプリのタスク | threadモード | 最低 |

フラッシュの操作はそれぞれ、消去単位1個の消去の間割込をマスクします。スロットのストアは、単位の間にウォッチドッグをキックします。UARTの受信FIFOは容量分を保持します。停止で途切れたフレームは20 msの間隔の規則で破棄し、要求側が再試行します。その間USB Serial/JTAGはホストにNAKします。

### 12.3 上限の決定方法

マイコンごとの上限は、ボードでの以下の値から決定します。DAGON自身のコードで最長のクリティカルセクション（`masked_max_us`: 最悪の値に余裕を加算した値）、ボードに搭載したフラッシュ部品の最悪の消去時間（と、定義が明示するそのデータシートの値）、1回の消去の終了時に残存するウォッチドッグの余裕、その後のリンクの回復（PC側のノードの再試行への応答）です。
