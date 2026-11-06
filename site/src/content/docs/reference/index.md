---
title: リファレンス
description: DAGON自身のソースから作るページです。どのページも元のソースを書いています。
sidebar:
  order: 0
---

ここのページは、DAGON自身が読むソースから作っています。
ページと元のソースが食い違うと、リポジトリの検査が失敗します。
コマンドの説明とrustdocの本文は、元のソースのままで英語です。

- [コマンド（dagon）](/reference/cli/): `dagon`のすべてのコマンド（clapの定義から）
- [Serve HTTP API](/reference/serve-api/): `dagon serve`のすべてのルート
- [イベント](/reference/events/)と[ポート](/reference/ports/): 拡張が使うすべてのイベントとポート
- [エージェント定義の形式](/reference/agent-definition/): DAGONが読むエージェントのファイル
- [ボードとマイコン](/reference/boards/): すべてのボードとマイコンの定義
- [取り決めと部品](/reference/contracts/): `compat.toml`にある、すべての形式番号と部品のバージョン
- [マイコンごとのツール](/reference/tools/): `dagon doctor`がマイコンごとに確かめるツール
- [Rust API](/reference/rust-api/): ワークスペースのcrateとそのrustdoc

手で書いたページです。中の一覧は生成するか、テストで確かめています。

- [エージェントとのつなぎ方: serve API](/reference/agent-api/): DAGONのエージェントとのつなぎ方、質問、キー
- [拡張のマニフェスト](/reference/extension-manifest/)と[マイコンとボードの定義](/reference/definitions/): キーの表は、DAGONがファイルを読み込む型から生成しています
- [ライセンス](/reference/licenses/)
