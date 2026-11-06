---
title: "Adding DAGON to existing firmware"
description: "Give a firmware you already have DAGON's agent and writes."
---

For firmware that is not a `dagon new` project:

1. Add the crates as git dependencies pinned to a release tag, for example
   `dagon-agent = { git = "<repository URL>", tag = "<release tag>" }`, and
   the same for `dagon-format`, `dagon-proto`, `dagon-port` and, for a boot
   core, `dagon-core`.
2. On a supported MCU, use the MCU package (`targets/mcu/<mcu>`: flash
   layout, linker map, boot images, watchdog, links) and a board package
   (`targets/boards/<board id>`), or implement the MCU package's `BoardDef`
   for your own board; the MCU package READMEs show a complete app in about
   20 lines. On another MCU, lay out the flash the same way: a boot loader,
   two state log faces, and slots A and B, each starting with the 512-byte
   signed image header and the body at slot start + 0x200.
3. Implement the `dagon-port` traits for your board (`Flash`, `Link`,
   `Reset`, `Watchdog`, and `BootSelectS` or `BootSelectV`) and a
   `dagon_agent::SlotStore`, compose the app layer's chain (your
   extensions answer the decision points and observe the events, see
   [Events](/en/reference/events/)), then start the agent with your link:

   ```rust
   let chain = dagon_events::compose(Layer::App, events, board_exts, user_exts, bounds);
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, chain);
   agent.run(&mut link, &mut reset).await;                    // serves requests forever
   ```

   Feed the watchdog while confirmed, and only for a limited time while on
   trial, so an image that is never confirmed is reset and rolled back.
4. Check the port with `dagon conform`.
