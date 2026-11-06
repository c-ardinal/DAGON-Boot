---
title: "Limitations"
description: "Where the code differs from the design and what DAGON does not do, as by-design limits and planned ones."
---

## By design

- Signing keys are files on the PC and the public key is built into the firmware. There is no hardware root of trust (no secure boot fuses, no key storage in hardware).
- The USB VID:PID `c0de:cafe` is a test ID, not an assigned one.
- `dagon conform` drives the synchronous test sequence and is CLI only. The browser does not run conformance tests.
- A definition value whose source has not been checked is marked `unverified` in the definition.
- The execution model's budgets ([section 12](/en/explanation/lifecycle-events/#12-execution-model)) are not values measured on hardware.
- That the boot loader's trial cell keeps its value rests on the vendors' documents. It is not checked per board.

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

Components in the design figure that the code does not have:

- Ports with no replaceable trait: BuildRunner, Signer, KeyStore, DecisionPolicy, LogStorage, SelfCheck, UpdateTransport, UpdateProtocol, Recovery, ChildLink, ImageVerifier, Digest, CryptoProvider, WatchdogDriver, Diagnostics, Display, LogSink, Agent, Action
- On the hub: Coordinator, Release Store, Bus Guardian. The PC drives a write through the hub.
- On the host: Host Extension Chain, Action Definitions
- At boot: Boot Record Writer, Boot Action Interpreter
