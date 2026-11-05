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
- A systematic survey of power cuts at every flash operation on real
  hardware has not been done; power cuts are tested in the simulator.
- The USB VID:PID `c0de:cafe` is a test ID, not an assigned one.
- The F401 node and the ESP32-S3 node firmware have not been checked on
  the hardware. The hub firmware (RP2040 with LCD) has been flashed and
  starts on the hardware, but its host communication and display are still
  being validated, and so is the full three-board flow.
- `dagon conform` drives the synchronous test sequence and is CLI only; the
  browser does not run conformance tests.
