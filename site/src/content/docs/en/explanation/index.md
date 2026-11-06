---
title: How DAGON works
description: The ideas behind DAGON's writes, formats and extensions.
sidebar:
  order: 0
---

DAGON is the boot stage and the write path of every node in a 構成. A
親機 (hub) on your PC's USB passes each new version on to the 子機
(nodes) over I2C or UART. A node writes the new image beside the running
one, starts it on trial and keeps it only once the new version confirms
itself; otherwise it goes back to the previous version by itself.

Every state change in DAGON is a transition of a state machine, and every
transition emits an event (see [Events](/en/reference/events/)). Extensions
observe those events and answer a few decisions through
[ports](/en/reference/ports/).

Every format, protocol and API DAGON reads or writes carries a number, and
a reader refuses a number it does not know.

- [Words used here](/en/explanation/words/)
- [Versions and releases](/en/explanation/versioning/)
- [Lifecycle events and extensions](/en/explanation/lifecycle-events/), and the
  [retained and persistent formats](/en/explanation/retained-formats/) it leaves to an executable model
- [The repository](/en/explanation/repository/)
- [Limitations](/en/explanation/limitations/)
