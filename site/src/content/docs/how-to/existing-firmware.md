---
title: "既にあるファームウェアにDAGONを組み込む"
description: "手元のファームウェアにDAGONのエージェントと書き込みを足します。"
---

`dagon new`のプロジェクトではないファームウェアの手順です。

1. crateを、リリースのタグに固定したgit依存として加えます。例: `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port`も同じように加えます。ブートローダを作るなら`dagon-core`も加えます。
2. マイコンの定義とボードの定義を用意します。

   - 対応するマイコン: マイコンの定義（`targets/mcu/<mcu>`。フラッシュの配置、リンカの割り付け、ブート用のイメージ、ウォッチドッグ、リンク）を使います。ボードはボードの定義（`targets/boards/<board id>`）を使うか、自分のボード用にマイコンの定義の`BoardDef`を実装します。マイコンの定義のREADMEに、約20行の完全なアプリがあります。
   - ほかのマイコン: 同じ形にフラッシュを割り付けます。ブートローダ、状態ログ2面、スロットAとBです。各スロットの先頭に512バイトの署名付きイメージヘッダを置き、本体はスロット先頭 + 0x200に置きます。

3. ボード用に`dagon-port`のtrait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS`か`BootSelectV`）と`dagon_agent::SlotStore`を実装します。
4. アプリ層のチェーンを`dagon_events::compose`で組み立て、`dagon_agent::Config`と`SlotStore`の実装を`dagon_agent::Agent::new`に渡して、エージェントを作ります。拡張が判断点に答え、イベントを受け取ります（[イベント](/reference/events/)）。
5. リンク（`dagon_port::Link`）から要求のフレームを読み、`Agent::handle`の答えを書き、求められたリセットをする繰り返しを書きます。次は、マイコンの定義が使っている繰り返し（`targets/mcu/common/src/serve.rs`）の抜き出しです。`host`はマイコンごとに違う部分（時計、記録、待ち）です。`Exit::Reboot`で戻ったら、呼んだ側がリセットします。

   ```rust
   let (via, addr) = (link.kind().code(), link.addr());
   loop {
       let read = link.read_frame().await;
       let request = match read {
           Ok(r) => r,
           Err(LinkError::Timeout) => {
               // 要求のないままIDLE_TIMEOUT_MSがたったら、agent.abort()で転送を捨てる（省略）
               continue;
           }
           Err(LinkError::Closed) => {
               return Exit::Closed;
           }
           Err(e) => {
               host.on_error(e);
               continue;
           }
       };
       agent.request(request, via);
       let mut reboot = None;
       // 適合試験のビルドが先に答える部分（host.intercept）は省略
       match agent.handle(request) {
           Some(reply) => {
               reboot = reply.reboot.then_some(reply.reboot_delay_ms);
               link.write_frame(reply.frame).await
           }
           None => link.no_reply().await,
       };
       // 答えを書けなかったときは、次の要求を待つ（省略）
       if let Some(delay_ms) = reboot {
           if link.flush().await.is_ok() {
               host.wait_ms(delay_ms).await;
               agent.before_reboot();
               return Exit::Reboot;
           }
       }
   }
   ```

6. ウォッチドッグをキックします。確定済みのあいだはキックし続け、試行中は限られた時間だけキックします。こうすると、確定されないイメージはリセットされて前のバージョンに戻ります。
7. `dagon conform`で移植を確かめます。
