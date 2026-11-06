---
title: Rust API
description: ワークスペースの crate と rustdoc です。
editUrl: false
---

:::note
`Cargo.toml` から生成しています。直接編集せず、`python site/scripts/gen.py` でもう一度生成してください。
rustdoc の本文は元のソースのままで、英語です。
:::

ワークスペースのすべての crate の Rust API の文書（rustdoc）を、このサイトと一緒に公開しています。
下の名前を選ぶと、その crate を開きます。

| Crate | 中身 |
|---|---|
| [`dagon-agent`](/api/dagon_agent/) | DAGON のエージェント: アプリケーションに組み込まれ、HELLO/STATUS/VERSION に答え、イメージを使っていないスロットに受け取り、ブートの対象を切り替えて確定します。 |
| [`dagon-boot`](/api/dagon_boot/) | 状態機械としてのブートローダの手順: 有効にしたウォッチドッグからジャンプか停止までの各ステップが、ブートローダの拡張が観測するイベントです |
| [`dagon-catalog`](/api/dagon_catalog/) | DAGON のすべての状態機械のカタログを 1 つにまとめた登録簿と、文書のイベントとポートの生成ページ |
| [`dagon-cli`](/api/dagon/) | DAGON Boot のコマンドラインツール |
| [`dagon-conform`](/api/dagon_conform/) | DAGON の適合試験: ポートが通らなければならない、正常な経路の 12 の検査。 |
| [`dagon-core`](/api/dagon_core/) | ポートの取り決めの上で動く、ボードに依存しない DAGON のロジック: 状態ログ（畳み込み、読み込み、追記、コンパクション）、kind S のブートの判断（検証、試行、ロールバック）、stage0 のあるマイコンのブートローダの A/B のルール、ビルド時の鍵の補助。 |
| [`dagon-events`](/api/dagon_events/) | DAGON のライフサイクルイベント: 遷移がイベントを出す状態機械、拡張、ポート、公開するイベントの状態 |
| [`dagon-extension`](/api/dagon_extension/) | DAGON の拡張のマニフェスト（dagon-extension.toml）、ポートとイベントのカタログとノードの定義に照らした検証、保護の種類ごとのマップの生成器、そこから dagon gen が書き出すファイル |
| [`dagon-format`](/api/dagon_format/) | DAGON のすべての部品が共有するバイトのレイアウト: イメージのヘッダ、マニフェスト、リリースのバンドル（`.dgb`）、状態ログ、構成の正規のバイナリ、そして CRC-32、SHA-256、Ed25519 の検証。複数バイトの欄はすべてリトルエンディアンです。 |
| [`dagon-host`](/api/dagon_host/) | CLI とブラウザが共有するホスト側の手順: 検出、状態、構成の送信、適用、ロールバック、確定、復旧。 |
| [`dagon-port`](/api/dagon_port/) | ポートの取り決め: ボードが DAGON に用意するもの。 |
| [`dagon-proto`](/api/dagon_proto/) | DAGON のリンク v1: フレームの形式（USB CDC、UART、I2C で共通）、コマンドの集合、ステータスコード、リクエストとレスポンスのペイロード、I2C のメールボックスの状態。 |
| [`dagon-retained-model`](/api/dagon_retained_model/) | 障害注入の下での、DAGON の保持と永続の形式の実行可能なモデル |
| [`dagon-romlink`](/api/dagon_romlink/) | I2C の STM32 システムメモリのブートローダ（ST AN4221）。ホストから、親機の NRST/BOOT0 の線と生の I2C のやり取りで動かします。 |
| [`dagon-sign`](/api/dagon_sign/) | DAGON の署名: 鍵、署名付きのイメージ、リリースのバンドル、最初のインストールのフラッシュの中身。 |
| [`dagon-sim`](/api/dagon_sim/) | DAGON のシミュレータ: ハードウェアの決定的な代役です。ホストのツール、ブラウザの UI、テストが、ボードなしで相手を持てます。 |
| [`dagon-topology`](/api/dagon_topology/) | 構成の YAML: スキーマ、検証のルール、対応ボードの表。CLI とブラウザ向けのビルドが共有し、同じルールを使います。 |
| [`dagon-wasm`](/api/dagon_wasm/) | 共有する Rust のルールのブラウザ向けビルド: マイコンとボードの定義、構成の確認とコンパイル、リリースの検証、HELLO のペイロードとバージョンの文字列。公開する関数はどれも薄いラッパーで、ページと CLI が同じコードを使います。 |
