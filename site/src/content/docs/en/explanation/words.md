---
title: "What the words mean"
description: "The terms these pages use."
---

For application developers, extension authors and DAGON contributors.

- **node**: one microcontroller of your setup.
- **hub**: the node on the PC's USB. It relays writes to the other nodes.
- **child**: a node linked to the hub, over a link of any kind.
- **grandchild**: a node beyond a child.
- **topology**: the nodes, how they are linked, and the write order (`topology.yaml`).
- **link**: a connection between nodes.
- **image**: a program written to a node, made of a signed header and a body.
- **package**: a file (`.dgb`) holding the images signed for a topology.
- **slot**: the flash area that holds one image. The MCU definition (`[[slot]]`) gives each image kind one or two.
- **trial**: how a new version runs after its write and before it is confirmed. A new version that is not confirmed goes back to the previous one, where a slot holds it.
- **confirm**: the decision to keep a new version on trial. By default the host's CONFIRM starts it and the node confirms after its self-check passes (with `Confirm::Own` the start confirms by itself).
- **boot loader**: DAGON's boot stage that starts before the app (image kind `bootcore`). It decides which slot starts.
- **stage0**: DAGON's small program that starts first after a reset. It picks the boot loader's slot and is never written in the field.
- **2nd stage**: the vendor's program that starts after the boot ROM. On the ESP32-S3 (port `vendor-stage2`), ESP-IDF's 2nd stage picks the slot that starts.
- **recovery agent**: the image (kind `agent`) that receives a write when no app slot can start. On a vendor-stage2 MCU (the ESP32-S3) it stays in the factory partition from the factory on.
- **update mode**: the state in which the boot loader receives a write. A STAY within the listen window, or a REBOOT mode 1 to the app, enters it.
- **recovery mode**: the state the boot loader stays in, receiving a write, when no app can start.
- **listen window**: the time right after the boot loader starts in which it takes a STAY (`[boot_receive] listen_window_ms`).
- **component**: a unit released on its own, with its own SemVer version (`[[component]]` in `compat.toml`).
- **contract**: a compatibility agreement between components (a file format, a protocol, an API).
- **format number**: a contract's integer number. A reader refuses a number it does not know.
- **lost / never reached / history unknown**: the three cases of a node that does not answer a relayed request. Lost: a record says it answered before. Never reached: the records are complete and none is of it. History unknown: the records cannot be checked. A write stops at a lost node and at one with unknown history, and skips a node never reached ([section 1.5 of Lifecycle events and extensions](/en/explanation/lifecycle-events/#15-host-operations)).
