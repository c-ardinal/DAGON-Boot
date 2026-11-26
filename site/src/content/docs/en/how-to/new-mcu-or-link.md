---
title: "Adding an MCU or a link"
description: "What to write, and what stays untouched, when DAGON gets a new MCU or link."
---

The layer figure is in [Explanation](/en/explanation/).

## Adding an MCU

`crates/` does not change. MCU knowledge lives only in `targets/mcu/<id>/`.

1. Make the crate `dagon-mcu-<id>` in `targets/mcu/<id>/`, with its own `[workspace]`. The root `Cargo.toml` does not change.
2. Write `dagon-mcu.toml`. The keys are in [MCU and board definitions](/en/reference/definitions/); adding the folder adds the definition.
3. Implement the traits of `crates/dagon-port/src/lib.rs`: `Flash`, `Link`, `Reset`, `Watchdog`, and by boot kind `BootSelectS` or `BootSelectV`. An MCU that can be the hub also implements `StreamPort`, `I2cBus` and `SubControl`. The microsecond clock is not a port but DAGON's own: the MCU package implements `Clock` of `crates/dagon-events/src/compose.rs`.
4. Implement `SlotStore` of `crates/dagon-port/src/slot_store.rs` (`targets/mcu/stm32f401re/src/store.rs` is an example).
5. An MCU with DAGON's boot stage provides `stage0/` and `bootcore/` (`targets/mcu/stm32f401re/` is an example).
6. Every MCU provides `template/`, from which `dagon gen` makes the node crates.
7. Add the component to `compat.toml` ([Versions and releases](/en/explanation/versioning/)).
8. Check with `dagon sim`, then run `dagon conform` through the hub.
9. Check the power-cut acceptance criterion: port `targets/mcu/stm32f401re/tests/explore.rs` and `update_mode.rs` (with `tests/cut/`) to the new layout and run them on the host. Every flash operation (each slot's erase, body and header; each state log record; each step of a log compaction) is cut every way (not landed or half landed, with a power loss or a reset), and every state must recover. A flash operation the search does not cut, or one outside the inventory, fails the test. With the folder in `DAGON_POWERCUT_REPORT`, the tests write the cuts per flash operation as Markdown tables. For an MCU whose vendor boot stage selects the slot (store kind V), port `targets/mcu/esp32s3/src/tests_cut.rs` instead.

`plugin/skills/dagon-port/SKILL.md` walks the same steps in Claude Code.

## Adding a link

The link kinds are a closed list, in one place in the code, so this does change `crates/`.

1. Add one line to `with_link_kinds!` in `crates/dagon-format/src/link.rs`. The name a 構成 uses and the code events carry come from it. The line's traits say what the medium is: `compiled` (the compiled topology carries it between a coordinator and a node), `addressed` (its nodes are reached at a 7-bit address), `enumerated` (the other end enumerates it before it carries a frame).
2. On each MCU that has the link, implement `dagon_port::Link` (`StreamPort` for a byte stream from the hub to a child). `targets/mcu/esp32s3/src/links.rs` is an example.
3. Add it to `[links]` in the MCU's `dagon-mcu.toml`. The package's `LINK_KINDS` (`targets/mcu/rp2040/src/kinds.rs`) is built from `[links]` and changes with it.
4. Add the words DAGON Console shows for it to `LINK_KIND_WORDS` in `web/src/app/copy.ts` (`label` on the canvas and in the table, `choice` in the inspector's list). DAGON Console's tests fail until it is there.
5. For a new medium on the PC side, implement `Transport` of `crates/dagon-host/src/lib.rs` (`crates/dagon-cli/src/serial.rs` is an example).

To replace the transport and protocol without changing DAGON's source, provide the `UpdateTransport` and `UpdateProtocol` ports in an extension ([Lifecycle events and extensions](/en/explanation/lifecycle-events/), 8.4).
