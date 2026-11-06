---
title: Rust API
description: ワークスペースのcrateとrustdocです。
editUrl: false
---

:::note
`Cargo.toml`から生成しています。直接編集せず、`python site/scripts/gen.py`でもう一度生成してください。
rustdocの本文は元のソースのままで、英語です。
:::

ワークスペースのすべてのcrateのRust APIの文書（rustdoc）を、このサイトと一緒に公開しています。
下の名前を選ぶと、そのcrateを開きます。

| Crate | 中身 |
|---|---|
| [`dagon-agent`](/api/dagon_agent/) | DAGONのエージェント: アプリケーションに組み込まれ、HELLO/STATUS/VERSIONに答え、イメージを使っていないスロットに受け取り、ブートの対象を切り替えて確定します。 |
| [`dagon-boot`](/api/dagon_boot/) | 状態機械としてのブートローダの手順: 有効にしたウォッチドッグからジャンプか停止までの各ステップが、ブートローダの拡張が観測するイベントです |
| [`dagon-catalog`](/api/dagon_catalog/) | DAGONのすべての状態機械のカタログを1つにまとめた登録簿と、文書のイベントとポートの生成ページ |
| [`dagon-cli`](/api/dagon/) | DAGON Bootのコマンドラインツール |
| [`dagon-conform`](/api/dagon_conform/) | DAGONの適合試験: ポートが通らなければならない、正常な経路の12の検査。 |
| [`dagon-core`](/api/dagon_core/) | ポートの取り決めの上で動く、ボードに依存しないDAGONのロジック: 状態ログ（畳み込み、読み込み、追記、コンパクション）、kind Sのブートの判断（検証、試行、ロールバック）、stage0のあるマイコンのブートローダのA/Bのルール、ビルド時の鍵の補助。 |
| [`dagon-events`](/api/dagon_events/) | DAGONのライフサイクルイベント: 遷移がイベントを出す状態機械、拡張、ポート、公開するイベントの状態 |
| [`dagon-extension`](/api/dagon_extension/) | DAGONの拡張のマニフェスト（dagon-extension.toml）、ポートとイベントのカタログとノードの定義に照らした検証、保護の種類ごとのマップの生成器、そこからdagon genが書き出すファイル |
| [`dagon-format`](/api/dagon_format/) | DAGONのすべての部品が共有するバイトのレイアウト: イメージのヘッダ、マニフェスト、リリースのバンドル（`.dgb`）、状態ログ、構成の正規のバイナリ、そしてCRC-32、SHA-256、Ed25519の検証。複数バイトの欄はすべてリトルエンディアンです。 |
| [`dagon-host`](/api/dagon_host/) | CLIとブラウザが共有するホスト側の手順: 検出、状態、構成の送信、適用、ロールバック、確定、復旧。 |
| [`dagon-port`](/api/dagon_port/) | ポートの取り決め: ボードがDAGONに用意するもの。 |
| [`dagon-proto`](/api/dagon_proto/) | DAGONのリンクv1: フレームの形式（USB CDC、UART、I2Cで共通）、コマンドの集合、ステータスコード、リクエストとレスポンスのペイロード、I2Cのメールボックスの状態。 |
| [`dagon-retained-model`](/api/dagon_retained_model/) | 障害注入の下での、DAGONの保持と永続の形式の実行可能なモデル |
| [`dagon-romlink`](/api/dagon_romlink/) | I2CのSTM32システムメモリのブートローダ（ST AN4221）。ホストから、親機のNRST/BOOT0の線と生のI2Cのやり取りで動かします。 |
| [`dagon-sign`](/api/dagon_sign/) | DAGONの署名: 鍵、署名付きのイメージ、リリースのバンドル、最初のインストールのフラッシュの中身。 |
| [`dagon-sim`](/api/dagon_sim/) | DAGONのシミュレータ: ハードウェアの決定的な代役です。ホストのツール、ブラウザのUI、テストは、ボードがなくても相手を得られます。 |
| [`dagon-topology`](/api/dagon_topology/) | 構成のYAML: スキーマ、検証のルール、対応ボードの表。CLIとブラウザ向けのビルドが共有し、同じルールを使います。 |
| [`dagon-wasm`](/api/dagon_wasm/) | 共有するRustのルールのブラウザ向けビルド: マイコンとボードの定義、構成の確認とコンパイル、リリースの検証、HELLOのペイロードとバージョンの文字列。公開する関数はどれも薄いラッパーで、ページとCLIが同じコードを使います。 |
