---
title: "Swapping a link"
description: "Change how a node is linked and send the new topology."
---

Every hop in a 構成 has a kind: `usb-cdc`, `uart`, `i2c`, `ble`,
`wifi-tcp`, `ethernet` or `can`. A 親機 can also declare its uplink, the
hop towards whatever drives it, and what is at the other end:

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

Without `uplink` it is `usb-cdc` to dagon on a PC, as before; the uplink is
not part of the compiled topology, so existing 構成 and their releases stay
the same. In DAGON Console's 定義 the inspector offers each kind for a link
and, on the 親機, for its uplink. Kinds the firmware of either end does not
implement yet are marked ◇ まだ使えない; the 構成 may still name them
(problem まだ使えないつながり), and `dagon gen` refuses them with the hop's
name (`uplink of hub over ble: not available yet on rp2040`). The
firmware offers, as each MCU's definition lists it (`[links]`; up: towards
the PC or the 親機, down: to 子機):

- RP2040: up usb-cdc; down i2c, uart
- STM32F401RE: up i2c
- ESP32-S3: up uart, usb-cdc

The demo shows the swap without new radios:

```sh
dagon demo --uplink uart          # or usb-cdc (default), wifi-tcp
```

The simulated uplink follows the demo 構成: change the 親機's uplink in
定義 (USB, UART, Wi-Fi) and the same update runs over the other medium, with
its own frame size, speed and behaviour while the 親機 reboots (USB CDC
and Wi-Fi reconnect; a UART adapter stays).
