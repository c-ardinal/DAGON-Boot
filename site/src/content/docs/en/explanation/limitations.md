---
title: "Limitations"
description: "What DAGON does not do by design, what the user is responsible for, and where the code differs from the design."
---

For application developers, extension authors and DAGON contributors. The last section of this page is the one place that says where the code differs from the design.

## By design

- An MCU definition gives each image kind one slot or two (`[[slot]]`), and one slot loads only when `[writers]` names another kind that writes it. A kind with one slot cannot do this:

| Image kind | Previous version | A power cut during a write | What recovers it |
|---|---|---|---|
| `app` | None. A new version that fails its trial has none to go back to; `dagon rollback` answers that there is no previous version, and DAGON Console offers no rollback for the node. | No app is left to start in the slot. | The kind that writes the app (`[writers] app`) starts and receives the app again; on the ESP32-S3 the 2nd stage starts the factory recovery agent. |
| `bootcore` | None. A new boot loader that fails its trial has no previous one to go back to. | Stage0 has no boot loader it may start and starts the app the state log's confirmed APP record names, after checking it. While an app is pending or on trial, its agent refuses a boot loader transfer and its pending setting, and while the boot loader is not confirmed (pending, or its slot left empty by a cut write) it refuses an app's pending setting, so neither this cut nor a boot loader trial ever meets an app trial. | That confirmed app (`[writers] bootcore`) writes the boot loader again. Only without a confirmed app does stage0 start nothing and wait for recovery through the ROM loader. |
| `agent` | None. | The confirmed app keeps running; the recovery agent cannot be used until it is written again. | The confirmed app (`[writers] agent`) writes it again. |
| `stage0` | None: DAGON never writes stage0 in the field. | Does not arise: stage0 is not written in the field. | The MCU's ROM loader (`dagon flash`). |

- The boot loader does not relay. A node reached only through a node whose boot loader holds it in update or recovery mode is reached again once that node runs an app: write that node first.

- The ESP32-S3 boots through the official ESP-IDF 2nd stage (the port `vendor-stage2` in compat.toml). A broken 2nd stage is outside what DAGON recovers: it has to be rewritten over USB with the vendor tools.
- On the RP2040 a boot takes about 15 s on a board without a crystal (the ring oscillator, nominally 6.5 MHz) and about 0.8 s with a 12 MHz crystal: stage0 and the boot loader each check one signature with `Ed25519Small` on every boot. The boot loader keeps its watchdog fed through its check. This is by design: the small verifier leaves the boot loader room for the update session.
- On a board running on the ring oscillator, the RP2040's boot loader makes its UART bit rate from the nominal 6.5 MHz, so the rate may be off, and its I2C target follows a 400 kHz bus only with a crystal; on the ring oscillator use 100 kHz.
- DAGON Console writes the topology file back in its own layout. It keeps a hand-written comment only on a line whose content did not change (indentation aside): the comments of lines changed or removed in DAGON Console, and of lines written in another style than its own (quotes, flow style), are not kept, and blank lines follow its layout. Keeping every comment would take a YAML library that keeps the syntax tree, which DAGON Console does not carry.

## The user's responsibility

- How to protect the signing key is the user's choice. DAGON does not decide it. Whether to use a hardware root of trust (secure boot fuses, key storage in hardware) is part of that choice.
- An extension is the user's own trusted code and runs in the app and the host only. DAGON does not isolate it. Reviewing an extension's content, the crates it depends on and the source it takes in is the user's responsibility.
- An AI agent (a command agent connected through the serve API) runs with the user's rights. DAGON puts no sandbox around it; running it in an OS sandbox is the user's responsibility (see [Agent link: serve API](/en/reference/agent-api/)).
- The USB VID:PID `c0de:cafe` is a test ID, fixed in the code. It is not an assigned one. Whether it may be used in a product is the user's decision.
- Through some USB hubs, the PC does not see the hub again after it updates itself and restarts. Connect the hub directly to a USB port of the PC.

## Where the code differs from the design

The hub and the boot:

- The relay (`dagon-relay`) does not depend on the MCU, and the `[coordinator]` section of an MCU definition decides whether that MCU may relay. A node relays over the links its MCU definition lists in `[[relay_link]]`, and the topology routes each node to one of them. The RP2040 is the only MCU with both that section and the code that runs the relay.
- DAGON writes the hub and the children beyond it. Nothing beyond a child can be written.
- Updating reaches the first coordinator and the nodes linked to it over I2C or UART. It skips a second coordinator and the nodes beyond it, and nodes linked by any other kind of link. The 構成 check tells them as information (◇).
- The boot loader of the STM32F401 receives only on its board's DAGON I2C bus (`[[bus]]` with `dagon = true`). A node without a board, or on a board without such a bus, gets a boot loader that opens no listen window and has no update mode. It enters update mode by a STAY within the listen window or by an UPDATE_STAY, but it has no recovery mode: with no app to start it halts. Its app answers REBOOT mode 1 with `E_UNSUPPORTED`, so the hand-over to update mode goes through the listen window only. The ESP32-S3's recovery agent receives only when the 2nd stage starts it because no app slot starts: it opens no listen window and answers HELLO with mode 3, not 1 or 2. The boot loader that takes its place behind DAGON's own stage0 (targets/mcu/esp32s3/bootcore: a listen window on UART0, the app's hand-over and recovery mode) runs only in the emulator until the ESP32-S3 definition gives stage0's layout, and the app on that MCU does not hand over yet (its REBOOT mode 1 answers `E_UNSUPPORTED`).
- The measured worst-case verify time of stage0 on the built-in MCUs is not recorded. [Stage0's clock and its verify time](/en/explanation/lifecycle-events/) gives an emulator's estimate for the Cortex-M0+; there is no estimate for the STM32F401's Cortex-M4.
- On the RP2040, stage0, the boot loader and the app take their clock from a crystal or from the ring oscillator; they do not take an external clock driven into XIN.
- Only the app's session reports the reset cause (`reset` in `dagon status`); the boot loader's sessions (update and recovery mode) do not. The ESP32-S3 boot loader on the emulator (targets/mcu/esp32s3/bootcore) asks for the app by a reset, so that app always reports `software`. No record is kept of the hub driving a node's NRST.

Parts the design no longer has that the code still declares:

- In `dagon_events::PORTS`: the boot layer, the trust policy (`trusted only` and so on) and services columns, the `Action` port, the note that `Display` runs in the sandbox at boot, and the `BootRecord` and `Sandbox` records. The table of section 2.1 of [Lifecycle events and extensions](/en/explanation/lifecycle-events/) is generated from them.
- VERSION ids 0x07, 0x08, 0x09, 0x11 and 0x12 in `version_id::TABLE`. The table of section 5.4 equals it.
- Boot-stage extensions: the `boot_extensions` machine (`boot.extension_overran`), `targets/boot-extension-template`, the manifest's `trusted`, `stack_bytes` and `ram_bytes`, and `dagon-extension`'s protection map generation and trusted-extension checks.
- The MCU definitions' `[isolation]`, `[[peripheral]]` and `[boot.record]`. The design's `root_protection` is a table of its own; the code keeps it as a field of `[isolation]` and moves it to its own table with the code change.
- The models of the boot record, the lockup fuse, the state log's keyed records and the link diagnostics (DTC) in `dagon-retained-model`.
- `REPLAY`, the decoder for the boot record's replay that `state_machine!` generates.

Links:

<!-- generated from the MCU definitions' [links]: python site/scripts/gen.py -->
- Firmware exists only for the link kinds `usb-cdc`, `uart` and `i2c`. `ble`, `wifi-tcp`, `ethernet` and `can` have none.
<!-- end of generated table -->

Identity and VERSION:

- VERSION answers only ids 0x01, 0x06, 0x0A, 0x0B, 0x0C and 0x0D, and refuses a non-empty request.

Values:

<!-- generated from MAX_NODES and the sample hardware test setups: python site/scripts/gen.py -->
- A 構成 holds at most `MAX_NODES`, 32 nodes. The base setup of the sample test that writes boards (`samples/hardware-test/hwt.toml`) has 3 nodes. Writes to 4 or more nodes are tested by a simulator test of 32 nodes (`one_write_reaches_every_node_of_a_topology_at_the_limit` in `dagon-host`) and by a board setup of 4 nodes (`hwt-fan.toml`).
<!-- end of generated table -->
- DAGON's own MCU definitions hold values marked `unverified`. `dagon release` and `dagon deploy` warn of each such value the firmware of a node in the release is built with (the interrupt masking budget and the erase times); `dagon release --checked-facts` refuses a release that still has one (RB-06).
- The execution model's budgets ([section 11](/en/explanation/lifecycle-events/#11-execution-model)) are not set the way 11.3 says.

Parts of the design the code lacks, in whole or in part:

<!-- generated from dagon_events::PORTS: DAGON_BLESS=1 cargo test -p dagon-catalog --test docs -->
- Ports with no trait in the code (where it is to go): `UpdateProtocol` (`dagon_agent::UpdateProtocol`), `LogStorage` (`dagon_core::LogStorage`), `Recovery` (`dagon_port::Recovery`), `Diagnostics` (`dagon_agent::Diagnostics`), `Display` (`dagon_events::Display`), `LogSink` (`dagon_events::LogSink`), `Action` (`dagon_host::Action`)
- `BootRecord`: The firmware writes no boot record: its format exists only in the executable model (`dagon-retained-model`).
- `Sandbox`: There is no sandbox at run time (Service ABI, Entry Gate, Operation Cancellation, Fault Unwind, Lockup Fuse, Reset Cleanup, SandboxArch). Definitions are checked and protection maps are generated.
<!-- end of generated list -->
<!-- generated from site/scripts/architecture.py: python site/scripts/gen.py -->
- AI Agent Link: `dagon serve` has no job that ports DAGON to a board: `POST /v1/ai/port` answers 501.
- Boot Action Interpreter: Not in the code.
- Retained Codecs: The firmware has none of the retained codecs: the topology body is `dagon-format`'s (a 32-byte head, 40-byte nodes), as section 3.1 of [Retained and persistent formats](/en/explanation/retained-formats/) gives it. No node stores the topology: it stays in the host's package, and target 3 (where a node once stored it) stays a reserved number no node takes.
<!-- end of generated table -->
