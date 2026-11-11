---
title: "Adding DAGON to existing firmware"
description: "Give a firmware you already have DAGON's agent and writes."
---

These steps are for firmware that is not a `dagon new` project.

1. Add the crates as git dependencies pinned to a release tag, for example `dagon-agent = { git = "<repository URL>", tag = "<release tag>" }`. Do the same for `dagon-format`, `dagon-proto` and `dagon-port`. For a boot loader, add `dagon-core` too.
2. Prepare the MCU and board definitions.

   - On a supported MCU: use the MCU definition (`targets/mcu/<mcu>`: flash layout, linker map, boot images, watchdog, links). For the board, use a board definition (`targets/boards/<board id>`), or implement the MCU definition's `BoardDef` for your own board. The MCU definition READMEs show a complete app in about 20 lines.
   - On another MCU: lay out the flash the same way: a boot loader, two state log faces, and slots A and B. Each slot starts with the 512-byte signed image header, and the body is at slot start + 0x200.

3. Implement the `dagon-port` traits for your board (`Flash`, `Link`, `Reset`, `Watchdog`, and `BootSelectS` or `BootSelectV`) and a `dagon_agent::SlotStore`.
4. Compose the app layer's chain with `dagon_events::compose`, and create the agent by passing a `dagon_agent::Config` and your `SlotStore` to `dagon_agent::Agent::new`. Your extensions answer the decision points and observe the events (see [Events](/en/reference/events/)).
5. Write the loop that reads a request frame from your link (`dagon_port::Link`), writes the answer of `Agent::handle`, and resets when asked to. Below is an excerpt of the loop the MCU definitions use (`targets/mcu/common/src/serve.rs`). `host` is what differs between MCUs (clock, logging, waiting). When it returns `Exit::Reboot`, the caller resets.

   ```rust
   let (via, addr) = (link.kind().code(), link.addr());
   loop {
       let read = link.read_frame().await;
       let request = match read {
           Ok(r) => r,
           Err(LinkError::Timeout) => {
               // after IDLE_TIMEOUT_MS without requests, agent.abort() drops the transfer (left out)
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
       // a conformance build answering first (host.intercept) is left out
       match agent.handle(request) {
           Some(reply) => {
               reboot = reply.reboot.then_some(reply.reboot_delay_ms);
               link.write_frame(reply.frame).await
           }
           None => link.no_reply().await,
       };
       // a reply that could not be written: wait for the next request (left out)
       if let Some(delay_ms) = reboot {
           if link.flush().await.is_ok() {
               host.wait_ms(delay_ms).await;
               agent.before_reboot();
               return Exit::Reboot;
           }
       }
   }
   ```

6. Feed the watchdog: all the time while confirmed, and only for a limited time while on trial. An image that is never confirmed is then reset and goes back to the previous version.
7. Check the port with `dagon conform`.
