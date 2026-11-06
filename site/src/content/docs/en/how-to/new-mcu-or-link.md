---
title: "Adding an MCU or a link"
description: "What to write, and what stays untouched, when DAGON gets a new MCU or link."
---

`crates/` does not change. MCU knowledge lives only in `targets/mcu/<id>/`. The layer figures are in [Explanation](/en/explanation/).

## Adding an MCU

1. Make the crate `dagon-mcu-<id>` in `targets/mcu/<id>/`, with its own `[workspace]`; the root `Cargo.toml` does not change.
2. Write `dagon-mcu.toml`. The keys are in [MCU and board definitions](/en/reference/definitions/); adding the folder adds the definition.
3. Implement the traits of `crates/dagon-port/src/lib.rs`: `Flash`, `Link`, `Reset`, `Clock`, `Watchdog`, and by boot kind `BootSelectS` or `BootSelectV`. An MCU that can be the hub also implements `StreamPort`, `I2cBus` and `SubControl`.
4. Implement `SlotStore` of `crates/dagon-agent/src/lib.rs` (`targets/mcu/stm32f401re/src/store.rs` is an example).
5. An MCU with DAGON's boot stage provides `stage0/` and `bootcore/`, and every MCU provides `template/`, from which `dagon gen` makes the node crates (`targets/mcu/stm32f401re/` is an example).
6. Add the component to `compat.toml` ([Versions and releases](/en/explanation/versioning/)).
7. Check with `dagon sim`, then run `dagon conform` through the hub. `plugin/skills/dagon-port/SKILL.md` walks the same steps in Claude Code.

## Adding a link

The link kinds are a closed list, in one place in the code.

1. Add one line to `with_link_kinds!` in `crates/dagon-format/src/link.rs`. The name a 構成 uses and the code events carry come from it.
2. On each MCU that has the link, implement `dagon_port::Link` (`StreamPort` for a byte stream from the hub to a child). `targets/mcu/esp32s3/src/links.rs` is an example.
3. Add it to the MCU's `LINK_KINDS` (`targets/mcu/rp2040/src/kinds.rs`) and to `[links]` in `dagon-mcu.toml`.
4. For a new medium on the PC side, implement `Transport` of `crates/dagon-host/src/lib.rs` (`crates/dagon-cli/src/serial.rs` is an example).

To replace the transport and protocol without changing DAGON's source, provide the `UpdateTransport` and `UpdateProtocol` ports in an extension ([Lifecycle events and extensions](/en/explanation/lifecycle-events/), 8.4).
