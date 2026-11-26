---
title: "Topology links"
description: "The links of topology.yaml: two ends join two nodes, with no direction."
---

The `links` of a topology (`topology.yaml`, `dagon: 2`) list the links that
join two nodes. A link has no direction: the two `ends` weigh the same, and
the check, what a write reaches and the compiled topology are the same
whichever end is written first. Which node reaches which follows from the node
the PC is connected to.

An end (each item of `ends`) names its node and a `[[link_port]]` name of
that node's MCU definition ([MCU and board definitions](/en/reference/definitions/)),
of the link's `kind` (V-27). On a kind with addresses (I2C), an end's `addr` is
the address the other end reaches that node at. A per-node pin (`node_pins`,
`pins`, `nrst`, `boot`) is written on the end of the node that has it. The
compiled topology holds, per link, both ends' node, port index and address.
`buses` stays for the wiring checks and does not name a link to firmware or
host.

A link's `kind`, its `control.rom_entry` and a coordinator's `uplink.host` are
strings. A name this DAGON does not have is refused by name with the names it
has (V-01: ``this DAGON does not know the link kind `lora` ``), so a 構成
written for a DAGON with more kinds is told apart from a malformed one. A new
name changes no format number: `dagon: 2` stays. The names of `control.rom_entry` are not listed in the code: the MCU and
board definitions give them (`[rom_i2c] entry`, `[[rom_entry]]`), so a
definition alone adds one.

```yaml
links:
  - kind: i2c
    bus: i2c-main
    ends:
      - { node: hub, port: i2c0, nrst: 18, boot: 28 }
      - { node: seg, port: i2c3, addr: 0x42, node_pins: { sda: PB4, scl: PA8 } }
    control:
      kind: full
      rom_entry: stm32-i2c-rom
```

## Link

<!-- generated from dagon-topology's Link: python site/scripts/gen.py -->
| Key | Type | Required | Meaning |
|---|---|---|---|
| `kind` | string | yes | The medium: `i2c`, `uart`, `can`, `ethernet`, `usb-cdc`, `ble`, `wifi-tcp`. |
| `ends` | array of two ends | yes | The two nodes it joins, each with the port it uses there (V-04, V-27). |
| `bus` | string | no | An I2C link's bus: an `id` of `buses` (V-01). |
| `iface` | string | no | A UART link's UART: `hw-uart0` or `pio-uart` (V-01). |
| `baud` | integer | no | A UART link's bit rate; 115200 when left out (V-26). |
| `control` | table | no | How a node is put into its ROM loader over this link: `kind` (`full`, `cooperative`) and `rom_entry` (a name a MCU or board definition gives a ROM entry). An I2C or UART link needs `rom_entry` when it is cooperative, the default (V-11). |
<!-- end of generated table -->

## End

<!-- generated from dagon-topology's End: python site/scripts/gen.py -->
| Key | Type | Required | Meaning |
|---|---|---|---|
| `node` | string | yes | A node `id`. |
| `port` | string | yes | A `[[link_port]]` name of the node's MCU definition, of the link's kind (V-27). |
| `addr` | integer | no | The 7-bit address (0x08 to 0x77) the other end reaches this node at, on a kind with addresses (I2C); refused on any other kind (V-18). No two ends on one bus share one, and none takes the address of a ROM loader reached over that bus (V-18). |
| `node_pins` | table | no | This node's I2C pins by MCU pin name (`{ sda: PB4, scl: PA8 }`), on an end with `addr` (V-01): one of its board's I2C pairs (V-07). |
| `pins` | table | no | This node's UART pins as GPIO numbers (`{ tx: 26, rx: 27 }`); a UART link names them on at least one end, and a link of any other kind on none (V-01). |
| `nrst` | integer | no | This node's GPIO driving the other node's reset, for `control.kind: full`. |
| `boot` | integer | no | This node's GPIO driving the other node's boot pin, for `control.kind: full`. A relay tells its recovery targets apart by port and loader address, one per ROM recovery link (`rom_entry` naming a ROM I2C loader). A target that gives `nrst` and `boot` uses only lines its board wires on those GPIOs. |
<!-- end of generated table -->
