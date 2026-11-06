---
title: "Limitations"
description: "Where the code differs from the design and what DAGON does not do, as by-design limits and planned ones."
---

## By design

- How to protect the signing key is the user's choice. DAGON does not decide it. Whether to use a hardware root of trust (secure boot fuses, key storage in hardware) is part of that choice.
- `dagon conform` drives the synchronous test sequence and is CLI only. The browser does not run conformance tests.

## Planned

The hub and the boot:

- Only an RP2040 can be the hub. The relay code lives in that MCU's definition.
- DAGON writes the hub and the children beyond it. Nothing beyond a child can be written.
- There are no single-slot layouts.
- The boot loader does not communicate and does not write the app.
- The ESP32-S3 uses the official ESP-IDF 2nd stage. If that 2nd stage itself is broken, DAGON cannot rescue the board; it has to be rewritten over USB with the vendor tools.

Extension isolation:

- The ESP32-S3's isolation level is `none`. No code programs the PMS and WCL registers.
- There is no sandbox at run time (Service ABI, Entry Gate, Operation Cancellation, Fault Unwind). Definitions are checked and protection maps are generated.

The signing key:

- The signing key is read only from a file. The environment variable `DAGON_SIGNING_KEY` also holds a file's path. No port lets you choose another means (an environment variable, the OS key store, 1Password and so on).

Values and IDs:

- The USB VID:PID `c0de:cafe` is a test ID, fixed in the code. It is not an assigned one.
- DAGON's own MCU definitions hold values whose source has not been checked. They are marked `unverified` in the definition.
- The execution model's budgets ([section 12](/en/explanation/lifecycle-events/#12-execution-model)) are not values measured on hardware.
- That the boot loader's trial cell keeps its value rests only on the vendors' documents. It is not checked per board.

Components in the design figure that the code does not have:

- Ports with no replaceable trait: BuildRunner, Signer, DecisionPolicy, LogStorage, SelfCheck, UpdateTransport, UpdateProtocol, Recovery, ChildLink, ImageVerifier, Digest, CryptoProvider, WatchdogDriver, Diagnostics, Display, LogSink, Agent, Action
- On the hub: Coordinator, Release Store, Bus Guardian. The PC drives a write through the hub.
- On the host: Host Extension Chain, Action Definitions
- At boot: Boot Record Writer, Boot Action Interpreter
