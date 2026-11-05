---
title: "既にあるファームウェアに DAGON を組み込む"
description: "手元のファームウェアに DAGON のエージェントと書き込みを足します。"
---

`dagon new` のプロジェクトではないファームウェアの場合:

1. crate をリリースのタグに固定した git 依存として加えます。例: `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port`、ブートコアを作るなら `dagon-core` も同様です。
2. 対応するマイコンなら、MCU のパッケージ（`targets/mcu/<mcu>`。フラッシュの配置、リンカの割り付け、ブート用のイメージ、見守り、リンク）とボードのパッケージ（`targets/boards/<board id>`）を使うか、自分のボード用に MCU のパッケージの `BoardDef` を実装します。MCU のパッケージの README に、約 20 行の完全なアプリがあります。ほかのマイコンでは同じ形にフラッシュを割り付けます: ブートコア、状態ログ 2 面、スロット A と B。各スロットの先頭に 512 バイトの署名付きイメージヘッダ、本体はスロット先頭 + 0x200。
3. ボード用に `dagon-port` の trait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS` か `BootSelectV`）と `dagon_agent::SlotStore` を実装し、アプリ層のチェーンを組み立て（拡張が判断点に答え、イベントを受け取ります。[Events](/ja/reference/events/) を参照）、リンクを渡してエージェントを起動します:

   ```rust
   let chain = dagon_events::compose(Layer::App, events, board_exts, user_exts, bounds);
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, chain);
   agent.run(&mut link, &mut reset).await;                    // 要求に応え続ける
   ```

   確定済みのあいだはウォッチドッグに給餌し、試行中は限られた時間だけ給餌して、確定されないイメージがリセットされ巻き戻るようにします。
4. `dagon conform` で移植を確かめます。
