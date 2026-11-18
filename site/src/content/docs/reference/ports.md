---
title: ポート
description: DAGONの拡張が使用する全ポートです。
editUrl: false
---

:::note
`dagon_events::PORTS`から生成しています。直接編集せず、
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`で再度生成してください。
:::

ポートは、拡張による置換が可能なDAGONの部品です。置換は、一覧のレイヤーで、リンクした
イメージの内部だけで有効です。既定はDAGON自身の実装です。拡張のマニフェストは、そのポートがある
レイヤーでだけポートを提供可能で、1つのイメージで1つのポートを提供可能な拡張は1つだけです。

信頼の区分:

- *untrusted*: どの拡張でも使用可能です。レイヤーのサンドボックス内で、マニフェストで宣言した能力で
  動作し、DAGONは一覧のサービス経由でのみ呼出します。
- *untrusted, additive*: untrustedと同一で、追加だけが可能です。DAGON自身の実装は残存します。
- *trusted only*: 宣言して署名した、信頼する拡張だけが使用可能です。特権で動作し、サービスは呼出しません。

| ポート | 役割 | レイヤー | 既定 | 信頼 | サービス |
|---|---|---|---|---|---|
| `UpdateTransport` | リンクに入出力するバイト | boot, app, relay, host | boot, app, relay: USB CDC、UART、I2Cターゲット、PIO UART; host: シリアル | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` | メッセージ ↔ セッションのトリガー: 識別、ステータス、転送、切替、再起動、確定 | boot, app, relay, host | DAGONのリンク | untrusted (ガードの省略は不可能です。ガードを実行するのはセッションの状態機械で、プロトコルではありません) | return, time, memory |
| `ImageVerifier` | ヘッダの署名、キーの検索 | boot, app | プロジェクトのキーによるEd25519 | trusted only | – |
| `Digest` | 本体のハッシュ | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` | 検証器とダイジェストが使用する基本要素。ハードウェアによる高速化、HSM | boot, app, host | ソフトウェア | trusted only | – |
| `DecisionPolicy` | 試行の上限、優先するスロット | boot | 定義の`max_attempts`、確定より待機を優先 | trusted only | – |
| `WatchdogDriver` | ブートと試行を保護するウォッチドッグ | boot, app | マイコンの定義 | trusted only | – |
| `SlotStore` | イメージの格納先 | boot, app | マイコン定義のフラッシュの配置 | trusted only | – |
| `LogStorage` | 状態ログの格納先 | boot, app | マイコン定義のフラッシュの配置 | trusted only | – |
| `Recovery` | 追加の復旧経路: 転送用のサービスの時間枠、ベンダーのダウンロードモード | boot, app | マイコンとボードの定義に従うBOOTSEL/UF2、factory、ROMの入口 | untrusted, additive (拡張は経路の追加だけが可能で、DAGONの経路は削除不可です) | return, transport, time, memory |
| `Diagnostics` | 追加のVERSIONのTLV、追加のステータス、UDSのDIDの表 | boot, app, relay | なし | untrusted | return, time, memory |
| `Display` | 段階やブートの結果の表示方法 | boot, app, relay | boot: アクションプログラム経由の、ボード定義の`[boot] led`と`halt`; app, relay: なし | untrusted (ブート時は宣言済のピンとバスを使用してサンドボックス内で動作します) | return, pins, buses, time, memory |
| `LogSink` | `Recent`の溢れと拡張のログの出力先 | app, relay, host | なし | untrusted | return, events, time, memory |
| `SelfCheck` | 試行中スロットの確定時における健全性の応答 | app | 健全 | untrusted (判断点、3) | return, time, memory |
| `BoardBringUp` | アプリ起動時のボードの電源、ピン、バス | app, relay | ボードの定義 | untrusted | return, pins, buses, time, memory |
| `NodeLink` | `dagon_port::NodeLink`による、リンク経由での別ノードへの到達方法 | relay | I2Cコントローラ、PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` | イメージとリリースの署名 | host | ファイルのキーの組 | untrusted (ホストは定義上信頼されます。検証の担当はデバイスです) | – |
| `KeyStore` | 利用者が選択した格納先（環境変数、OSのキーストア、1Passwordなど）から署名のキーを受信 | host | ファイルのキーの組 | untrusted (ホストは定義上信頼されます。検証の担当はデバイスです) | – |
| `BuildRunner` | ノードのイメージのビルド | host | cargo | untrusted | – |
| `Agent` | CLIとページがリクエストを委託する外部のエージェント | host | データ定義（エージェントの定義） | untrusted | – |
| `Action` | ホストのイベントの後に実行するプログラムやリクエスト | host | データ定義（アクションの定義） | untrusted | – |

## ポートではない部品

信頼する拡張でも、以下の部品は置換不可です。

| 部品 | 内容 | レイヤー | ポートではない理由 |
|---|---|---|---|
| `ImageLifecycle` | スロットのストアと状態ログの上位にある、イメージのライフサイクルの状態機械 | boot, app | 永続イメージ状態の唯一の書込元。どの変更もイベントを発行し、確定の規則を維持します |
| `SessionGuards` | 書込のセッションのガード: ターゲット、スロット、ヘッダの署名と互換性、本体のダイジェスト | boot, app | 全プロトコルのトリガーが同一のガードを経由します。置換すると、DAGONが拒否するものを転送が書込可能になります |
| `BootRecord` | アプリがリプレイするブートのイベントの保持記録 | boot, app | その形式はDAGONの保持の規約で、ブートや確定の判断の入力にはなりません |
| `Composition` | レイヤーのチェーンの順番（公開元、ボードの定義、ライブラリ、利用者）と判断の上限 | boot, app, relay, host | 全部の拡張に全部のイベントを配信し、応答を有界に維持する機構です |
| `Sandbox` | 入口のゲート、サービスの呼出、保護マップ、ロックアップヒューズ | boot, app, relay | 拡張をDAGONの状態から隔離する機構です |
