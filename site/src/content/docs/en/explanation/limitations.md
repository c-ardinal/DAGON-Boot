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
- DAGON writes the hub and the children beyond it. Nothing beyond a child can
  be written.
- The USB VID:PID `c0de:cafe` is a test ID, not an assigned one.
- `dagon conform` drives the synchronous test sequence and is CLI only; the
  browser does not run conformance tests.
