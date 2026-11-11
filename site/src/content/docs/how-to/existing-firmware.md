---
title: "既にあるファームウェアにDAGONを組み込む"
description: "手元のファームウェアにDAGONのエージェントと書き込みを足します。"
---

`dagon new`のプロジェクトではないファームウェアの手順です。

1. crateを、リリースのタグに固定したgit依存として加えます。例: `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port`も同じように加えます。ブートローダを作るなら`dagon-core`も加えます。
2. マイコンの定義とボードの定義を用意します。

   - 対応するマイコン: マイコンの定義（`targets/mcu/<mcu>`。フラッシュの配置、リンカの割り付け、ブート用のイメージ、ウォッチドッグ、リンク）を使います。ボードはボードの定義（`targets/boards/<board id>`）を使うか、自分のボード用に`BoardCore`（`targets/mcu/common`。ボードID、フラッシュ容量、立ち上げ、拡張）とマイコンの定義の`BoardDef`（周辺機能の分け方）を実装します。マイコンの定義のREADMEに、約20行の完全なアプリがあります。
   - ほかのマイコン: 同じ形にフラッシュを割り付けます。ブートローダ、状態ログ2面、スロットAとBです。各スロットの先頭に512バイトの署名付きイメージヘッダを置き、本体はスロット先頭 + 0x200に置きます。

3. ボード用に`dagon-port`のtrait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS`か`BootSelectV`）と`dagon_agent::SlotStore`を実装します。
4. アプリ層のチェーンを`dagon_events::compose`で組み立て、`dagon_agent::Config`と`SlotStore`の実装を`dagon_agent::Agent::new`に渡して、エージェントを作ります。拡張が判断点に答え、イベントを受け取ります（[イベント](/reference/events/)）。
5. リンク（`dagon_port::Link`）から要求のフレームを読み、`Agent::handle`の答えを書き、求められたリセットをする繰り返しを書きます。次は、マイコンの定義が使っている繰り返し（`targets/mcu/common/src/serve.rs`）そのものです。`host`はマイコンごとに違う部分（時計、ウォッチドッグへの報告、記録、待ち）です。答えを書けなかったとき（`wrote`が`Err`）は、リセットせずに次の要求を待ちます。リセットするのは、リブートへの答えを書けて、送り終えた（`flush`）ときだけです。`Exit::Reboot`で戻ったら、呼んだ側がリセットします。

   ```rust
   let (via, addr) = (link.kind().code(), link.addr());
   let mut up = false;
   let mut last_request: Option<u64> = None;
   loop {
       host.idle();
       let read = link.read_frame().await;
       host.busy();
       let request = match read {
           Ok(r) => r,
           Err(LinkError::Timeout) => {
               let idle = IDLE_TIMEOUT_MS as u64;
               if last_request.is_some_and(|t| host.now_ms().saturating_sub(t) >= idle) {
                   agent.abort();
                   last_request = None;
               }
               continue;
           }
           Err(LinkError::Closed) => {
               if up {
                   agent.link_down(via, addr);
               }
               return Exit::Closed;
           }
           Err(e) => {
               host.on_error(e);
               continue;
           }
       };
       host.feed();
       if !up {
           up = true;
           agent.link_up(via, addr);
       }
       let arrived = host.now_ms();
       last_request = Some(arrived);
       // フレームの5バイト目がコマンド
       let cmd = request.get(5).copied().unwrap_or(0);
       agent.request(request, via);
       host.on_request();
       let mut reboot = None;
       let mut status = None;
       let wrote = match host.intercept(agent, request) {
           Some(answer) => {
               status = reply_status(answer);
               link.write_frame(answer).await
           }
           None => match agent.handle(request) {
               Some(reply) => {
                   reboot = reply.reboot.then_some(reply.reboot_delay_ms);
                   status = reply_status(reply.frame);
                   link.write_frame(reply.frame).await
               }
               // 正しい要求でないか、このノード宛てでない: 答えないが、
               // リンクは次の要求を受けられるようにする
               None => link.no_reply().await,
           },
       };
       host.after(agent, cmd);
       match (wrote, status) {
           (Err(e), _) => {
               host.on_error(e);
               up = false;
               agent.link_down(via, addr);
               continue;
           }
           (Ok(()), Some(s)) => {
               let ms = host.now_ms().saturating_sub(arrived);
               agent.replied(s, u32::try_from(ms).unwrap_or(u32::MAX));
           }
           (Ok(()), None) => {}
       }
       // 答えが出ていったときだけリセットする。それが分からないリンク
       // （先に次の要求が来た）では、リセットせずに応え続ける
       if let Some(delay_ms) = reboot {
           if link.flush().await.is_ok() {
               host.idle();
               host.wait_ms(delay_ms).await;
               agent.before_reboot();
               return Exit::Reboot;
           }
       }
   }
   ```

6. ウォッチドッグをキックします。確定済みのあいだはキックし続け、試行中は限られた時間だけキックします。こうすると、確定されないイメージはリセットされて前のバージョンに戻ります。
7. `dagon conform`で移植を確かめます。
