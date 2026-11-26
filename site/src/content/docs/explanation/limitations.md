---
title: "制限事項"
description: "DAGONの設計上の範囲外、利用者の責任範囲、設計とコードの差異です。"
---

対象読者はアプリ開発者、拡張作者、DAGON貢献者です。設計とコードの差異の記載箇所は、この頁の最終節だけです。

## 設計上の範囲外

- マイコン定義はイメージの種類ごとにスロットを1個または2個付与します（`[[slot]]`）。スロット1個の定義を読込可能なのは、そのスロットに書込する別の種類を`[writers]`が明示する場合だけです。スロットが1個の種類で不可能な動作は以下のとおりです。

| イメージの種類 | 前版 | 書込中の電断 | 復旧するイメージ |
|---|---|---|---|
| `app` | 残存しません。試行に不合格の新版にも復帰先がありません。`dagon rollback`は前版の不在を応答し、DAGON Consoleはそのノードの復帰操作を表示しません。 | スロットに起動可能なアプリが残存しません。 | アプリを書込する別の種類（`[writers] app`）が起動し、アプリを再度受信します。ESP32-S3では、2nd stageが工場出荷時の復旧用エージェントを起動します。 |
| `bootcore` | 残存しません。試行に不合格の新規ブートローダから旧ブートローダへの復帰は不可能です。 | stage0には起動可能なブートローダが存在しなくなり、状態ログにある確定済APPの記録が指定するアプリを、検証後に起動します。アプリの待機中と試行中は、アプリのエージェントがブートローダの転送とその待機設定を拒否し、ブートローダが確定済でない場合（待機中、または中断した書込で空白のスロット）はアプリの待機設定を拒否するので、この電断とブートローダの試行がアプリの試行と重複することはありません。 | その確定済アプリ（`[writers] bootcore`）がブートローダを再度書込します。確定済アプリが不在の場合だけ、stage0は何も起動せずにROMローダでの復旧を待機します。 |
| `agent` | 残存しません。 | 確定済アプリは動作を継続します。復旧用エージェントは再度の書込まで使用不可能です。 | 確定済アプリ（`[writers] agent`）が再度書込します。 |
| `stage0` | 残存しません。DAGONはstage0を現場で書込しません。 | 現場で書込しないので発生しません。 | マイコンのROMローダ（`dagon flash`）。 |

- ブートローダは中継しません。書込モードか復旧モードでブートローダに滞留中のノードの経由でしか到達不可能なノードには、そのノードのアプリ起動後に再度到達します。そのノードへの書込を最初に実施してください。

- ESP32-S3は公式のESP-IDFの2nd stageを使用します（compat.tomlのポート`vendor-stage2`）。2nd stage自体の破損はDAGONの復旧範囲外です。復旧にはベンダーのツールでのUSB経由の再書込が必要です。
- `dagon recover`は、ESP32-S3の復旧用エージェントをチップのROMシリアルローダー経由で再書込します。経路は中継ノードのUARTリンクです（UART_RAW）。無応答のノードは、60秒以内に手動でローダーへ切替する必要があります。
- RP2040の起動所要時間は、水晶非搭載のボード（リングオシレータ、公称6.5MHz）で約15秒、12MHzの水晶搭載のボードで約0.8秒です。stage0とブートローダが、毎回の起動時に`Ed25519Small`による署名検査を各1回実行するためです。ブートローダは検査中もウォッチドッグのキックを継続します。これは設計上の選択です。小規模な検証器の採用により、ブートローダ内に書込セッション用の領域を確保しています。
- リングオシレータで動作するボードでは、RP2040のブートローダがUARTのビット速度を公称6.5MHzから生成するため、速度に誤差が発生する場合があります。また、400kHzのバスに追従可能なI2Cターゲットは、水晶搭載のボードに限定されます。リングオシレータ使用時は100kHzを設定してください。
- DAGON Consoleは構成のYAMLを独自の書式で再出力します。手書きのコメントを保持するのは、内容が無変更の行（字下げは比較対象外）だけです。DAGON Consoleで変更または削除した行と、DAGON Consoleと異なる書式（引用符、フロー形式など）で記述した行のコメントは保持しません。空行の位置もDAGON Consoleの書式に準拠します。全コメントの保持にはYAMLの構文木を保持する処理系が必要で、DAGON Consoleはその処理系を搭載していません。

## 利用者の責任

- 署名キーの保護方法の選択は利用者の責任です。DAGONは保護方法を規定しません。ハードウェアの信頼の起点（セキュアブートのヒューズやハードウェアでのキー保管）の使用可否も、その選択の一部です。
- 拡張は利用者自身の信頼されたコードで、アプリとホストでだけ動作します。DAGONは拡張を隔離しません。拡張の内容、依存するcrate、取込むソースの審査は利用者の責任です。
- AIエージェント（serve APIに接続するcommandのエージェント）は利用者の権限で動作します。DAGONはエージェントにサンドボックスを適用しません。OSのサンドボックスの適用は利用者の責任です（[エージェントとの接続方法: serve API](/reference/agent-api/)）。
- USBのVID:PID `c0de:cafe`は試験用IDで、コードに固定されています。割当済のIDではありません。製品での使用可否の判断は利用者の責任です。
- 一部のUSBハブでは、PCとUSB接続したノードが自身の新版の書込後に再起動した際、PCがそのノードを再認識しない場合があります。そのノードは、PCのUSBポートに直接接続してください。

## 設計とコードの差異

コーディネータとブート:

- リレー（`dagon-relay`）はマイコン非依存で、リレーの可否はマイコン定義の`[coordinator]`で決定します。ノードはそのマイコン定義の`[[relay_link]]`が列挙するリンクで中継し、各ノードへの経路となるリンクは構成が決定します。その定義とリレーを実行するコードの両方を保持するマイコンはRP2040だけです。RP2040のリレーは、I2C0とI2C1、PIO0とPIO1のPIO UARTの最大4本のリンクで中継し、ボードが配線したリンクだけを使用します。
- 書込が可能な範囲は、PC接続ノードと、その接続先のノードまでです。接続先のノードが接続するノードへの書込は不可能です。
- 書込の到達範囲は、1台目のコーディネータと、I2CまたはUARTでそのコーディネータに接続したノードだけです。2台目以降のコーディネータとその配下のノード、その他の種類のリンクで接続したノードは、書込でスキップします。構成の検査は、それらを情報（◇）として表示します。
- 1個のノードには複数のノードからリンクを接続可能で、スター型とリング型の構成を定義可能です。同一のノードの組を接続するリンクは1本だけです。書込は、コーディネータと各ノードを直接接続するリンクと、中継可能なノード（MCUの定義が`[relay]`の`relays_as_node = true`を記述したノード）から配下のノードへのUARTのリンクを使用します。組込のMCUの定義は`relays_as_node`を未設定とするので、現時点の書込の対象はコーディネータに直接接続したノードだけです。中継するノードの段数は、コーディネータも1段として最大4です。到達済のノード間を接続するリンク（リングを構成するリンク）は書込で使用せず、構成の検査が情報（◇）として表示します。
- STM32F401のブートローダが受信するのは、ボードのDAGONのI2Cバス（`dagon = true`の`[[bus]]`）だけです。ボード未指定のノードと、そのバスがないボード上のノードでは、ブートローダは待受時間を設けず、書込モードもありません。待受時間内のSTAYかUPDATE_STAYで書込モードに移行しますが、復旧モードはありません。起動対象のアプリが不在の場合は停止します。アプリはREBOOTのmode 1に`E_UNSUPPORTED`で応答するので、書込モードへの移行は待受時間経由だけです。ESP32-S3の復旧用エージェントが受信するのは、アプリのスロットがいずれも起動しないために2nd stageがエージェントを起動した場合だけです。待受時間を設けず、HELLOには1や2ではなくmode 3で応答します。DAGON独自のstage0の背後でこれに代わるブートローダ（targets/mcu/esp32s3/bootcore：UART0の待受時間、アプリからの移行、復旧モード）は、ESP32-S3の定義がstage0の配置を規定するまでエミュレータ上でのみ動作します。このMCUのアプリは現時点では移行要求を発行しません（REBOOTのmode 1には`E_UNSUPPORTED`で応答します）。
- DAGON同梱のマイコンについて、stage0の最悪検証時間の実測値は記録していません。[stage0のクロックと検証時間](/explanation/lifecycle-events/)の値はCortex-M0+のエミュレータによる推定値で、STM32F401のCortex-M4の推定値はありません。
- RP2040のstage0、ブートローダ、アプリは、クロックを水晶かリングオシレータから取得します。XINに入力する外部クロックは使用しません。
- リセットの要因（`dagon status`の`reset`）を通知するのは、アプリのセッションだけです。ブートローダのセッション（書込モードと復旧モード）は通知しません。エミュレータ上のESP32-S3のブートローダ（targets/mcu/esp32s3/bootcore）は、リセットでアプリの起動を要求するので、そのアプリは常に`software`を通知します。ハブがノードのNRSTを操作した記録は存在しません。

設計から削除済で、コードに残存する構成要素:

- `dagon_events::PORTS`のbootのレイヤー、信頼の方針（`trusted only`など）とサービスの列、`Action`のポート、`Display`の注記（ブート時はサンドボックス内で動作）、`BootRecord`と`Sandbox`の記録。[ライフサイクルイベントと拡張](/explanation/lifecycle-events/)の2.1節の表は、この宣言から生成します。
- VERSIONの0x07、0x08、0x09、0x11、0x12の定義（`version_id::TABLE`）。5.4節の表はこの定義と同一です。
- ブート段階の拡張: 状態機械`boot_extensions`（`boot.extension_overran`）、`targets/boot-extension-template`、マニフェストの`trusted`、`stack_bytes`、`ram_bytes`、`dagon-extension`の保護マップの生成と信頼された拡張の検査。
- マイコン定義の`[isolation]`と`[[peripheral]]`。設計の`root_protection`は独立したテーブルです。コードでは`[isolation]`の欄で、コードの変更時に独立したテーブルに移動します。

リンク:

<!-- generated from the MCU definitions' [links]: python site/scripts/gen.py -->
- ファームウェアがあるリンクの種類は`usb-cdc`、`uart`、`i2c`だけです。`ble`、`wifi-tcp`、`ethernet`、`can`にはありません。
<!-- end of generated table -->

識別とVERSION:

- VERSIONが応答するのはid 0x01、0x06、0x0A、0x0B、0x0C、0x0Dだけです。サイズが0ではないリクエストは拒否します。

値:

<!-- generated from MAX_NODES and the sample hardware test setups: python site/scripts/gen.py -->
- 構成のノード数の上限は`MAX_NODES`の32です。ボードに書込するサンプルの試験の基本構成（`samples/hardware-test/hwt.toml`）は3ノードです。4ノード以上の書込の試験は、32ノードのシミュレータの試験（`dagon-host`の`one_write_reaches_every_node_of_a_topology_at_the_limit`）と、4ノードのボード用の試験構成（`hwt-fan.toml`）です。
<!-- end of generated table -->
- DAGONのマイコン定義には、`unverified`とマークした値があります。`dagon release`と`dagon deploy`は、リリースの対象ノードのファームウェアが使用する未検証の値（割込禁止の最長時間と消去時間）を警告として表示します。`dagon release --checked-facts`は、その値が残存するリリースを拒否します（RB-06）。
- 実行モデルの上限（[11節](/explanation/lifecycle-events/#11-実行モデル)）は、11.3の手順で決定した値ではありません。

設計の構成要素のうち、コードとの差異があるもの:

<!-- generated from dagon_events::PORTS: DAGON_BLESS=1 cargo test -p dagon-catalog --test docs -->
- 置換用のトレイトがコードに存在しないポート（括弧内は予定の配置）: `UpdateProtocol`（`dagon_agent::UpdateProtocol`）、`LogStorage`（`dagon_core::LogStorage`）、`Recovery`（`dagon_port::Recovery`）、`Diagnostics`（`dagon_agent::Diagnostics`）、`Display`（`dagon_events::Display`）、`LogSink`（`dagon_events::LogSink`）、`Action`（`dagon_host::Action`）
- `BootRecord`: ファームウェアはブート記録を保存しません。
- `Sandbox`: 実行時のサンドボックスは存在しません（Service ABI、Entry Gate、Operation Cancellation、Fault Unwind、Lockup Fuse、Reset Cleanup、SandboxArch）。定義の検査と保護マップの生成は実装済みです。
<!-- end of generated list -->
<!-- generated from site/scripts/architecture.py: python site/scripts/gen.py -->
- AI Agent Link: DAGON Consoleにボード移植の開始画面は存在しません。開始方法はCLIの`dagon ai port`と`POST /v1/ai/port`です。移植の実行は模擬プログラムとの試験と、実機なしの起動確認だけで、実機での完走は未確認です。
- Boot Action Interpreter: コードに存在しません。
- Retained Codecs: ファームウェアに保持形式のコーデックは存在しません。構成本体の形式は`dagon-format`の形式（先頭32バイト、ノード40バイト）で、[保持と永続の形式](/explanation/retained-formats/)の3.1に記載しています。ノードは構成を保存しません。構成はホストのパッケージにだけ存在し、ターゲット3（旧来の構成の保存先）は予約番号として残存し、どのノードも受理しません。
- ROM Loader Client: ESP32-S3のROMシリアルローダー経由の書込（`dagon recover`）は、模擬ローダーとの試験だけです。実機では、ENTER_ROM後にローダーがリンクのUARTで受信すること、中継ノードのUARTがローダーの応答を欠落なく受信することが未確認です。
<!-- end of generated table -->
