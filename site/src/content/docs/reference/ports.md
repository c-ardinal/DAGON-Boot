---
title: ポート
description: DAGONの拡張が使用する全ポートです。
editUrl: false
---

:::note
`dagon_events::PORTS`から生成しています。直接編集せず、
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`で再生成してください。
:::

ポートは、拡張が置換可能なDAGONの部品です。置換は、一覧のレイヤーで、リンクされた
イメージの内部のみで有効です。既定はDAGON自身の実装です。拡張のマニフェストは、そのポートが存在する
レイヤーにおいてのみポートを提供可能です。1つのイメージ内で1つのポートを提供可能な拡張は1つのみです。

信頼の区分:

- *untrusted*: 全拡張が使用可能です。レイヤーのサンドボックス内で、マニフェストで宣言した能力の範囲で
  動作します。DAGONは一覧のサービス経由でのみ呼出します。
- *untrusted, additive*: untrustedと同様で、追加のみです。DAGON自身の実装は残存します。
- *trusted only*: 宣言と署名を経た信頼する拡張のみが使用可能です。特権で動作し、サービスは呼出しません。

| ポート | 役割 | レイヤー | 既定 | 信頼 | サービス |
|---|---|---|---|---|---|
| `UpdateTransport` | リンクに出入りするバイト | boot, app, relay, host | boot, app, relay: USB CDC、UART、I2Cターゲット、PIO UART; host: シリアル | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` | メッセージ ↔ セッションのトリガー: 識別、ステータス、転送、切り替え、再起動、確定 | boot, app, relay, host | DAGONのリンク | untrusted (ガードは飛ばせません。ガードを実行するのはセッションの状態機械で、プロトコルではありません) | return, time, memory |
| `ImageVerifier` | ヘッダの署名、鍵の検索 | boot, app | プロジェクトの鍵でのEd25519 | trusted only | – |
| `Digest` | 本体のハッシュ | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` | 検証器とダイジェストが使う基本要素。ハードウェアの加速、HSM | boot, app, host | ソフトウェア | trusted only | – |
| `DecisionPolicy` | 試行の上限、どのスロットが勝つか | boot | 定義の`max_attempts`、確定より待機を先に | trusted only | – |
| `WatchdogDriver` | ブートと試行を守るウォッチドッグ | boot, app | マイコンのもの | trusted only | – |
| `SlotStore` | イメージの置き場所 | boot, app | マイコンの定義のフラッシュの配置 | trusted only | – |
| `LogStorage` | 状態ログの置き場所 | boot, app | マイコンの定義のフラッシュの配置 | trusted only | – |
| `Recovery` | 追加の復旧の経路: 転送のためのサービスの時間枠、ベンダーのダウンロードモード | boot, app | マイコンとボードの定義のとおりのBOOTSEL/UF2、factory、ROMの入り口 | untrusted, additive (拡張は経路を足すだけで、DAGONのものを消せません) | return, transport, time, memory |
| `Diagnostics` | 追加のVERSIONのTLV、追加のステータス、UDSのDIDの表 | boot, app, relay | なし | untrusted | return, time, memory |
| `Display` | 段階やブートの結果の見え方 | boot, app, relay | boot: アクションプログラムを通した、ボードの定義の`[boot] led`と`halt`; app, relay: なし | untrusted (ブート時は、宣言されたピンとバスでサンドボックスの中で動きます) | return, pins, buses, time, memory |
| `LogSink` | `Recent`のあふれと拡張のログの行き先 | app, relay, host | なし | untrusted | return, events, time, memory |
| `SelfCheck` | 試行中のスロットを確定するときの健全さの答え | app | 健全 | untrusted (判断点、3) | return, time, memory |
| `BoardBringUp` | アプリの起動時のボードの電源、ピン、バス | app, relay | ボードの定義のもの | untrusted | return, pins, buses, time, memory |
| `NodeLink` | `dagon_port::NodeLink`で、ノードがリンクの先の別のノードに届く方法 | relay | I2Cコントローラ、PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` | イメージとリリースの署名 | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `KeyStore` | 利用者が選んだ置き場所（環境変数、OSのキーストア、1Passwordなど）から署名の鍵を受け取る | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `BuildRunner` | ノードのイメージのビルド | host | cargo | untrusted | – |
| `Agent` | CLIとページがリクエストを渡す外部のエージェント | host | データで定義（エージェントの定義） | untrusted | – |
| `Action` | ホストのイベントの後で実行するプログラムやリクエスト | host | データで定義（アクションの定義） | untrusted | – |

## ポートではない部品

信頼する拡張でも、以下の部品は置換不可です。

| 部品 | 内容 | レイヤー | ポートではない理由 |
|---|---|---|---|
| `ImageLifecycle` | スロットのストアと状態ログの上のイメージのライフサイクルの状態機械 | boot, app | 永続のイメージの状態の唯一の書き手。どの変更もイベントを出し、確定のルールを保ちます |
| `SessionGuards` | 書き込みのセッションのガード: ターゲット、スロット、ヘッダの署名と互換性、本体のダイジェスト | boot, app | どのプロトコルのトリガーも同じガードを通ります。置き換えると、DAGONが拒むものを転送が書けてしまいます |
| `BootRecord` | アプリがリプレイするブートのイベントの保持の記録 | boot, app | その形式はDAGONの保持の取り決めで、ブートや確定の判断の入力にはなりません |
| `Composition` | レイヤーのチェーンの順（公開元、ボードの定義、ライブラリ、利用者）と判断の上限 | boot, app, relay, host | すべての拡張がすべてのイベントを見られるようにし、答えを有界に保つものです |
| `Sandbox` | 入り口のゲート、サービス呼び出し、保護マップ、ロックアップヒューズ | boot, app, relay | 拡張をDAGONの状態から遠ざけているものです |
