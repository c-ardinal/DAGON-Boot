---
title: ポート
description: DAGON の拡張が使うすべてのポートです。
editUrl: false
---

:::note
`dagon_events::PORTS` から生成しています。直接編集せず、
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs` でもう一度生成してください。
:::

ポートは、プラグインが置き換えられる DAGON の部品です。置き換えは、一覧のレイヤーで、リンクされた
イメージの中だけに効きます。既定は DAGON 自身の実装です。プラグインのマニフェストは、そのポートがある
レイヤーでだけポートを提供でき、1 つのイメージで 1 つのポートを提供できるのは 1 つだけです。

信頼の区分:

- *untrusted*: どのプラグインでも使えます。レイヤーのサンドボックスの中で、マニフェストで宣言した能力で
  動き、DAGON は一覧のサービスを通してだけ呼びます。
- *untrusted, additive*: untrusted と同じで、足すだけです。DAGON 自身の実装は残ります。
- *trusted only*: 宣言して署名した、信頼する拡張だけが使えます。特権で動き、サービスは呼びません。

| ポート | 役割 | レイヤー | 既定 | 信頼 | サービス |
|---|---|---|---|---|---|
| `UpdateTransport` | リンクに出入りするバイト | boot, app, relay, host | boot, app, relay: USB CDC、UART、I2C ターゲット、PIO UART; host: シリアル | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` | メッセージ ↔ セッションのトリガー: 識別、ステータス、転送、切り替え、再起動、確定 | boot, app, relay, host | DAGON のリンク | untrusted (ガードは飛ばせません。ガードを実行するのはセッションの状態機械で、プロトコルではありません) | return, time, memory |
| `ImageVerifier` | ヘッダの署名、鍵の検索 | boot, app | プロジェクトの鍵での Ed25519 | trusted only | – |
| `Digest` | 本体のハッシュ | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` | 検証器とダイジェストが使う基本要素。ハードウェアの加速、HSM | boot, app, host | ソフトウェア | trusted only | – |
| `DecisionPolicy` | 試行の上限、どのスロットが勝つか | boot | 定義の `max_attempts`、確定より待機を先に | trusted only | – |
| `WatchdogDriver` | ブートと試行を守るウォッチドッグ | boot, app | マイコンのもの | trusted only | – |
| `SlotStore` | イメージの置き場所 | boot, app | マイコンのパッケージのフラッシュの配置 | trusted only | – |
| `LogStorage` | 状態ログの置き場所 | boot, app | マイコンのパッケージのフラッシュの配置 | trusted only | – |
| `Recovery` | 追加の復旧の経路: 転送のためのサービスの時間枠、ベンダーのダウンロードモード | boot, app | マイコンとボードの定義のとおりの BOOTSEL/UF2、factory、ROM の入り口 | untrusted, additive (プラグインは経路を足すだけで、DAGON のものを消せません) | return, transport, time, memory |
| `Diagnostics` | 追加の VERSION の TLV、追加のステータス、UDS の DID の表 | boot, app, relay | なし | untrusted | return, time, memory |
| `Display` | 段階やブートの結果の見え方 | boot, app, relay | boot: アクションプログラムを通した、ボードの定義の `[boot] led` と `halt`; app, relay: なし | untrusted (ブート時は、宣言されたピンとバスでサンドボックスの中で動きます) | return, pins, buses, time, memory |
| `LogSink` | `Recent` のあふれと拡張のログの行き先 | app, relay, host | なし | untrusted | return, events, time, memory |
| `SelfCheck` | 試行中のスロットを確定するときの健全さの答え | app | 健全 | untrusted (判断点、3) | return, time, memory |
| `BoardBringUp` | アプリの起動時のボードの電源、ピン、バス | app, relay | ボードのパッケージのもの | untrusted | return, pins, buses, time, memory |
| `ChildLink` | 親機が子機に届く方法 | relay | I2C コントローラ、PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` | イメージとリリースの署名 | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `KeyStore` | 署名の鍵の置き場所 | host | ファイルの鍵の組 | untrusted (ホストは定義上信頼されます。検証するのはデバイスです) | – |
| `BuildRunner` | ノードのイメージのビルド | host | cargo | untrusted | – |
| `Agent` | CLI とページがリクエストを渡す外部のエージェント | host | データで定義（エージェントの定義） | untrusted | – |
| `Action` | ホストのイベントの後で実行するプログラムやリクエスト | host | データで定義（アクションの定義） | untrusted | – |

## ポートではない部品

信頼する拡張でも、次のものは置き換えられません。

| 部品 | 内容 | レイヤー | ポートではない理由 |
|---|---|---|---|
| `ImageLifecycle` | スロットのストアと状態ログの上のイメージのライフサイクルの状態機械 | boot, app | 永続のイメージの状態の唯一の書き手。どの変更もイベントを出し、確定のルールを保ちます |
| `SessionGuards` | 書き込みのセッションのガード: ターゲット、スロット、ヘッダの署名と互換性、本体のダイジェスト | boot, app | どのプロトコルのトリガーも同じガードを通ります。置き換えると、DAGON が拒むものを転送が書けてしまいます |
| `BootRecord` | アプリがリプレイするブートのイベントの保持の記録 | boot, app | その形式は DAGON の保持の取り決めで、ブートや確定の判断の入力にはなりません |
| `Composition` | レイヤーのチェーンの順（公開元、ボードのパッケージ、ライブラリ、利用者）と判断の上限 | boot, app, relay, host | すべての拡張がすべてのイベントを見られるようにし、答えを有界に保つものです |
| `Sandbox` | 入り口のゲート、サービス呼び出し、保護マップ、ロックアップヒューズ | boot, app, relay | 拡張を DAGON の状態から遠ざけているものです |
