---
title: "Your own board (chosen by MCU)"
description: "Use a board that is not in the list by writing its board definition."
---

A board in the [Boards and MCUs](/en/reference/boards/) list only needs choosing. Its definition holds its pins, default link and extras.

For a board that is not in the list, choose the MCU on it and write the board's definition yourself.

Write the board definition in `dagon.toml`. The table name `[board.<board id>]` is the board's id. Its fields are those of a board definition in [MCU and board definitions](/en/reference/definitions/), without `id`.

```toml
[board.my-f401]
format = 3
product = "My F401 board"
mcu = "stm32f401re"
flash_bytes = 524288
i2c_target = true
drawing = "nucleo-64"
i2c = [{ controller = "i2c3", sda = "PB4", scl = "PA8" }]
```

Name that board id in the node's `board:` in `topology.yaml`:

```yaml
  - id: seg
    mcu: stm32f401re
    board: my-f401
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

`dagon gen` makes this node from its MCU package and writes the board's `flash_bytes` into `wiring.rs`.

The link's end at the child names a `[[link_port]]` of its MCU (`port`); for an I2C child that end also gives its address (`addr`) and pins (`node_pins`) ([Topology links](/en/reference/topology/)).

The check and `dagon gen` tell you what is still missing.

To go through it with Claude Code, use the `add-a-board-by-mcu` skill.

## A board that differs from a listed one in part

For a board that differs from a listed one in part (a revision with other pin assignments, for example), write only the fields that differ in `[board_override.<board id>]` of `dagon.toml` instead of copying the board's definition. Fields not written stay as the listed board's definition has them. An override is for the whole project: it applies to every node on that board. When only some nodes differ, define a board under a new id with `[board.<board id>]`.

Example: a revision of the Waveshare RP2040 Touch LCD 1.28 whose display and touch controller reset pins are swapped. Write two `[[board_override."waveshare-rp2040-touch-lcd-1.28".pin]]` entries, one with `pin = "GPIO22"` and `name = "lcd_rst"`, the other with `pin = "GPIO13"` and `name = "touch_rst"`. Each `[[pin]]` entry is found by its `pin`, and only the `name` written changes.

`dagon gen` writes the board definition with the override applied to `boot/<id>/board.toml` for each node; the board package and the boot loader build from that file. After changing an override, run `dagon gen` again. An MCU definition is overridden the same way with `[mcu_override.<mcu id>]`. The merge rules, removing (`dagon-remove`), unsetting (`dagon-unset`), order (`dagon-order`) and the fields that may not change are under Project definitions in [MCU and board definitions](/en/reference/definitions/).
