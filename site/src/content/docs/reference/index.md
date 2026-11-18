---
title: リファレンス
description: DAGON自身のソースから生成するページです。各ページに生成元のソースを記載しています。
sidebar:
  order: 0
---

このセクションのページは、DAGON自身が読込するソースから生成しています。
ページと生成元のソースが不一致の場合は、リポジトリの検査が失敗します。
コマンドの説明とrustdocの本文は、生成元のソースのままで英語です。

- [コマンド（dagon）](/reference/cli/): `dagon`の全コマンド（clapの定義から生成）
- [Serve HTTP API](/reference/serve-api/): `dagon serve`の全ルート
- [イベント](/reference/events/)と[ポート](/reference/ports/): 拡張が使用する全イベントと全ポート
- [エージェント定義の形式](/reference/agent-definition/): DAGONが読込するエージェントのファイル
- [ボードとマイコン](/reference/boards/): 全ボードと全マイコンの定義
- [規約と部品](/reference/contracts/): `compat.toml`に記載した全形式番号と部品のバージョン
- [マイコンごとのツール](/reference/tools/): `dagon doctor`がマイコンごとに確認するツール
- [Rust API](/reference/rust-api/): ワークスペースのcrateとそのrustdoc

以下は手動で記述したページです。ページ内の一覧は生成するかテストで検証しています。

- [エージェントとの接続方法: serve API](/reference/agent-api/): DAGONとエージェントの接続方法、質問、キー
- [拡張のマニフェスト](/reference/extension-manifest/)と[マイコンとボードの定義](/reference/definitions/): キーの表はDAGONがファイルの読込に使用する型から生成しています
- [構成のリンク](/reference/topology/): 2つの端でノード間を接続するリンク。キーの表はDAGONが構成の読込に使用する型から生成しています
- [ライセンス](/reference/licenses/)
