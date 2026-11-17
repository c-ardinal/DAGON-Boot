---
title: "Limitations"
description: "Where the code differs from the design and what DAGON does not do, as by-design limits and planned ones."
---

## By design

- How to protect the signing key is the user's choice. DAGON does not decide it. Whether to use a hardware root of trust (secure boot fuses, key storage in hardware) is part of that choice.
- An MCU definition gives each image kind one slot or two (`[[slot]]`), and one slot loads only when `[writers]` names another kind that writes it. A kind with one slot cannot do this:

| Image kind | Previous version | A power cut during a write | What recovers it |
|---|---|---|---|
| `app` | None. A new version that fails its trial has none to go back to; `dagon rollback` answers that there is no previous version, and DAGON Console offers no rollback for the node. | No app is left to start in the slot. | The kind that writes the app (`[writers] app`) starts and receives the app again; on the ESP32-S3 the 2nd stage starts the factory recovery agent. |
| `bootcore` | None. A new boot loader that fails its trial has no previous one to go back to. | Stage0 has no boot loader it may start and starts the app the state log's confirmed APP record names, after checking it. While the app is on trial, its agent refuses a boot loader transfer and its pending setting, so this cut never meets an app trial. | That confirmed app (`[writers] bootcore`) writes the boot loader again. Only without a confirmed app does stage0 start nothing and wait for recovery through the ROM loader. |
| `agent` | None. | The confirmed app keeps running; the recovery agent cannot be used until it is written again. | The confirmed app (`[writers] agent`) writes it again. |
| `stage0` | None: DAGON never writes stage0 in the field. | Does not arise: stage0 is not written in the field. | The MCU's ROM loader (`dagon flash`). |

- The boot loader does not relay. A node reached only through a node whose boot loader holds it in update or recovery mode is reached again once that node runs an app: write that node first.

## Planned

The hub and the boot:

- The relay (`dagon-relay`) does not depend on the MCU, and the `[coordinator]` section of an MCU definition decides whether that MCU may relay. A node relays over the links its MCU definition lists in `[[relay_link]]`, and the topology routes each node to one of them. The RP2040 is the only MCU with both that section and the code that runs the relay.
- DAGON writes the hub and the children beyond it. Nothing beyond a child can be written.
- Through some USB hubs, the PC does not see the hub again after it updates itself and restarts. Connect the hub directly to a USB port of the PC.
- The boot loader of the STM32F401 does not communicate. The MCU definition names the boot loader a writer of the app (`[writers] app = ["app", "bootcore"]`) and gives its listen window (`[boot_receive]`), but the boot loader firmware opens no listen window, serves no update or recovery mode and does not consume UPDATE_STAY; the app answers REBOOT mode 1 with `E_UNSUPPORTED`. An app with one slot on this MCU has nothing that writes it again after a cut write.
- The RP2040's boot loader takes its I2C address from the board's `[default_link] addr`, not yet from the address the topology gives the node's link end. On a board running on the ring oscillator its UART makes its bit rate from the nominal 6.5 MHz, so the rate may be off.
- The measured worst-case verify time of stage0 on the built-in MCUs is not recorded. [Stage0's clock and its verify time](/en/explanation/lifecycle-events/) gives an emulator's estimate for the Cortex-M0+; there is no estimate for the STM32F401's Cortex-M4.
- On the RP2040 a boot takes about 15 s on a board without a crystal (the ring oscillator, nominally 6.5 MHz) and about 0.8 s with a 12 MHz crystal: stage0 and the boot loader each check one signature with `Ed25519Small` on every boot. The boot loader keeps its watchdog fed through its check. This is by design: the small verifier leaves the boot loader room for the update session.
- On the RP2040, stage0, the boot loader and the app take their clock from a crystal or from the ring oscillator; they do not take an external clock driven into XIN.
- The ESP32-S3 uses the official ESP-IDF 2nd stage. If that 2nd stage itself is broken, DAGON cannot rescue the board; it has to be rewritten over USB with the vendor tools.

Extension isolation:

- The ESP32-S3's isolation level is `none`. No code programs the PMS and WCL registers.
- There is no sandbox at run time (Service ABI, Entry Gate, Operation Cancellation, Fault Unwind, Lockup Fuse, Reset Cleanup, SandboxArch). Definitions are checked and protection maps are generated.
- Protection maps can be generated only for the kinds `armv6m-mpu` and `armv7m-mpu`.

Links:

- Firmware exists only for the link kinds `usb-cdc`, `uart` and `i2c`. `ble`, `wifi-tcp`, `ethernet` and `can` have none.

Identity and VERSION:

- The MCU identity is a hand-assigned 16-bit `wire_id`, carried in the image header, the identity reply, VERSION 0x06 and the topology node. It is not the 64-bit name hash of the design.
- VERSION answers only ids 0x01, 0x06, 0x0A, 0x0B, 0x0C and 0x0D, and refuses a non-empty request.

The signing key:

- The signing key is read only from a file. The environment variable `DAGON_SIGNING_KEY` also holds a file's path. No port lets you choose another means (an environment variable, the OS key store, 1Password and so on).

Values and IDs:

- The USB VID:PID `c0de:cafe` is a test ID, fixed in the code. It is not an assigned one.
- DAGON's own MCU definitions hold values marked `unverified`.
- The execution model's budgets ([section 12](/en/explanation/lifecycle-events/#12-execution-model)) are not set the way 12.3 says.

Components in the design figure that the code does not have:

- Ports with no replaceable trait: BuildRunner, Signer, DecisionPolicy, LogStorage, UpdateProtocol, Recovery, ImageVerifier, Digest, CryptoProvider, Diagnostics, Display, LogSink, Agent, Action
- On the hub: Coordinator, Release Store, Bus Guardian. The PC drives a write through the hub.
- The write plan: the Delivery Plan's trial window and promotion judgement
- Recovery: Recovery Policy's and Node Recovery's automatic choice of tier and bounded retry
- On the host: Host Machines, Host Extension Chain, Action Definitions
- At boot: Boot Record Writer, Boot Action Interpreter
- Retained Codecs: the boot record's and the lockup fuse's formats exist only in the executable model, not in the firmware.
- The topology body: the firmware's format is `dagon-format`'s (a 32-byte head, 32-byte nodes), not the one in [Retained and persistent formats](/en/explanation/retained-formats/). The hub stores it in one region, with no two-bank replacement.
