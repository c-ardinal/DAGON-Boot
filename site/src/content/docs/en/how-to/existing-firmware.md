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
4. Compose the app layer's chain and start the agent with your link. Your extensions answer the decision points and observe the events (see [Events](/en/reference/events/)).

   ```rust
   let chain = dagon_events::compose(Layer::App, events, board_exts, user_exts, bounds);
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, chain);
   agent.run(&mut link, &mut reset).await;                    // serves requests forever
   ```

5. Feed the watchdog: all the time while confirmed, and only for a limited time while on trial. An image that is never confirmed is then reset and goes back to the previous version.
6. Check the port with `dagon conform`.
