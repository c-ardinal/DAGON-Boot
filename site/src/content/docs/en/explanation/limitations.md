---
title: "Limitations"
description: "Where the code differs from the design and what DAGON does not do, as by-design limits and planned ones."
---

## By design

- How to protect the signing key is the user's choice. DAGON does not decide it. Whether to use a hardware root of trust (secure boot fuses, key storage in hardware) is part of that choice.

## Planned

The hub and the boot:

- The relay (`dagon-relay`) does not depend on the MCU, and the `[coordinator]` section of an MCU definition decides whether that MCU may relay. A node relays over the links its MCU definition lists in `[[relay_link]]`, and the topology routes each node to one of them. The RP2040 is the only MCU with both that section and the code that runs the relay.
- DAGON writes the hub and the children beyond it. Nothing beyond a child can be written.
- Through some USB hubs, the PC does not see the hub again after it updates itself and restarts. Connect the hub directly to a USB port of the PC.
- There are no single-slot layouts.
- The boot loader does not communicate and does not write the app.
- The measured worst-case verify time of stage0 on the built-in MCUs is not recorded. [Stage0's clock and its verify time](/en/explanation/lifecycle-events/) gives an emulator's estimate for the Cortex-M0+; there is no estimate for the STM32F401's Cortex-M4.
- The RP2040's stage0 takes its clock from a crystal or from its ring oscillator; it does not take an external clock driven into XIN.
- Only stage0 takes its clock from the definitions. The boot loader and the app set up their clocks with their HAL settings: on the RP2040 a 12 MHz crystal to 125 MHz, on the STM32F401 the HSI to 84 MHz.
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
