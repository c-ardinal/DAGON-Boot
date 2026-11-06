---
title: "既にあるファームウェアに DAGON を組み込む"
description: "手元のファームウェアに DAGON のエージェントと書き込みを足します。"
---

`dagon new` のプロジェクトではないファームウェアの手順です。

1. crate を、リリースのタグに固定した git 依存として加えます。例: `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port` も同じように加えます。ブートローダを作るなら `dagon-core` も加えます。
2. マイコンの定義とボードの定義を用意します。

   - 対応するマイコン: マイコンの定義（`targets/mcu/<mcu>`。フラッシュの配置、リンカの割り付け、ブート用のイメージ、ウォッチドッグ、リンク）を使います。ボードはボードの定義（`targets/boards/<board id>`）を使うか、自分のボード用にマイコンの定義の `BoardDef` を実装します。マイコンの定義の README に、約 20 行の完全なアプリがあります。
   - ほかのマイコン: 同じ形にフラッシュを割り付けます。ブートローダ、状態ログ 2 面、スロット A と B です。各スロットの先頭に 512 バイトの署名付きイメージヘッダを置き、本体はスロット先頭 + 0x200 に置きます。

3. ボード用に `dagon-port` の trait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS` か `BootSelectV`）と `dagon_agent::SlotStore` を実装します。
4. アプリ層のチェーンを組み立て、リンクを渡してエージェントを起動します。拡張が判断点に答え、イベントを受け取ります（[イベント](/reference/events/)）。

   ```rust
   let chain = dagon_events::compose(Layer::App, events, board_exts, user_exts, bounds);
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, chain);
   agent.run(&mut link, &mut reset).await;                    // 要求に応え続ける
   ```

5. ウォッチドッグをキックします。確定済みのあいだはキックし続け、試行中は限られた時間だけキックします。こうすると、確定されないイメージはリセットされて前のバージョンに戻ります。
6. `dagon conform` で移植を確かめます。
