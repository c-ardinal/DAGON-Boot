---
title: Rust API
description: ワークスペースのcrateとrustdocです。
editUrl: false
---

:::note
`Cargo.toml`から生成しています。直接編集せず、`python site/scripts/gen.py`で再度生成してください。
rustdocの本文はソースの原文のままで、英語です。
:::

ワークスペースの全crateのRust APIの文書（rustdoc）を、このサイトと同時に公開しています。
以下の名称を選択すると、そのcrateの文書をオープンします。

| Crate | 内容 |
|---|---|
| [`dagon-agent`](/api/dagon_agent/) | DAGONのエージェント: ノードの書込のセッションで、アプリケーションでもブートローダでも同一です。HELLO/STATUS/VERSIONに応答し、書込側が動作していないスロットにイメージを受信し、組込の検証器で検証し、ブート対象を切替して確定します。 |
| [`dagon-boot`](/api/dagon_boot/) | 状態機械としてのブートローダの手順: ウォッチドッグの有効化からジャンプか停止までの各ステップが、ブートローダの拡張が観測するイベントです |
| [`dagon-catalog`](/api/dagon_catalog/) | DAGONの全状態機械のカタログを1つに集約した登録簿と、文書のイベントとポートの生成ページ |
| [`dagon-cli`](/api/dagon/) | DAGON Bootのコマンドラインツール |
| [`dagon-conform`](/api/dagon_conform/) | DAGONの適合試験: ポートの合格が必要な、正常経路の12の検査。 |
| [`dagon-core`](/api/dagon_core/) | ポートの規約上で動作する、ボード非依存のDAGONのロジック: 状態ログ（集約、読込、追記、コンパクション）、kind Sのブート判断（検証、試行、ロールバック）、stage0のあるマイコンのブートローダのスロット規則、ビルド時のキーの補助。 |
| [`dagon-ed25519`](/api/dagon_ed25519/) | DAGONの署名付き構造を検証するEd25519（RFC 8032）の署名検証。`SigVerifier`の実装です。検証だけを実行し、署名せず、秘密の値も保持しません。 |
| [`dagon-events`](/api/dagon_events/) | DAGONのライフサイクルイベント: 遷移がイベントを発行する状態機械、拡張、ポート、公開するイベントの状態 |
| [`dagon-extension`](/api/dagon_extension/) | DAGONの拡張のマニフェスト（dagon-extension.toml）、ポートとイベントのカタログとノードの定義との照合による検証、保護の種類ごとのマップの生成器、それらからdagon genが出力するファイル |
| [`dagon-format`](/api/dagon_format/) | DAGONの全部品が共有するバイトのレイアウト: イメージのヘッダ、マニフェスト、リリースのバンドル（`.dgb`）、状態ログ、構成の正規のバイナリ、さらにCRC-32、SHA-256、署名検証のインターフェース（`SigVerifier`）。複数バイトの欄は全部リトルエンディアンです。 |
| [`dagon-host`](/api/dagon_host/) | CLIとブラウザが共有するホスト側の手順: 検出、状態、適用、ロールバック、確定、復旧。 |
| [`dagon-port`](/api/dagon_port/) | ポートの規約: ボードがDAGONに提供するもの。 |
| [`dagon-proto`](/api/dagon_proto/) | DAGONのリンクv2: フレームの形式（USB CDC、UART、I2Cで共通）、コマンドの表、ステータスコード、全ペイロードの欄の文法とその唯一の読取側と書込側、型付きのペイロード、I2Cのメールボックスの状態。 |
| [`dagon-relay`](/api/dagon_relay/) | リレー: ノードが、リンクで接続した他のノードに到達する機構。どのMCUにも依存しません。 |
| [`dagon-retained-model`](/api/dagon_retained_model/) | 障害注入下での、DAGONの保持と永続の形式の実行可能なモデル |
| [`dagon-romlink`](/api/dagon_romlink/) | I2CのSTM32システムメモリのブートローダ（ST AN4221）。ホストから、コーディネータのNRST/BOOT0の信号線と未加工のI2Cの通信で駆動します。 |
| [`dagon-sign`](/api/dagon_sign/) | DAGONの署名: キー、署名付きのイメージ、リリースのバンドル、初回インストールのフラッシュの内容。 |
| [`dagon-sim`](/api/dagon_sim/) | DAGONのシミュレータ: ハードウェアの決定的な代役です。ホストのツール、ブラウザのUI、テストは、ボードなしでも対向側を確保します。 |
| [`dagon-topology`](/api/dagon_topology/) | 構成のYAML: スキーマ、検証の規則、対応ボードの表。CLIとブラウザ向けのビルドが共有し、同一の規則を使用します。 |
| [`dagon-wasm`](/api/dagon_wasm/) | 共有するRustの規則のブラウザ向けビルド: マイコンとボードの定義、構成の確認とコンパイル、リリースの検証、HELLOのペイロードとバージョンの文字列。公開する関数はいずれも薄いラッパーで、ページとCLIが同一のコードを使用します。 |
