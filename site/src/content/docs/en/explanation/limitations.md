---
title: "Limitations"
description: "What DAGON does not do, and where its guarantees stop."
---

- The ESP32-S3 uses the official ESP-IDF 2nd stage. If that 2nd stage itself
  is broken, DAGON cannot rescue the board; it has to be reflashed over USB
  with the vendor tools.
- Signing keys are files on the PC and the public key is built into the
  firmware. There is no hardware root of trust (no secure boot fuses, no key
  storage in hardware).
- Power cuts have not been run on hardware. In the simulator, `dagon sim run`
  has one power-cut scenario (a cut during a transfer to the F401), and DAGON's
  tests cut the power at every flash operation of the hub's update.
- The USB VID:PID `c0de:cafe` is a test ID, not an assigned one.
- The last run on hardware passed 16 scenarios on all three nodes (hub RP2040,
  F401, ESP32-S3), before the write flow changed. The current scenario set has
  not been run since; `samples/hardware-test` runs it. Boot core updates are
  unproven on hardware. The F401's ROM recovery has not been run on hardware.
- `dagon conform` drives the synchronous test sequence and is CLI only; the
  browser does not run conformance tests.
