---
title: "Swapping a link"
description: "Change how a node is linked."
---

## In the 構成

<!-- generated from dagon_format::with_link_kinds!: python site/scripts/gen.py -->
Every link in a 構成 has a kind: `usb-cdc`, `uart`, `i2c`, `ble`, `wifi-tcp`, `ethernet` or `can`.
<!-- end of generated table -->

A coordinator can also declare its uplink, the link towards whatever drives it, and what is at the other end:

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

Without `uplink` it is `usb-cdc` to dagon on a PC.

The uplink is not part of the compiled topology, so existing 構成 and their packages stay the same.

## In DAGON Console

In 準備 (define), the inspector offers each kind for a link and, on the coordinator, the kinds its MCU definition offers up for its uplink.

## Which kinds work

A kind the firmware of either end does not implement is marked （使用不可）.

- The 構成 may still name it (problem 使用不可の接続).
- `dagon gen` refuses it and names the link (`uplink of hub over ble: the firmware of rp2040 does not run this link kind`).

The kinds the firmware offers are in the Up links and Down links columns of [Boards and MCUs](/en/reference/boards/). Up is towards the PC or the coordinator, and down is to the nodes at the other end of the link. The data comes from each MCU definition's `[links]`.
