---
title: "Adding DAGON to existing firmware"
description: "Give a firmware you already have DAGON's agent and writes."
---

These steps are for firmware that is not a `dagon new` project.

1. Add the crates as git dependencies pinned to a release tag, for example `dagon-agent = { git = "<repository URL>", tag = "<release tag>" }`. Do the same for `dagon-format`, `dagon-proto` and `dagon-port`. For a boot loader, add `dagon-core` too.
2. Prepare the MCU and board definitions.

   - On a supported MCU: use the MCU definition (`targets/mcu/<mcu>`: flash layout, linker map, boot images, watchdog, links). For the board, use a board definition (`targets/boards/<board id>`), or implement `BoardCore` (`targets/mcu/common`: board id, flash size, bring-up, extension) and the MCU definition's `BoardDef` (the peripheral split) for your own board. The MCU definition READMEs show a complete app in about 20 lines.
   - On another MCU: lay out the flash the same way: a boot loader, two state log faces, and the slots of each image kind (A and B, or A alone for a kind another image writes: `[[slot]]` and `[writers]` in [MCU and board definitions](/en/reference/definitions/)). Each slot starts with the 512-byte signed image header, and the body is at slot start + 0x200.

3. Implement the `dagon-port` traits for your board (`Flash`, `Link`, `Reset`, `Watchdog`, and `BootSelectS` or `BootSelectV`) and a `dagon_agent::SlotStore`.
4. Compose the app layer's chain with `dagon_events::compose`, and create the agent by passing a `dagon_agent::Config`, a signature verifier (a `dagon_format::SigVerifier`, for example `dagon_ed25519::Ed25519Compact::new(PUBKEY)`) and your `SlotStore` to `dagon_agent::Agent::new`. Your extensions answer the decision points and observe the events (see [Events](/en/reference/events/)).
5. Write the loop that reads a request frame from your link (`dagon_port::Link`), writes the answer of `Agent::handle`, and resets when asked to. Below is the loop the MCU definitions use (`targets/mcu/common/src/serve.rs`), as it is. `host` is what differs between MCUs (clock, watchdog reports, logging, waiting). When the answer could not be written (`wrote` is `Err`), the loop does not reset and waits for the next request; it resets only after the reply to a reboot was written and has left (`flush`). When it returns `Exit::Reboot`, the caller resets.

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
       // Byte 5 of a frame is the command.
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
               // Not a valid request, or not for this node: no answer, but
               // the link must take the next request.
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
       // Reset only once the reply has left; a link that cannot tell (a
       // new request came first) keeps serving instead.
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

6. Feed the watchdog only through liveness. Register each task's `Level` with a `dagon_mcu_common::guard::Guard`; each task reports `busy` when it starts a piece of work and `idle` when it waits for the next one. `Guard::feed` feeds only while every task is within the deadline of its current work, so a stuck task stops the feeding and the chip resets. On trial, the `Trial` policy decides (`FeedUntilMs`: feed for a while after boot; `NeverFeed`: never feed). An image that is never confirmed is reset and, with two slots, goes back to the previous version.
7. Choose who confirms with `Confirm`. With the default `Confirm::ByHost`, the host sends CONFIRM and the node confirms when its `self_check` decision answers healthy. With `Confirm::Own`, the start runs `self_check` and confirms by itself after the board's bring-up and before the link serves; this is for a node used without a host, and the host's CONFIRM still works. When you use the agent directly, `Agent::confirm_own` confirms by itself.
8. Check the port with `dagon conform`.
