---
title: "Swapping a link"
description: "Change how a node is linked and send the new topology."
---

## In the 構成

Every link in a 構成 has a kind: `usb-cdc`, `uart`, `i2c`, `ble`, `wifi-tcp`, `ethernet` or `can`.

A 親機 can also declare its uplink, the link towards whatever drives it, and what is at the other end:

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

Without `uplink` it is `usb-cdc` to dagon on a PC.

The uplink is not part of the compiled topology, so existing 構成 and their packages stay the same.

## In DAGON Console

In 準備 (define), the inspector offers each kind for a link and, on the 親機, for its uplink.

## Which kinds work

A kind the firmware of either end does not implement is marked ◇ まだ使えない.

- The 構成 may still name it (problem まだ使えないつながり).
- `dagon gen` refuses it and names the link (`uplink of hub over ble: not available yet on rp2040`).

The kinds the firmware offers are in the Up links and Down links columns of [Boards and MCUs](/en/reference/boards/). Up is towards the PC or the 親機, and down is to 子機. The data comes from each MCU definition's `[links]`.
