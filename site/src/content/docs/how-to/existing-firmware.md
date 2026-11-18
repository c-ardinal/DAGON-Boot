---
title: "既存のファームウェアへのDAGONの組込"
description: "利用者のファームウェアにDAGONのエージェントと書込の機能を追加します。"
---

`dagon new`で作成したプロジェクト以外のファームウェア用の手順です。

1. crateを、リリースのタグに固定したgit依存として追加します。例: `dagon-agent = { git = "<リポジトリの URL>", tag = "<リリースのタグ>" }`。`dagon-format`、`dagon-proto`、`dagon-port`も同様に追加します。ブートローダを作成する場合は`dagon-core`も追加します。
2. マイコンの定義とボードの定義を用意します。

   - 対応済のマイコン: マイコンの定義（`targets/mcu/<mcu>`。フラッシュの配置、リンカの配置指定、ブート用のイメージ、ウォッチドッグ、リンク）を使用します。ボードはボードの定義（`targets/boards/<board id>`）を使用します。または独自のボード用に`BoardCore`（`targets/mcu/common`。ボードID、フラッシュ容量、起動処理、拡張）とマイコンの定義の`BoardDef`（周辺機能の分担）を実装します。マイコンの定義のREADMEに、約20行の完全なアプリがあります。
   - 他のマイコン: 同一の形式でフラッシュを配置します。配置対象はブートローダ、状態ログ2面、イメージの種類ごとのスロット（AとB、または別の種類のイメージが書込する種類ではAだけ。[マイコンとボードの定義](/reference/definitions/)の`[[slot]]`と`[writers]`）です。各スロットの先頭に512バイトの署名付きイメージヘッダを配置し、本体はスロット先頭 + 0x200に配置します。

3. ボード用に`dagon-port`のtrait（`Flash`、`Link`、`Reset`、`Watchdog`、`BootSelectS`か`BootSelectV`）と`dagon_agent::SlotStore`を実装します。
4. アプリ層のチェーンを`dagon_events::compose`で構築します。`dagon_agent::Config`、署名の検証器（`dagon_format::SigVerifier`の実装。例: `dagon_ed25519::Ed25519Compact::new(PUBKEY)`）、`SlotStore`の実装を`dagon_agent::Agent::new`に指定して、エージェントを作成します。拡張が判断点に応答し、イベントを受信します（[イベント](/reference/events/)）。
5. ループを記述します。ループはリンク（`dagon_port::Link`）から要求のフレームを受信し、`Agent::handle`の応答を送信し、要求されたリセットを実行します。以下はマイコンの定義が使用するループ（`targets/mcu/common/src/serve.rs`）と同一です。`host`はマイコンごとに異なる部分（クロック、ウォッチドッグへの報告、記録、待機）です。応答の送信に失敗した場合（`wrote`が`Err`）は、リセットせずに後続の要求を待機します。リセットの条件はリブートへの応答の送信と送信完了（`flush`）の両方の成功だけです。`Exit::Reboot`で復帰した場合は、呼出元がリセットします。

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
               // 不正な要求か、このノード宛以外の要求: 応答はしないが、
               // リンクは後続の要求を受信可能な状態にする
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
       // 応答の送信完了時だけリセットする。送信完了が不明なリンク
       // （後続の要求が先着した場合）では、リセットせずに応答を継続する
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

6. ウォッチドッグは生存監視（liveness）経由でだけキックします。`targets/mcu/common`の`guard::Guard`に各タスクの`Level`を登録し、各タスクは作業開始時に`busy`、待機開始時に`idle`を報告します。`Guard::feed`は全タスクが作業期限内の場合だけキックします。停止したタスクが存在すると、キックが停止してチップはリセットします。試行中は`Trial`の方針（`FeedUntilMs`は起動後の一定時間だけキック、`NeverFeed`はキック無し）に従います。未確定のイメージはリセットされ、スロットが2つの場合は以前のバージョンに復帰します。
7. 確定の方式を`Confirm`で選択します。既定の`Confirm::ByHost`ではホストがCONFIRMを送信し、ノードは`self_check`の判断結果が正常の場合に確定します。`Confirm::Own`では起動処理がボード初期化の後、リンク応答の開始前に`self_check`を実行して自己確定します。ホスト不在のノード用で、ホストのCONFIRMも有効です。エージェントを直接使用する場合、自己確定は`Agent::confirm_own`です。
8. `dagon conform`で移植を確認します。
