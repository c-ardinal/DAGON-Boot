---
title: "ライフサイクルイベントと拡張"
description: "ブートローダ、エージェント、リレー、ホストに共通の1つのモデル: 状態機械、イベント、ポートです。"
---

この文書はDAGONのライフサイクルイベントと拡張のモデルの設計です。実装と、DAGONを拡張する全利用者のための参照資料です。以下の事項を提示します。

- 全レイヤーの全状態遷移を、観測可能なイベントにする方法
- ボードの定義、ライブラリ、アプリケーションがDAGONを拡張する方法。アプリとホストで、イベントを観測し、少数の判断点に応答し、ポート経由でDAGONの一部を置換します。
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

状態機械はスロットのストアのポートをラップします。`ImageStore<S: SlotStore>`がストアを保持し、ポートの変更メソッド（`erase`、`write`、`write_header`、`set_pending`、`confirm`）は、状態機械だけが作成する`Transition`を、その変更のイベントを発行する遷移内で受信します。`SlotStore`のトレイトは、ボードが実装するポートのままです。エージェントがストアを保持し、誰にも受渡しません。ボードのその他の資源（フラッシュの読込側、ウォッチドッグ）は、マイコン定義のコードが自身で保持するハンドルです。それらのハンドルの到達先（未加工のフラッシュドライバ、シミュレータの準備用の補助）は、この保証の対象外です。同一イメージでフラッシュにアクセスするその他のコードと同様に、ボードやテスト環境の信頼されたコードです。ヘッダの書込が再読出不可能な場合は、スロットを再度消去します。そのため、状態機械が未記録のイメージを故障が残存させることはありません。エージェントは`images() -> &ImageView`を公開します（読取専用: ステータス、各スロットのライフサイクルの状態、自身のバージョン、試行中かどうか）。起動時に自身で確定するアプリ（テスト環境用）は、起動にその動作を要求します（`App { confirm: Confirm::Own, .. }`）。起動はボードの起動処理の後に`confirm_own()`を呼出します。これが`confirmed`の遷移です。最初に`self_check`の判断、次にポートの確定を実行し、イベントは必ず発行します。ブートのレイヤーの遷移は、アプリが起動時に状態ログから報告します（`app.trial_started`、`app.rolled_back`）。

### 1.2 ブートの手順

self-bootのマイコン（stage0とブートローダ）の状態は`Entry → HalReady →
WatchdogArmed → LogRead → BeforeDecision → Planned → Committed →
Jumping | Halted`です。`plan`は純粋です（フラッシュを読込し、判断と追記する記録を計算します）。`commit`がイメージのライフサイクルの遷移`attempt`、`rolled_back`、`chosen`、`no_image`を実行します。stage0が保持する状態は`Entry`、`CoreChosen`、`Halted`だけです。状態ログのブートローダの記録と試行のセルから、ブートローダのスロットを選択します（`dagon_core::bootcore::start`、retained-formats.md 5）。計数した起動が`trial_attempts`未満の間は待機のコアを、それ以外は確定済のコアを選択します。コアを起動するのは記録が指定する場合だけで、記録のないコアも、起動回数が上限に到達した待機のコアも起動しません。コアの選択に先行して、コアのヘッダの署名が検証可能であること、ヘッダが記録の指定どおりであること、本体がヘッダのダイジェストと一致することを検証し（4.3）、試行のコアの起動前にウォッチドッグを開始し、拡張は実行しません。stage0はフラッシュに書込しません。起動可能なコアがない場合（ブートローダのスロットが1個のマイコンで、新規コアの書込が電断で途切れたか、新規コアが試行に不合格の場合）は、状態ログにある確定済APPの記録が指定するアプリを起動します。その起動前に、記録のヘッダのダイジェスト、署名（4.3）、イメージの種類、マイコン、ターゲットのタグ、スロットのバインディング、配置とロードの範囲、本体のダイジェスト、エントリポイントが本体内にあることを検証します。APPの試行がオープン中の間は起動せず、待機のアプリも起動しません。試行のセルは無変更にし、何も記録しません。そのアプリがブートローダを再度書込します。そのようなアプリも不在の場合だけ、何も起動せずに復旧を待機します（4.3）。起動したブートローダは、自身のブート判断の実行前にブートローダの試行を決着させます（`settle_log`）。待機のコアは自身の検査（起動可能なアプリがあり、そのアプリのヘッダがコアのキーで検証可能）の合格後にだけCONFIRMED（ターゲットBOOTCORE）を追記し、それ以外はリセットします。別のコアが待機中の場合に起動した旧コアは、ROLLBACKを追記します。ウォッチドッグの規則は4.1に記載しています。

ブートローダのイメージのライフサイクル（エージェントの`core_image`の状態機械）は、1.1のアプリイメージのライフサイクルに、ブートローダの経路が保持する遷移で従います。エージェントの`erase`、`written`、`pending_set`（ターゲットBOOTCOREのXFERとSET_PENDINGで、未使用のブートローダのスロットへ遷移。稼働中のコアや確定済のコアのスロットは不可。マイコン定義がブートローダのスロットを1個だけ付与する場合はその1個へ書込し、アプリの待機中と試行中はその転送と待機の設定を拒否し、そのコアが確定済でない場合（待機中、または中断した書込で空白のスロット）はアプリの待機の設定を拒否します。ホストは、最初にアプリを確定またはロールバックするよう要求します）、新規コア自身のCONFIRMEDによる`confirmed`と`demoted`、旧コアのROLLBACKによる`rolled_back`（アプリの起動時にリプレイします）です。アプリから観測可能な`Trial`の状態はありません。アプリの動作開始前に必ず、コアが試行を決着させるからです。ターゲットのバイトが`BOOTCORE`のSTATUSは、ブートローダのスロット（バージョン、ヘッダのダイジェスト、稼働中のコアと確定済のコア）を報告します。マイコン定義がブートローダのスロットを宣言しないボードは、`Unsupported`を返却します。ブートローダのスロットの数と配置は、マイコン定義の`[[slot]] image = "bootcore"`の項目に由来します（1個か2個です。スロット1個で不可能な動作は[制限事項](/explanation/limitations/)に記載しています）。

書込モードと復旧モードについて説明します。ブートローダも受信します。アプリと同一の書込のセッション（1.3）を、セッションのブートローダの書込側として、書込モードか復旧モードの場合だけ実行し、HELLOに`mode`の1（書込）か2（復旧）で応答します。アプリは0、復旧用エージェントは3で応答します。待受時間内に到達したSTAY（0x05）か、UPDATE_DONEが未消費のUPDATE_STAYの記録で、書込モードに移行します。有効なログの面がないか、起動可能なアプリがない場合は、復旧モードに移行します。待受時間はブートローダの起動から計測し、時間長はマイコン定義の`[boot_receive] listen_window_ms`です（0から200ミリ秒。省略時は50ミリ秒、0は待受時間なし。プロジェクトは`[mcu_override]`で変更します）。`[links]`の種類のうち、`[boot_receive] kinds`が参加対象とした種類のリンクで待受時間を設けます。対向側が最初に列挙を実施する種類（USB）は対象外で、ブートローダはその種類では書込モードか復旧モードの場合だけ受信します。ブートローダは定義が付与する全リンクで受信し、リンク間に優劣はありません。アプリはREBOOTのmode 1で引渡します。アプリは状態ログに、試行の分とUPDATE_STAY（0x06）とUPDATE_DONE（0x09）の領域を確保し（`Log::request_stay`。アプリの稼働中にコンパクションを実行します）、UPDATE_STAYを追記した後にリセットします。ブートローダは書込モードへの移行時に、その領域へUPDATE_DONEを追記します（`Log::take_stay`。消去はしません）。そのため、UPDATE_STAYは1回だけ使用されます。コンパクションは、未消費のUPDATE_STAYを残存させます。書込モードのブートローダは、書込が指定するスロットだけを消去して書込し、書込開始前に共通の確保（`Log::reserve`。他方の面にコンパクションを実行し、その面のヘッダを最後に書込するので、唯一の有効な面を消去することはありません）でログの領域を確保し、REBOOTの直前にログを消去することはありません。STAYとREBOOTのmode 1では書込モードに滞留し、REBOOTのmode 0で退出します。STAYかUPDATE_STAYで移行した場合は、要求がないまま60秒経過しても退出します。復旧モードには起動するアプリがないので、ブートローダはそのモードに滞留します。アプリの稼働中はSTAYを拒否します（`E_STATE`）。書込モード以外では、ブート段階は消去しません。ブートローダのセッションが書込するのは、マイコン定義の`[writers]`がブートローダに列挙する種類（アプリ）だけです。その他の種類の転送は、何も消去しない段階で`E_UNSUPPORTED`で拒否します。これらの記録以前の読込側はこれらをスキップし、STAY以前のエージェントは`E_UNSUPPORTED`で応答するので、`wire`と`state-log`の番号は変化しません。ブートローダは中継しません（[制限事項](/explanation/limitations/)）。

ブートのアクションプログラム（5.2）は、`WatchdogArmed`以降でだけ動作します。そのため、ウォッチドッグによるブートの保護開始前に、アクションが動作することはありません。ブート段階に拡張のコードはありません。

| イベント | 時点 | ペイロード | アクションプログラム |
|---|---|---|---|
| `boot.watchdog_armed` | ウォッチドッグを開始した（4.1）。この時点以降、アクションプログラムが動作します | `period_ms` | はい |
| `boot.log_read` | 状態ログを読込した | `ok`、`pending`、`confirmed`、`attempts` | はい |
| `boot.before_decision` | `plan`の実行直前。判断前の最後のイベント | `pending`、`confirmed`、`attempts` | はい |
| `boot.decision_planned` | 判断が決定し、何も書込していない | `Decision` | はい |
| `boot.slot_chosen` | イメージのライフサイクルの`chosen` | `slot`、`trial`、`version`、`entry` | はい |
| `boot.attempt_recorded`、`boot.rollback`、`boot.no_image` | イメージのライフサイクル（1.1） | | はい |
| `boot.before_jump` | 制御がDAGONから離脱する前の最後のイベント | `slot`、`trial` | はい |
| `boot.recovery` | アプリの代替として復旧の経路に移行 | `path` | はい |
| `boot.halt` | コアが完全に停止 | `code` | はい（ブートローダ） |
| `boot.program_step` / `boot.program_skipped` | ブートのアクションを1個実行した/ プログラムを実行しなかった | `index`、`op`、`ok` / `why` | はい |

vendor-stage2のマイコンにはDAGONのブート段階がありません。アプリが起動時に選択データから、`app.trial_started`と`app.rolled_back`を報告します。

### 1.3 エージェントのセッション

状態は`Idle`、`Header { slot, body_len, got }`、`Body { target, slot,
len, next }`です。プロトコルのリクエストは例外なく遷移です（HELLO、STATUS、VERSION、進捗は自己遷移）。プロトコル自体もポートです（2.1）。状態機械のトリガーはプロトコルに依存せず（`update_offered`、`header_chunk`、`data_chunk`、`transfer_end`、`switch`、`reboot`、`confirm`、`status`）、プロトコルの拡張（DAGONのリンク、UDSなど）がメッセージをトリガーに対応付けます。

生成される起動時の順序: `app.starting` → `app.store_opened`（または`app.store_failed`）→ `app.trial_started` / `app.rolled_back` → `app.watchdog_taken` → `app.bring_up` → `app.link_up` …

| イベント | 時点 | ペイロード |
|---|---|---|
| `app.starting` | DAGONの起動関数に進入した | `tag`（名称のダイジェストとして、2.4）、`role`、`mode` |
| `app.store_opened` / `app.store_failed` | 状態ログや選択データを読込した/ 読込不可能 | `active`、`trial`、`version`、`attempts`、`last_reason` / `error` |
| `app.trial_started` / `app.rolled_back` | 状態ログからリプレイした | `slot`、`version`、`attempt`、`max` / `from`、`to`、`reason` |
| `app.watchdog_taken` | ブートローダのウォッチドッグを継承した | `period_ms`、`trial_policy` |
| `app.bring_up` | ボードの起動処理を実行した | `board_id`（名称のダイジェストとして、2.4） |
| `app.extension_started` / `app.extension_overran` | 拡張のハンドラが動作した/ 超過した（3） | `id`、`point` / `id`、`point`、`us` |
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

ホストは操作（`apply`、`rollback`、`recover`、`confirm`、`provision`、`build`）の進行をイベントで通知します。その他にプロジェクトのセッションとエージェントとの接続があります。ホストの転送、署名、ビルドの実行はポートです（2.1）。

| イベント | 時点 | ペイロード |
|---|---|---|
| `host.build.started` / `host.build.finished` | ノードとスロットごと | `node`、`slot`、`ok`、`elapsed_ms`、`image_sha256` |
| `host.release.prepared` | バンドルを作成した | `version`、`nodes[]`、`path` |
| `host.deploy.planned` | 一覧を表示した。**判断**`confirm_plan` | 一覧 |
| `host.deploy.started` / `host.deploy.finished` | | `release`、`nodes[]` / `ok`、`results[]` |
| `host.node.discovered` | 検出 | `target`、識別 |
| `host.node.step` | ノードの各段階（TRANSFER、VERIFY、REBOOT、CONFIRM、REJECTED、FAILED、ROM） | `node`、`target`、`phase`、`percent`、`version` |
| `host.node.reconnecting` | 再起動後のバージョンを待機中 | `node`、`elapsed_ms`、`other_version?` |
| `host.node.result` | | `node`、`Done(v) \| Failed(reason, status?) \| NotStarted \| NeedsFirstWrite`（`NeedsFirstWrite`: PC接続のノードの報告で無応答のノードです。このノードへの送信はなく、他ノードの書込は継続します。このノードには直接の書込が必要です） |
| `host.confirm.result` | | `Confirmed \| NotOnTrial \| NeedsChoice` |
| `host.rollback.*`、`host.recover.*` | deployと同一の形式 | |
| `host.provision.started` / `host.provision.recorded` | ROM経由の初回インストール。インストールしたブートイメージの識別を記録した | `node`、`boot_sha256` |
| `host.project.opened` / `host.project.closed` | プロジェクトのセッション | `path` |
| `host.agent.linked` / `host.agent.unlinked` | 同意を記録した/ 取消した | `agent`、`way` |
| `host.version.read` | VERSIONに応答した、またはUnsupported | `target`、`VersionResp?` |
| `host.job.*`、`host.device.*` | serveの作業とデバイスのメッセージ | |

### 1.6 永続の状態とその状態機械

リセットや再起動をまたいで残存する対象は例外なく状態機械の背後にあり、その書込は例外なく最後の列の意味でアトミックです。どの時点のリセットや電断でも、旧状態か新状態のいずれかが残存し、読込側が受理する中途半端な状態は残存しません。

| 永続の状態 | 位置 | 状態機械 | 書込する遷移 | アトミックである方法 |
|---|---|---|---|---|
| アプリのイメージのスロット（ヘッダ、本体）とその選択（状態ログkind S） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `erase`、`written`、`pending_set`、`pending_moved`、`attempt`、`confirmed`、`demoted`、`rolled_back` | 本体が先行、ヘッダが最後（スロットにイメージが存在するのは、ヘッダの再読出と検証の完了以降です）。状態ログは追記だけの記録で、それぞれCRCを保持し、交互に使用する2個の面にあります。途中で途切れた記録はCRCに失敗して無視され、前の記録が有効なままです |
| ベンダーの選択（kind V） | ノードのフラッシュ | イメージのライフサイクル（1.1） | `pending_set`、`attempt`、`confirmed`、`rolled_back` | ベンダーの2セクタの方式: 連番とCRCを保持する項目全体を別のセクタに書込し、有効で最新の連番が優先されます |
| ブートローダのスロットとその記録（ブートローダのログ、またはマイコン定義が両方のログに同一の面を付与している構成ではAPPの記録と共有する状態ログ） | ノードのフラッシュ（stage0のあるself-boot） | ブートローダのイメージのライフサイクル（`core_image`、1.2） | エージェントの`erase`、`written`、`pending_set`。新規コアの`confirmed` / `demoted`。旧コアの`rolled_back`。ホストの`provision`（`host.provision.recorded`）。stage0の`chosen`（書込なし） | アプリのスロットと状態ログと同一です。stage0の起動回数は、フラッシュではなく試行のセルにあります（retained-formats.md 5） |
| 工場出荷の復旧用エージェント（vendor-stage2のマイコン） | ノードのフラッシュ、factoryのパーティション、ヘッダは分離 | イメージのライフサイクル（1.1）、ターゲットAGENTの`erase`と`written` | エージェント。確定済のアプリがOTAスロットから稼働中の間だけ | 本体が先行、ヘッダが最後。書込中は、稼働中のアプリ（そのエージェントも対象）が復帰経路です |
| インストールしたブートイメージの識別（vendor-stage2のマイコン） | プロジェクトの準備の記録 | ホストの`provision` | `host.provision.recorded` | 一時ファイルに書込し、旧ファイルに上書きする形で名称を変更します |
| エージェントの同意 | `~/.dagon/agent.json` | 接続: `Unlinked → Linked → Unlinked` | `host.agent.linked` / `host.agent.unlinked` | 一時ファイルと名称の変更 |
| プロジェクトのセッション（オープン中のプロジェクト、リリースのキー） | serveのプロセス | プロジェクト: `Closed → Open` | `host.project.opened` / `host.project.closed` | 1個のロックで保護したプロセスメモリ |
| 診断のステップ、拡張の時間、無通信の時間、観測済の対象の表 | RAMだけ、プロセスの寿命 | 永続ではなく、イベントだけ | | |

このモデルの永続の構造に、その場で再書込するヘッダ、カウンタ、状態の欄を保持するものはありません。永続の書込は例外なく、1単位でコミットする新規記録か、名称の変更で置換するファイル全体です。このモデルが永続のデータのために導入する形式（マイコンの識別を保持するコンパイル済の構成）は、文章では規定しません。ランダムな故障の注入を伴う実行可能なモデルとして設計し、そのモデルに合格した形式が仕様です（5.5）。

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

このマクロは状態機械ごとに以下の要素を生成します。状態の列挙型は、生成された`Machine`型にある非公開の欄の背後に配置します（宣言したcrateでのみ作成可能で、`Default`も`Clone`もなく、変更メソッドは遷移メソッドだけです。各メソッドはレイヤーの合成されたチェーン（2.3）を受信し、イベントと遷移後の状態を作成し、イベントを配信し、その後に状態を設定します。遷移元ではない状態からは何も変更せず、何も配信しません。`from`の状態は名称で、任意でフィールドを束縛し、値のパターンにはなりません）。遷移ごとに1個のペイロードの構造体を生成します（`#[non_exhaustive]`で、`Copy`でも`Clone`でもないので、宣言したcrateだけが作成可能です）。各ペイロードが所有する短縮形とその解読も生成します（2.4）。`CATALOG`も生成します（状態機械、状態、そして遷移ごとにfrom、to、トリガー、種類、コード、バイト数付きのペイロードのフィールド、`observe`の経路、`action`と`retired`のフラグ）。宣言から遷移表も生成します（`StateId`、`Trigger`、`State::leaves(Trigger)`）。状態からの遷移の有無の把握が必要なコードは、名称を比較せずにこの遷移表に照会します。全状態から全遷移を実行し、遷移表をカタログと照合する、状態機械ごとのテストも生成します。イベントが保持するのはレイヤー、種類のコード（`Event::kind()`、`u8`）、ペイロードだけです。そのため、イベントを配信するイメージはカタログの名称をリンクしません。名称は、人間向けの箇所（ログの表示、`dagon_catalog::name`、生成するページ）でだけコードから参照します。crateごとに`MACHINES`のstaticを定義し、カタログの登録簿（`dagon-catalog`）がそれを列挙します。`dagon-events`に依存するcrateが欠落している間は、そこのテストが失敗します。どの遷移も到達しない状態、`emits`や`observe`の経路のない遷移、重複したコード、過大なペイロード、crate内の2個目の`state_machine!`、その外部の状態機械は、コンパイルエラーです。[イベント](/reference/events/)はカタログから生成し、テストが両者を同一に維持します。

判断点（3）は`emits`の節にはなりません。新規の判断点は設計の変更で、最初にこの文書に、上限、既定値、応答する側、安全の論拠とともに記述し、独立したトレイトのメソッドにします。

イベントを発行するのはDAGONだけです。拡張は観測するだけで、作成することはありません。

### 1.8 だれが何を観測するか

どのイベントにも最低1個の観測の経路があり、経路は生成されたカタログの属性です。そのため、生成された[イベント](/reference/events/)がこの表をイベントごとに保持します。経路のない種類があると、生成されたテストが失敗します。*Live*は同一イメージの拡張（2）で、ブート段階ではブートのアクションプログラム（5.2）です。*replay*は、アプリのエージェントが起動時に状態ログから再発行するものです。*host*はCLI、serveのストリームで、返答やVERSIONが保持するデバイス側の全イベントも観測します。リプレイが配信するのは、カタログが`replay`とマークし、`retired`ではない種類だけです。その他の記録は推測せず、受理しません。

| イベント | レイヤー | Live | Replay | Host |
|---|---|---|---|---|
| `boot.watchdog_armed`とそれ以降の全ブートのイベント（`halt`とプログラムのイベントも対象） | boot | はい: ブートのアクションプログラム（5.2） | | |
| `boot.slot_chosen`、`attempt_recorded`、`rollback`、`no_image` | boot | はい | 状態ログ（`app.trial_started`、`app.rolled_back`） | STATUS |
| `app.*` | app | はい | `trial_started`、`rolled_back`はそれ自体がリプレイです | 返答、リレーの`child_*` |
| `relay.*` | relay | はい（コーディネータのアプリのチェーン） | | 中継したリクエスト |
| `host.*` | host | `on_event`のコールバック（CLI、serve） | | ストリーム、`--events` |

## 2. 拡張のモデル

全レイヤーで形式は1種類です。構成要素は、DAGONが置換のために公開する**ポート**、ポートを提供してイベントを観測し判断に応答する**拡張**、既定では何もしない**イベント**、**1個の合成の順序**（ボードの定義 → ライブラリ → 利用者）の4点です。拡張はアプリとホストでだけ動作します。実行の文脈は異なりますが（アプリのembassyのタスク、ホストのスレッドとJSON）、形式は同一です。宣言的なアクションプログラム（5.2）はブートのデータで、拡張のコードではありません。

### 2.1 ポート

ポートは、DAGONが置換可能な1個の部分のために呼出するトレイトで、DAGON自身の実装が既定です。ポートを提供する拡張は、リンクされたイメージのその部分を置換します。ポートは全レイヤーにあります。

ポートのカタログは、コード内の1個の表`dagon_events::PORTS`で、ポートごとに1個の記録（`id`、存在するレイヤーの集合、レイヤーごとの既定、信頼の方針、必要なサービス、説明）です。この表から、以下の表、生成された[ポート](/reference/ports/)、マニフェストの検証（`provides`は拡張のレイヤーのポートを指定する必要があります）、`dagon status`のポートの列が派生します。テストがこの抜粋をカタログと同一に維持します。ポートでは*ない*DAGONの部分も、同一のカタログに`port = false`と理由とともに列挙するので、「これは置換可能か」という問いに1個のソースが応答します。

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

ポートの既定はDAGONのコードです。マニフェストの`provides`は、レイヤーの集合がマニフェストの`layers`の全レイヤーを包含するポートを指定します（8.4のUDSの例は、`app`に`UpdateTransport`、`UpdateProtocol`、`Diagnostics`を宣言します）。

### 2.2 拡張とマニフェスト

拡張はマニフェスト`dagon-extension.toml`を保持するRustのcrateです（最も単純な種類では、定義内のアクションプログラムです）。

```toml
format = 1
id = "uds-transport"                 # a-z 0-9 -, up to 32
layers = ["app"]                     # where it may be linked
provides = ["UpdateTransport", "UpdateProtocol"]   # ports it replaces (may be empty: observer only)
observes = ["app.update_*", "app.transfer_*"]   # event kinds, validated against CATALOG
decides = ["accept_update"]          # decision points it answers (may be empty)
[capabilities]                       # what DAGON hands it, nothing else
peripherals = ["can0"]               # declared in the board definition's peripheral table
pins = []
buses = []
timer = true
[limits]
max_run_us = 2000                    # per call, measured (3)
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

`Ctx`は、マニフェストが宣言した能力のハンドル（GPIO、バス、タイマ、ペリフェラルのウィンドウ）を保持し、その他には何も保持しません。ポートの実装は同一の型への独立した`impl UpdateTransport for MyExt`です。`Ctx`に、フラッシュ、状態ログ、ウォッチドッグ、スロットのストア、DAGONのメモリへのハンドルはありません。

拡張のインターフェースがAPIの境界です。拡張が受信するのは`&Event`、`&Ctx`、判断の引数だけで、いずれもチェーン、公開の状態、リプレイには接続しません。チェーンの合成、チェーンへの配信、リプレイは、レイヤーの起動のAPIです。拡張のインターフェースの外部で同一イメージに組込まれたコード（DAGONのcrate、アプリケーション自身の起動コード）は、1個のアドレス空間のコードと同様に信頼の基盤に属します。

### 2.3 合成

型付きのチェーンで、順序は型のレベルで、つまりビルド時に固定されます。

```rust
pub struct Chain<A, B> { /* private */ }   // Chain::new(a, b)
impl<A: Extension, B: Extension> Extension for Chain<A, B> { /* on_event: both; decisions: each in turn, first Some wins */ }
pub struct Nop;  // no extension
#[macro_export] macro_rules! extensions { ($a:expr $(, $b:expr)*) => { … nested Chain::new … } }
```

順序は全レイヤーで常に**公開元 → ボードの定義 → ライブラリ → 利用者**です。レイヤーの起動（マイコン定義のコードの`start`）が、`compose`で自身でチェーンを作成します。`Published`の状態（2.4）のための公開元、次にボードの定義のチェーン、次に利用者の`extensions![..]`（ライブラリが先行し、次にアプリ自身）です。結果は内容を参照不可能な`Composed`です。部分へのアクセスも順序変更も不可能で、拡張ではなく、レイヤーの起動だけが保持します。利用者のコードが自身を先頭に配置することは不可能で、`extensions![..]`が公開元を包含することはありません。通知は全拡張に到達します。判断は棄権ではない最初の応答で停止します。ポートを提供可能な拡張は、イメージごとに最大1個です。チェーンがポートを2回提供するプロジェクト（ライブラリと利用者の両方が`UpdateProtocol`を持込むなど）は、`dagon gen`が両方の名称を提示して拒否します。

レイヤーごとの登録は以下のとおりです。

- **アプリ（embassy）。**`node::start` / `board::start`は`App`を受信します。自己検査の関数、`exts: Option<&'static mut (dyn Extension +
  Send)>`（`static`に配置したアプリの`extensions![..]`。embassyのタスクはジェネリック化が不可能で、応答のタスクがチェーンを保持するので、ジェネリックではなく参照です）、`confirm`（`ByHost`か`Own`: 起動がアプリの試行を確定します）です。起動は、リンクの応答開始前に、ボードの`BoardBringUp`のポート（`app.bring_up`。ポートがないボードは何も発行しません）を実行し、次に自身の確定を実行します。ボードの定義の拡張（ボードの定義の`extension()`。ESP32-S3では`NodePeripherals::
  board_extension`）は、チェーンのボードの位置に配置されます。エージェントは起動とその応答のタスクの内部に限定され、受渡する公開の関数はありません。マイコン定義のコードがレイヤーの`Published`を作成して保持し、合成し、アプリに`Reader`を返却します。エージェントの状態機械が全イベントをチェーンに配信し、チェーンが3個の点で判断のメソッドに照会します。応答のタスクは、時間の上限を計測して報告すると同時に、チェーンのハンドラを通常のコードとして実行します。
- **リレー。** リレー自身のエージェントは、同一のチェーンを保持するアプリです。リレーの状態機械のイベントと`NodeLink`のポートはそのチェーンを経由します。
- **ホスト。** ホストの拡張は、データで定義する種類（エージェントの定義、プログラムやデバイスで指定するシリアルやCANのアダプタのような転送の定義）と、`dagon_host::Transport`、`Signer`、`BuildRunner`を実装するRustのcrateです。crateはCLIをビルドするワークスペースの依存として読込します。ホストに拡張のチェーンはありません。ホストは利用者の権限で動作します。

アプリのエージェントは、合成されたチェーンだけを受信します。`Agent::new(config, verifier, store, chain)`で、`chain`は`compose`の結果、`verifier`はマイコン定義の署名の検証器です。別のフックのトレイトはありません。イベントの観測と、`accept_update`、`allow_reboot_now`、`self_check`への応答は`Extension`のメソッドです。マイコン定義のコードの`start`は、アプリの`fn() -> bool`の自己検査も継続して受理し、1メソッドの拡張（`SelfCheck`）にラップして、チェーンの利用者部分で、アプリの拡張の後に配置します。この拡張は棄権しないので、前方に配置すると拡張の応答を隠蔽するからです。

### 2.4 待機可能な公開の状態

非同期の表示やログのタスクが、ハンドラ内で動作することはありません。チェーンの先頭の公開元が、各イベントを以下の2個にコピーします。

- `Latest`: `Latest { state, last }`を保持する`embassy_sync::watch::Watch`（複数の受信側、最後の値）です。全イベントから畳込したレイヤーのスナップショット（アプリの`{ phase, progress, percent, version, trial }`、リレーの接続先のノード）と、最後のイベントの記録です。
- `Recent<N>`: 直近N個のイベントを保持する固定のリングで、`EventRecord`という、`state_machine!`がイベントごとに生成する所有された有界の形式です。`{ layer: u8, kind: u8, flags: u8, dropped: u8, seq: u32, payload:
  [u8; 16] }`。フィールドは、カタログが指定する短縮形でコピーします（レングス、id、ダイジェストの先頭。ノードのタグやボードのidのような名称は、ドメインの接頭辞と名称から計算したSHA-256の最初の8バイトとして記録し、ホストが把握済の名称を経由して対応付けます）。作成側の復帰後に参照を残存させることはありません。溢れると、最古の記録を破棄します。その後の記録が`dropped`で欠落を報告し（それぞれ最大255で、残余は後続の記録へ繰越します）、作成側は決して待機しません。`seq`は一周し、連番の演算で比較します。

どちらも1個の`Published<S, N>`にあり、レイヤーのcrate（マイコン定義、ホスト）が作成して保持し、排他の参照で`compose`に受渡します。そのため、チェーンが唯一の書込側です。表示、ログ、アプリのタスクは、読取専用の`Reader`（受信側、最新の状態、最近のリング）を受信します。ブートローダに公開元はありません。ブートローダのイベントはブートのアクションプログラムに到達します。

### 2.5 ホスト: イベントのストリームとcrateの発見

`dagon serve`は、全ホストのイベントを`/v1/events`に以下の形式で出力します。

```json
{"type":"event","format":1,"layer":"host","kind":"node.result","at":"<RFC 3339 UTC>",
 "job":17,"payload":{"node":"n1","outcome":"done","version":"1.1.0"}}
```

既存の`job.*`と`device.*`のメッセージは変化しません（serve-api 1。`event`のメッセージは追加です）。受信側の規約: ストリームや`--events`の出力の受信側は、未知のメッセージの型、イベントの種類、ペイロードのフィールドをスキップし、読込不可能な`format`だけを拒否します。これにより、`EVENT_FORMAT`を上げずに種類を追加可能です。CLIは`--events`で同一のイベントを出力します（それぞれJSON 1行）。

serveによるcrateの探索範囲: `dagon.toml [serve] packages`は、プロジェクトからの相対のglobの一覧です（生成されたプロジェクトでは`["nodes/*"]`）。この一覧に一致し`Cargo.toml`を保持するディレクトリだけを列挙してビルドします。DAGONに組込の探索パスはありません。

## 3. 判断点

この表にないものは通知です。ブートローダに判断点はなく、DAGON外部のコードがブートローダの起動対象に影響することはありません。

| # | 判断 | レイヤー | 応答可能な主体（順に、棄権ではない最初のもの） | 上限 | 既定 | 決して変更不可能な事項 |
|---|---|---|---|---|---|---|
| 3.1 | `accept_update(&UpdateOffer) -> Accept` | app | ボード、ライブラリ、利用者 | 50 ms。DAGONの全ガードに合格した後、消去の前 | 受理する | DAGONが拒否したものは受理されません（その場合は呼出されません）。拒否はプロトコルの"state"のステータスで応答します |
| 3.2 | `allow_reboot_now(&RebootRequest) -> Delay` | app | ボード、ライブラリ、利用者 | `Now`か`After(ms)`、`ms ≤ REBOOT_DELAY_MAX`。マイコン定義のウォッチドッグの周期（`[watchdog] trial_ms`、`confirmed_ms`）から作成するビルド時の定数で、2個の周期のうち最小値の半分です。DAGONは最初に返答し、待機中はガードの方針のもとでキックを継続し、その後にリセットします。待機中のウォッチドッグによるリセットも同一のリセットです | 即時 | 再起動は拒否不可能で、遅延には上限があります |
| 3.3 | `self_check(&TrialInfo) -> Health` | app | ボード、ライブラリ、利用者 | 50 ms。`Pending`は"state"のステータスのままで、ホストに再試行させます | 健全 | 試行中ではないスロットは確定不可能で、`confirmed`の遷移をスキップ不可能です |
| 3.4 | `confirm_plan(&Prepared) -> bool` | host | 対話する利用者（TTY、ページ）か`--yes` | 利用者の応答まで | 照会する | キーの確認とイメージの検証は、計画の表示前に実施します |

上限は計測します。合成されたチェーンは、レイヤーのクロック（マイコン定義のコードに由来するDAGONのクロックで、ポートではありません）で、拡張の呼出をそれぞれ計測します。遅延した応答は棄権として無視し、チェーンは`app.extension_overran { id, point, us }`を発行します（`id`は拡張のidで、記録ではその64ビットのFNV-1aのハッシュです）。

## 4. 安全の規約

### 4.1 ブートのウォッチドッグ

ウォッチドッグはHALの初期化の直後、ログの読込前、どのアクションも動作を開始していない段階で、マイコン定義の`trial_ms`で有効にします。`plan`の後、判断が試行でなければ`confirmed_ms`に設定し、ジャンプの直前に1回キックします。ウォッチドッグへのキックはDAGONだけが、自身のスケジュールで実行します。

### 4.2 拡張

拡張が保持するのは、APIの形式の保証だけです。

| ルール | 強制するもの |
|---|---|
| ストアにアクセスしない | 拡張は読取専用のペイロードを受信します。アプリケーションは`ImageView`を取得します（2.3）。スロットのストアの変更メソッドには、イメージのライフサイクルの状態機械経由でだけ到達します。 |
| 検証と確定の順序 | 状態機械の遷移だけが状態を変更します。`self_check`に照会するのは、「試行中」のガード通過後、`confirmed`の遷移内だけです。`accept_update`はDAGONのガードの後だけです。拡張の応答は検証された`enum`で、DAGONが実行するクロージャではありません。プロトコルの拡張はトリガーを作成し、セッションの状態機械がガードを実行します。 |
| 時間 | 判断の上限は3に記載しています。通知にも同一の上限があり、計測します。 |
| ウォッチドッグ | `Ctx`にウォッチドッグのハンドルはありません。 |
| 再起動 | 3.2: 遅延に上限があり、拒否不可能です。 |

リレーの拡張はアプリの拡張です。ホストの拡張は利用者の権限で利用者のOS内で動作します。データで定義するもの（エージェント、プログラムで指定する転送）は、共有するcommand転送の上限のもとでプロセスの外部で動作します。ホストの拡張は判断に応答しません（3.4）。

### 4.3 認証の規則

全検証器（`ImageVerifier`と`Digest`のポートで、既定がこの規則です）が使用する認証の規則は、以下の1個です。

- **署名するバイト列。** イメージのヘッダの署名は、ヘッダのオフセット0から署名の欄の先頭までのバイトを網羅します。その範囲には、形式番号、イメージの種類、マイコンの識別、ターゲットのタグ、スロットのバインディング、バージョン、本体のレングス、本体のSHA-256があります。本体はそのダイジェストで認証されます。
- **キーの選択。** 検証器が受理するのは、ヘッダが指定するidのキーだけで、そのidがビルド時に検証するイメージに埋込まれたプロジェクトのキーの場合だけです（`DAGON_PUBKEY`。公開のテストキーは、そのキーを選択したビルドだけ）。デバイスにキーの一覧はなく、予備のキーもありません。
- **誰が何を検証するか。** エージェントは`written`に先行してイメージを検証し、ブートローダは全ブートで`chosen`に先行して検証します。待機のスロットと確定済のスロットの両方が対象です。状態ログにはキーがないので、記録だけでイメージを信頼することはありません。署名の検査は、ブートごとに起動するスロットの1回です（待機のスロットを拒否して確定済のスロットを検証する場合は2回です）。署名が検証不可能なスロットは決して起動せず、ログが指定しないスロットを起動することもありません。ブートローダのイメージは、エージェントがこの規則で`written`と`pending_set`の各時点の直前に再度検証します（SLOT_PENDINGの記録がそのヘッダのダイジェストを指定します）。

  stage0のあるself-bootのマイコンでは、stage0はダイジェストとともに、マイコン定義が指定する位置（`[stage0] key`。位置は、stage0自身のフラッシュに組込んだ定数の`flash`だけです）にあるプロジェクトのキーで、ブートローダのヘッダの署名を検証します。コアを起動しうる全経路で検証します。待機のコアと確定済のコアです。起動可能なコアがない場合に起動する確定済のアプリも、同一のキーで検証します（1.2）。署名が検証不可能なコアは決して起動しないので、署名のキーなしでフラッシュに書込可能なコードが、自身のコアをstage0に起動させることは不可能です。stage0の検証器（`Ed25519Small`）が受理する対象は、ホストとエージェントが受理する対象と完全に同一です。キーはstage0の一部で、stage0に現場で書込することはありません。プロジェクトの署名のキーを交換するには、新規キーでstage0を再ビルドし、マイコンのROMローダで書込します。新規キーでビルドしたブートローダは、旧キーで署名したアプリを起動しません。そのアプリは、新規キーで署名して再書込するまで起動しません。

  stage0が起動するのは、ログが指定するコアだけです。起動回数が残存する待機のコアか、それ以外は確定済のコアです。待機のコアが検証不可能な場合（破損したスロット、コアのないスロットを指定する記録、または検証不可能な署名）は確定済のコアを選択し、それも検証不可能な場合や、起動可能なコアをログが指定しない場合は、確定済のアプリを起動します（1.2）。記録のないスロットを試行することはありません。そのアプリも不在の場合は、何も起動せずにROMローダでの復旧を待機し、その理由をノードの診断の出力に通知します。定義の`[boot]`にLEDと点滅する停止のパターンがあるボードでは、LEDが理由の番号（ヘッダが1、ヘッダのダイジェストが2、サイズが3、イメージの種類・マイコン・スロットが4、ロードアドレスが5、本体のダイジェストが6、署名が7、起動可能なコアをログが指定しない場合が8）の回数だけ、そのパターンの5倍の速度で点滅し、1周期の間消灯し、それを反復します（そのパターンの周期は10 ms以上です。周期がそれより短時間のボードは読込時に拒否します）。ボードのないノードや、そのLEDがないボード、停止のパターンが消灯のボードでは何も表示されません。どちらでもstage0の判断は同一です。

  stage0のないself-bootのマイコンでは、ブートローダがルートで、書込前にホストが、書込の経路で書込時はエージェントが検証します。
- **stage0の上限。** マイコン定義の`[stage0] region_bytes`（`[layout] stage0`のサイズ）はstage0の全体です（ベクタテーブル、ログの読込側、検証器を使用する検査、試行のセル、診断の出力）。イメージが領域の1/16を未使用領域として確保しないと、リンカスクリプトがリンクを失敗させます。RP2040: boot2（ブートROMの256バイトの第2段階）を包含して16 KiB。STM32F401RE: 16 KiB、コアがリセットベクタを取得するセクタ0の全部（リセットベクタを包含する最小の消去単位なので、どの書込もstage0を消去しません）。`scripts/stage0_size.py`は、ボードのないビルドと、定義がそのマイコンを指定するボードごとに1個をビルドします。ボードは診断の出力をstage0に追加するからです。さらに、イメージの部分（検証器、SHA-2、dagon-core、dagon-format、マイコンのコード、その残余）のそれぞれを、パッケージの`stage0/budget.toml`が付与する最大のバイト数とリンカのマップで比較します。stage0に現場で書込することはありませんが、新規ノードごとにそのビルドのツールチェーンでビルドします。そのため、余裕のないイメージは、ビルド不可能な最初の書込になります。どちらのstage0もコンパクトなSHA-256のラウンドを使用します。展開したラウンドはRP2040の領域に収容不可能なためです。最初の公開リリース以降、stage0の領域は固定です。領域を拡大したstage0は新規の形式番号で、既存のノードはROMローダによる最初の書込（直接書込）で移行します。通常の書込経路はstage0の書換の対象外のためです（電断時の安全性）。
- **stage0のクロックと検証時間。** stage0は、定義がノードに選択するクロックで検査します。マイコン定義の`[[stage0.clock]]`のうちボードの`[clock] source`に一致する設定（`out_hz`が最大のもの）で、一致する設定がなければ（ボードのないノード、`[clock]`のないボード、どの設定にも一致しない供給元）マイコンの内部の発振器です。ノードのstage0のビルドが、選択した設定をstage0に書込します。stage0はコアの起動前に、クロックをチップの起動時の状態に復元します。そのため、どのボードでもブートローダは同一のクロックから開始します。診断の出力と試行のウォッチドッグは、それぞれが動作するクロックで時間を計測します。`Ed25519Small`による署名の検査は、Cortex-M0+で約5,000万コアサイクルです（`crates/dagon-ed25519/tests/m0_cycles.rs`がARMv6-Mのインタプリタで、フラッシュの待機なしで実行し、5,500万を超過すると失敗します）。RP2040では、公称6.5MHzのリングオシレータで約7.7秒（その1.8から12MHzの幅で4.2秒から28秒）、ボードの12MHzの水晶から125MHzで約0.4秒です。ブートローダも同一の`Ed25519Small`で署名を検証し、SHA-2はコンパクトな手順で計算します。書込のセッションの領域を確保するためです。ブートローダは毎回のブートで起動するアプリのヘッダを検証するので、ブートごとにこの検査が1回追加されます。検査の各段階では、ウォッチドッグをキックします。`verify-time`の機能を付与してビルドしたstage0は、この時間をノード上で計測します。検査のそれぞれをコアのクロックのサイクルで計測し（SysTick）、最長の値を`DAGON_VERIFY_CYCLES`に、その回数を`DAGON_VERIFY_COUNT`に記録してデバッグプローブから読込可能にし、最長の値を0.1秒単位で切上げて診断の出力で点滅回数として提示し、コアは起動しません。計測後は、そのノードに通常のstage0を再書込します。ブートローダとアプリも同様にクロックを取得します。マイコン定義の`[[bootcore.clock]]`と`[[app.clock]]`のうち、ボードの`[clock] source`に一致する設定で、それぞれのHALが選択した設定を構成します。ボードが記述していない水晶を前提にする段階はありません。RP2040では水晶だけがアプリとブートローダにUSBのクロックを付与するので、上りのリンクがUSBのノードは、水晶のないボードではビルド不可能で、そのボードのブートローダは、ボードのDAGONのI2CとUARTのバスでだけ受信します。構成の確認（V-26）は、ノードごとにアプリが動作するクロックからUARTの速度を算出し、定義が許容誤差を記述していない内部の発振器で動作するUARTリンクを拒否します。
- **試行のウォッチドッグの下で消去しない。** stage0はフラッシュに書込しません。ブートローダは記録（BOOT_ATTEMPT、ROLLBACK、ブートローダのCONFIRMEDかROLLBACK）を、ログに既存の領域にだけ書込します（`Log::append_in_place`）。エージェントは、イメージを待機にする時点で、その試行用の領域を予約し（`Log::reserve`、`trial_room`）、電断で記録が途中で途切れた後は、アプリのエージェントが起動時にその領域を復元します（`SlotStore::keep_room`）。記録が途切れたログには、ブートローダは記録を追記不可能なので、記録不可能な試行は開始せずに確定済のスロットを起動し、ログ上は試行が未完了のまま残存します。そのため、稼働中のスロットはログから推定しません。STATUSの`active`は、アプリ自身が把握する起動スロット（リンク時のスロット。ESP32-S3では稼働中のパーティション）で、`Trying`はそのスロットが待機中の場合だけ報告します。CONFIRMは稼働中のスロットだけを確定するので、未起動のイメージを確定することはありません。消去はアプリの内部だけで、定義の確認が十分と証明したウォッチドッグの周期の下で実施します。`[watchdog]`の両方の周期は、`[execution]
  erase_regions`のどの領域の`unit_max_ms`と比較しても2倍以上です。
- **試行のセル。** 試行のセルが書込値を保持しないノード（`bootcore::cell_holds`、アプリのストアとブートローダが検証します）は、ブートローダを受信しません（XFERをunsupportedとして拒否します）。セルの破損を検出した待機中のコアは自身のロールバックを記録します。そのため、破損したコアを回数の制限なしで再試行することはありません。

- **ルートイメージ**（あればstage0、なければブートローダ）は、準備時にマイコンのROMローダで書込します。ROMローダはDAGONの要素を何も検証しません。ROMローダを実行可能な者はルートを置換可能で、同時に全規則を置換可能です。そのため、この規則が成立するのは、DAGONが提供する全書込経路（リンク、UDSのような置換した転送、コーディネータのリレー）に対してで、ROM経由の物理的な再書込に対してではありません。マイコンがルートイメージの保護を保持する場合は、定義がそのレベルを列挙し（`root_protection`: STM32では、`rdp1`は全消去で解除可能、`rdp2`は解除不可能で、セクタの書込保護`wrp`もあります。RP2040には何もなく、ブートROMは常にBOOTSELに従います。ESP32-S3にはセキュアブートとフラッシュの暗号化のeFuseがあります）、`dagon provision`がレベルの設定を提案して解除可能なレベルを明示し、VERSIONが正確なレベルと保護されたセクタを報告します（0x07）。そのためホストは、ルートが置換可能なノード、解除可能な形式で保護されたノード、解除不可能な形式で保護されたノードを明示します。

## 5. ボードの値、アクションプログラム、共有の資源、VERSION、モデルで決定する形式

### 5.1 ボードの値の出所は定義で、マイコンのコードではありません

`dagon-board.toml`は`[boot]`のテーブルを保持します。`led`（ピンの名称かGPIOの番号）、`led_active = "high" | "low"`、`halt = { pattern = "blink" |
"code" | "off", period_ms }`、任意の`power_hold = { pin, level }`、`[[boot.action]]`の項目（5.2）です。そして、拡張の能力を照合するテーブル`[[pin]]`、`[[bus]]`、`[[peripheral]]`、`[[device]] { bus, addr, writable = [regs] }`も保持します。定義の読込側は未知のキーを拒否するので、これらのテーブルを使用する定義は、それらを保持する`definitions`の形式を明示します（6）。`dagon gen`はブートローダのビルドのために`boot_wiring.rs`を出力します（`LED`、`HALT`、`PROGRAM`）。ボードの定義は、ブートローダのビルドが同様に読込します。マイコンのcrateはこれらの定数を受信し、自身ではピン、パターン、環境変数の既定を保持しません。

`dagon-mcu.toml`は、ボードではなくマイコンが決定する値を保持します。`[watchdog] trial_ms, confirmed_ms`、`boot_capabilities`（5.2）です。

### 5.2 ブートのアクションプログラム: ボードの事実のデータ

LEDのパターンや電源保持の線だけが必要でコードが不要なボードのために、ボードの定義はDAGONが解釈する宣言的で有界のプログラムを保持します。このプログラムはDAGONが解釈するデータで、拡張のコードではありません。アクションに使用可能なイベントで動作します。`WatchdogArmed`以降でだけ、宣言されたピン、バス、デバイスでだけ動作します。

```toml
[boot]
led = "GPIO25"
led_active = "high"
halt = { pattern = "blink", period_ms = 100 }
power_hold = { pin = "GPIO14", level = "high" }

[[boot.action]]
on = "watchdog_armed"              # any boot event marked action-capable in CATALOG
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
  Shared<I2c<'static, Blocking>>`（`dagon_events::Shared<T>`。`try_lock`で取得する`embassy_sync::mutex::Mutex`）として公開し、`with(|bus| ..)`で使用します。これは貸出で、クリティカルセクションではありません。貸出の間、割込は許可のままです。所要時間に上限のない操作を割込をマスクして実行せず（実行モデル）、バスのドライバが自身の割込を待機する場合があるからです。ネストや並行の`with`は、待機せずにビジーを返却します。DAGONの起動処理と復旧は`with`で取得し、アプリも常に同様にします。ボードの定義はデバイスの定数とレジスタのレベルの関数を公開するので、アプリは何もコピーしません。
- ボードの起動処理は、`app.bring_up`でのボードの定義の拡張です（`BoardBringUp`のポート）。利用者のステップは、同一イベントでのチェーンの後方の拡張か、`Latest.phase ==
  Serving`を待機するタスクです。
- ボードの定義が保持するのはボードで、その上に搭載するものではありません。搭載するペリフェラルのドライバは、そのドライバを使用するアプリ側にあり、ボードの定義はバスのピンを単なるペリフェラルとして受渡します。

### 5.4 VERSIONの照会

コマンド`Cmd::Version`で、リンクの規約の一部です。実装していないノード（復旧用のエージェント、最小のイメージ）は、未知のコマンドの規則で`E_UNSUPPORTED`を返却し、ホストはそれを"versions
unknown"と表示します。リクエストに欄はありません。レスポンス: 値ごとに1個の欄で、形式は全ペイロードと同一です（`key, len, bytes`。keyは以下の番号の2倍で、番号はレスポンス内で増加します）。読込側は未知の番号をスキップします。デバイスが決定不可能な値は省略し、番兵の値は使用しません。各値は最大255バイトで、レスポンス全体が最小のリンクペイロードに収容されます（10）。

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

`dagon status`は、ノードごとに1行で、これらの値を出力します。`dagon doctor`は形式番号を`compat.toml`と比較し、CLIが読込しない番号について警告します。

### 5.5 マイコンの識別とコンパイル済の構成: 要件と実行可能なモデル

このモデルのマイコンの識別は、リセット、電断、部分的な書込、バージョンの不一致のもとでのバイト単位の動作に正当性が依存するデータです。イメージのヘッダ、識別の返答、VERSION、コンパイル済構成のノードのレイアウトが保持します。そのような形式が故障のどんな組合せにも耐えることを、文章では証明不可能です。実行可能なモデルなら証明可能です。そのため、これらの形式はこの文書では規定しません。ランダムな故障の注入を伴うホスト側のRustのモデルとして設計し、性質に合格した形式が各形式の成文の仕様になり、モデルのテストを証明として公開します。

この節は、それらの形式が満たすべき要件を提示します。モデルを受理するのは、以下の全性質が故障の注入のもとで成立する場合だけです。合格した形式はモデルの性質の一覧と各主張の限界とともに、[retained-formats.md](/explanation/retained-formats/)で規定します。

#### マイコンの識別

- マイコンは手動で割当てた番号ではなく、定義の`id`から導出した値で識別します。マイコンの追加は定義の追加で、表の編集ではなく、数値のマイコンの表はどこにもありません（テストがソースを調査します）。
- 識別は一意です。定義の読込は識別を保持するどの欄の幅でも衝突する2個の定義を、両方のidを提示して拒否します。読込したどの定義とも一致しない識別のデバイスは不明と表示し、別の定義に一致させることはありません。
- イメージのヘッダ、識別の返答、VERSIONは、ホストとコーディネータにとって曖昧ではないとモデルが証明する形式でこの識別を保持します。位置ごとの幅とハッシュ関数は、モデルの出力です。

#### コンパイル済の構成

- 本体の形式（`topology-bin`）は、リリースのパッケージに格納され、ホストが読込します。ノードは構成を保存しません。形式は[retained-formats.md](/explanation/retained-formats/)の3で規定します。

#### 文章で規定する形式

この文書のその他のバイト単位の構造は、以下のいずれかです。ビルド時に生成してデバイスへの到達前に検証するもの（ブートのアクションプログラムの表、`boot_wiring.rs`の定数）、永続を伴わないリクエストとレスポンスの符号化（VERSIONの欄、serveのJSONのメッセージ）、このモデルが変更しない既存の規約（イメージのヘッダの署名とダイジェストの欄、状態ログのスロットの記録、ベンダーの選択データ、リリースのバンドル）です。いずれも2回の書込も、故障後の再読出もないので、モデルは不要です。

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

除去の手順: `start`の呼出を中止します。ブートローダは継続して検証と試行の計数を実施します。ホストは識別の返答を受信せず、`dagon status`がその旨を表示します。

どの拡張、イベント、定義、ブートのアクションも、ブートローダの検証対象を変更しません。

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

### 8.3 全イベントのログ（アプリかリレー）

```rust
pub struct DefmtLog;
impl Extension for DefmtLog {
    fn id(&self) -> &'static str { "defmt-log" }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx) { defmt::info!("{}", defmt::Debug2Format(e)); }
}
// … extensions![DefmtLog, MyApp]
// The log shows the generated order of 1.3: app.starting, app.store_opened,
// app.trial_started, app.watchdog_taken, app.bring_up, app.link_up …
```

または、応答のタスクの時間に影響しない非同期の方法もあります。`Reader`から`events.recent()`を取得するタスクです（所有された`EventRecord`、2.4）。

### 8.4 書込の転送とプロトコルの置換: CAN上のUDS

車載のノードには、シリアルリンク上のDAGONのリンクではなく、ISO-TP（ISO 15765-2）を使用するCAN上のUDS（ISO 14229）で通信する診断テスタが書込します。1個の拡張がポートを提供し、アプリで動作します。

マニフェストは以下のとおりです。

```toml
format = 1
id = "uds-can"
layers = ["app"]
provides = ["UpdateTransport", "UpdateProtocol", "Diagnostics"]
observes = ["app.transfer_*", "app.verify_*"]
decides = []
[capabilities]
peripherals = ["can1"]       # declared in the board definition's peripheral table
timer = true
[limits]
max_run_us = 500
```


この拡張は、アプリのチェーン内で、ポートをエージェントに提供します。応答のタスクが`UpdateTransport`からフレームを読込し（ISO-TPの組立は拡張の内部）、`UpdateProtocol`がUDSのサービスをセッションのトリガーに変換し、エージェントの返答が同一の経路で返送されます。対応付けは、`RequestDownload` → `update_offered`、`TransferData` → `header_chunk`/`data_chunk`、`RequestTransferExit` → `transfer_end`、`RoutineControl(switchSlot)` → `switch`、`ECUReset` → `reboot`です。セッションの状態機械は、DAGONのリンクと完全に同様に、トリガーごとにDAGONの全ガードと検証器のポートを実行します。そのため、テスタが署名をスキップすることも、稼働中のスロットに書込することも不可能です。拡張の`Diagnostics`のポートは、宣言した識別子について`ReadDataByIdentifier`に応答します（VERSIONが保持する値と同一の値に由来するバージョン）。`accept_update`と`self_check`は、ボードの定義、ライブラリ、アプリが担当します（ここでは`decides = []`）。

拡張がAPI経由で実行不可能な操作は、スロットのストアやログへの到達（ハンドルがありません）、スロットの確定（宣言する判断ではありません）、ブートローダの起動対象の変更です。

### 8.5 起動時の確定（テスト環境用）

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
| 拡張が応答する、ブート時の拒否権 | `Boot`をロールバックに変更する信頼されていない応答は、故障した拡張が有効なイメージを永久に起動させない事態を許容します。ブートの方針はDAGONが決定します。 |
| スロットのストアの変更メソッドを公開し、エージェントをその利用者の1個にする | 永続のイメージの状態が、イベントも確定の規則もなしに変化する可能性があります。イメージのライフサイクルの状態機械が唯一の書込側です。 |
| 新規タイミングごとにメソッドを追加する固定のフックのトレイト | 合成がなく、ディスプレイのためのペイロードのストリームもなく、部分の置換もなく、想定外のタイミングのたびに一覧の修正が必要になります。 |
| 手動で保守するカタログと照合する、手動で保守するイベントの列挙型 | 2個の一覧が一致しても、遷移が何も発行しない可能性があります。イベントの発行なしでは状態を変更不可能な基本要素だけが、カタログを派生物にします。 |
| リンカのセクションの登録簿（`linkme`） | 項目の順序が不定で、ライブラリによる不可視の登録があり、`dyn`のディスパッチのコストとサイズが発生します。 |
| デバイスの`alloc`を使用する`dyn Extension`の一覧 | ホストより下層にallocはありません。静的なスライスは登録簿の問題を再び持込みます。 |
| 全部をチャネルにする | 判断とポートには同期の応答が必要です。キューが適切な道具なのは公開の状態だけです。 |
| 待機可能なリングに借用のペイロードを残存させる | 静的なリングは作成側の存続期間を超過して参照を保持不可能です。生成される所有された短縮形（2.4）は有界で、表示やログのタスクに十分です。 |
| デバイスからホストへの要求外のイベントのフレーム | リンクは、コーディネータがホップごとに中継するリクエストとレスポンスです。誰も要求していないフレームには、経路も、再試行の連番も、リレーの無通信時間モデルにおける位置もありません。`EVENT`のフレームのフラグは予約で、使用しません。ホストはデバイスの遷移を、返答とVERSIONから導出します。 |
| ビルド時の環境変数でのボードの値 | 定義にない、形式を変更したマイコンの知識です。ボードの定義の`[boot]`に置換しました。 |
| アプリがコンパイル時に対象としたブートローダのバージョンを報告する | インストール済の対象を識別しません。VERSIONはインストールされたイメージをフラッシュから読込します。 |
| 再起動の遅延に対する固定の上限 | 1回決定した数値は、試行のウォッチドッグの周期が短期のマイコンでは誤りです。上限はマイコン定義の周期から導出します。 |
| 保持メモリと識別の形式を文章で規定する | リセット、部分的な書込、バージョンの不一致のあらゆる組合せに正当性が依存する形式の正当性を証明するのは、文章ではなく、ランダムな故障の注入を伴う実行可能なモデルです（5.5）。 |

## 10. サイズの上限とその出所

どの上限にも唯一の出所があり、その上限に収容されるべき箇所を全部列挙し、テストが各組を検証します。

| 上限 | 値 | 唯一の出所 | 収容されるべき対象 |
|---|---|---|---|
| 種類ごとのリンクのペイロード | USB 1088、UART 1024、I2C 256バイト | `dagon_proto::max_payload` | 全リクエストと返答、VERSIONのレスポンス、表示の文字列、適合試験の詳細 |
| 最小のリンクのペイロード | `max_payload::MIN` = 前述の種類の最小値（導出値で、記述値ではありません） | `dagon-proto` | VERSIONのレスポンス全体（デバイスが応答する全TLVの集合） |
| VERSIONの値 | ≤ 255バイト | VERSIONの符号化（`dagon-proto`のコマンドの表） | 5.4の全番号 |
| 拡張の割当時間 | `max_run_us ≤ APP_EXT_MAX_US` | マイコン定義の`[watchdog]`に由来する`dagon-events`の`const fn` | `dagon gen`の検証、応答のタスクの計測、マニフェスト |
| ブートのアクションプログラム | イベントごとに`ops ≤ 32`、`delay total ≤ 50 ms`、`bus_write`ごとに`bytes ≤ 8`、`delay ≤ 20 ms` | `dagon_format::bootprogram`の定数 | `dagon gen`の検証、ブートローダのインタプリタ、`boot_wiring.rs`の表の型 |
| liveのイベントのペイロード | ブートのイベントは ≤ 16バイト（値で）。それ以上のアプリやホストのペイロードは参照で | `dagon-events`のサイズのアサーション | `Published`のスナップショット、短縮形 |
| `EventRecord` | 24バイト: `layer`、`kind`、`flags`、`dropped`、`seq u32`、`payload [u8; 16]` | `state_machine!`の`compact`属性（生成されたテストが、全種類の保持を検証します） | `Recent<N>` |
| カタログ | 数値の上限なし。`kind`はレイヤーごとの`u8` | `state_machine!` | 記録の`kind u8`（生成されたテストが、レイヤーの256種類で失敗します）、serveのストリーム |
| 最近のリング | レイヤーのcrateが選択する`N` | `Published<S, N>` | レイヤーのcrateの`Published`。溢れると`dropped`を計数します |
| 判断の上限 | 50 ms、`REBOOT_DELAY_MAX = min(trial_ms, confirmed_ms) / 2` | `dagon-events`の定数、マイコン定義の`[watchdog]` | 応答のタスクのタイマ、`boot_wiring.rs` |

## 11. 実行モデル

DAGON自身のコードについて、各処理の実行位置、DAGONのコードが割込をマスク可能な時間長、フラッシュの操作が停止させる対象を、マイコンごとに提示します。この節は規範です。マイコン定義のコードはこの節に従って記述され、マイコン定義はそのハードウェアの事実（`[execution]`、`[watchdog]`）を保持し、以下に列挙する確認がコードとデータをこの節に従わせます。

### 11.1 ルール

1. **処理はハンドラではなくタスクで実行します。** DAGONの応答のループ、リレー、ウォッチドッグへのキックは、エグゼキュータ上のタスクとして動作します。DAGON自身の割込ハンドラは、タスクの起動かバイトの移動だけを実行します（リンクのドライバ、エグゼキュータ自身の割込）。待機、消去、フラッシュへの書込をする処理が、ハンドラ内で動作することはありません。
2. **マスク時間の上限。** DAGON自身のコードのクリティカルセクション（ウォッチドッグのガードの状態、ロックの状態）が割込をマスクするのは、最大でマイコン定義の`[execution] masked_max_us`です。フラッシュの消去や書込、その他のマイクロ秒単位の上限のない操作を、その中で実行することはありません。複数の利用者を保持するフラッシュドライバは、操作のためにマスクせずに*取得する*ロックで共有します（`dagon_mcu_common::shared::SharedFlash`、`try_lock`。取得済と判明したハンドルは即時にエラーを返却し、待機しません）。clippyが強制します。`targets/mcu/clippy.toml`は、マイコン定義とブートイメージ（stage0、ブートローダ）内の全マスクの基本要素（`critical_section::with`と`acquire`、`cortex_m::interrupt::free`と`disable`、クリティカルセクションの`blocking_mutex::Mutex::lock`）を、解決したパスで拒否するので、別名やラッパーでは隠蔽不可能です。マスクする関数（ウォッチドッグのガードにおける状態の変更）は、理由とともに`#[allow(clippy::disallowed_methods)]`を保持し、テストが一覧と例外をレビュー済の状態に維持します。
3. **フラッシュの操作はハードウェアの事実です。** 消去や書込が停止させる対象は、定義の`[execution] flash_op`です。
   - `fetch-stall`: コードが動作するシングルバンクのフラッシュ。命令の取得とフラッシュの読出は、操作の終了まで停止します。割込は許可のまま待機し、何も動作しません（DAGONはRAMにハンドラを配置しません）。
   - `xip-off-masked`: 操作はexecute-in-placeを無効にしてRAMから動作します。ハードウェアの要求どおり、フラッシュドライバはその1回の操作の間割込をマスクします（そして他方のコアを停止させます）。
   - `cache-off-masked`: 操作の間フラッシュのキャッシュは無効です。フラッシュドライバはその1回の操作の間割込をマスクします（内部RAMの外部のハンドラは動作不可能です）。

   どの場合も、DAGONは1回の操作で、消去する領域（`[execution]
   erase_regions`）の消去単位を1個発行し、所要時間の最大はその領域のデータシートの最大値`unit_max_ms`です。エージェントは全消去を定義の領域と`[[slot_layout]]`から計画し、スロットのストアは計画した操作だけを実行します。ホストは同一の計画を待機します（[消去と試行の待機時間](/reference/definitions/#消去と試行の待機時間)）。
4. **ウォッチドッグの周期は1回の消去より長期です。**どの領域の`unit_max_ms`の2倍も、`[watchdog] trial_ms`と`confirmed_ms`以下です（定義の読込時に検証します）。どのリクエストも応答のループの`ServeHost::feed`（RP2040のリレーにおけるPC側のループでは、ガードの`progress`）経由の、条件付きのキックで開始します。複数の単位の消去では、単位の間にキックするか、進捗を報告します。待機中のイメージの試行は独立した期間です。その期間のキックの有無は、ガードの試行の方針が決定します（`app.watchdog_taken`が報告します）。
5. **停止中のリンク。** リクエストは新規リクエストに対してリンクをクローズして処理します（I2Cのターゲットは自身のアドレスにNACKし、ストリームのリンクの対向側は返答を待機します）。CPUの停止中に到達したデータは、消失するかハードウェアが保持します。要求側は同一の連番で再試行し、ノードは反復したリクエストに、作成済の返答で応答します。
6. **リセットは返答の出力後だけ。** 再起動は返答の書込とフラッシュの完了まで待機し、その後に`allow_reboot_now`が要求した遅延の間、応答のタスクをアイドルにして待機します（ガードはキックを継続します）。送信不可能だった返答では何もリセットせず、要求側の再試行は同一の返答を取得します。
7. **事実は検証の程度を明示します。** 定義のハードウェアの事実は、出所と照合して検証した数値（定義のコメントが文書と表を明示します）、`{ unverified = N }`（出所と未照合の値）、または`"unmeasured"`（値がない）です。ツールと文書は後者2種類を未検証と報告します。値が必要な確認はunverifiedの値を使用します。リリースがそのいずれかに依存することはありません。

### 11.2 マイコンごと

表はマイコン定義のコードの処理で、上限はそれぞれのマイコン定義の値です。

**STM32F401RE**（`flash_op = "fetch-stall"`、消去単位はセクタ1個。領域により16、64、128KiB）

| 処理 | 実行位置 | 優先度 |
|---|---|---|
| I2Cターゲットのリンクの応答（エージェント、CONF_RUN）、フラッシュの消去と書込 | DAGONの割込エグゼキュータ（USART6のベクタ） | P6、threadモードより上 |
| リンクのI2CとDMAの転送 | I2Cドライバの割込 | HALの既定 |
| タイマ（`embassy-time`） | そのタイマの割込 | HALの既定 |
| ウォッチドッグへのキック | アプリのキックのタスク、ガードが条件を検証します | threadモード |

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

### 11.3 上限の決定方法

マイコンごとの上限は、ボードでの以下の値から決定します。DAGON自身のコードで最長のクリティカルセクション（`masked_max_us`: 最悪の値に余裕を加算した値）、ボードに搭載したフラッシュ部品の最悪の消去時間（と、定義が明示するそのデータシートの値）、1回の消去の終了時に残存するウォッチドッグの余裕、その後のリンクの回復（PC側のノードの再試行への応答）です。
