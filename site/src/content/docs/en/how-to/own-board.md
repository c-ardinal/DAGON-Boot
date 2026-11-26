---
title: "Your own board (chosen by MCU)"
description: "Use a board that is not in the list by writing its board definition."
---

A board in the [Boards and MCUs](/en/reference/boards/) list only needs choosing. Its definition holds its pins, default link and extras.

For a board that is not in the list, choose the MCU on it and write the board's definition yourself.

Write the board definition in `dagon.toml`. The table name `[board.<board id>]` is the board's id. Its fields are those of a board definition in [MCU and board definitions](/en/reference/definitions/), without `id`.

```toml
[board.my-f401]
format = 4
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

## Importing a board definition from Zephyr

When the board is one Zephyr supports, the first version of `[board.<board id>]` can be made from Zephyr's devicetree. Give the script a Zephyr tree checked out at a release tag and the board's `.dts`.

```sh
python3 scripts/import_zephyr_board.py <zephyr tree> <zephyr tree>/boards/raspberrypi/rpi_pico/rpi_pico.dts \
    --id rpi-pico --mcu rp2040 --tag v4.1.0 >> dagon.toml
```

The head of the output records the tag and the SHA-256 of every file read. The same tree gives the same output.

| Taken over | Where Zephyr says it |
|---|---|
| Product name (`product`) | `full_name` in `board.yml` |
| Flash size (`flash_bytes`) | `reg` of `flash0` |
| Header pins and printed names (`header`) | the connector's `gpio-map` and the comment on each row |
| I2C pin pairs (`i2c`) | `pinctrl-0` of the I2C controllers |
| UART buses (`[[bus]]`) | `pinctrl-0` and `current-speed` of the UARTs with `status = "okay"` |
| LEDs and buttons (`[[pin]]`) | `gpios` under `gpio-leds` and `gpio-keys` |

What only DAGON decides is not taken over. You add the default link (`default_link`), the boot LED and its halt pattern (`[boot]`), the clock (`[clock]`) and the bus of DAGON's own link (`dagon = true`). Zephyr does not say where the slots go either. The Raspberry Pi Pico, for example, has 2 MiB of flash and the built-in RP2040 layout (4 MiB) does not fit, so `[mcu_override.rp2040]` shrinks the flash map and the app slots.

The script does not run the C preprocessor: it reads the text of the files as it stands. Pins come from the pin control names (`I2C0_SDA_P4`, `i2c1_scl_pb8`) and the GPIO node names (`gpio0`, `gpioa`). Two forms are read, RP2040 and STM32; a board written another way is refused, not guessed. SPI buses and shield definitions are not read.

Zephyr's files are under Apache-2.0. The output is a list of facts (pin numbers and names), but keep the comment at its head: it is the record of where they came from.

## A board that differs from a listed one in part

For a board that differs from a listed one in part (a revision with other pin assignments, for example), write only the fields that differ in `[board_override.<board id>]` of `dagon.toml` instead of copying the board's definition. Fields not written stay as the listed board's definition has them. An override is for the whole project: it applies to every node on that board. When only some nodes differ, define a board under a new id with `[board.<board id>]`.

Example: a revision of the Waveshare RP2040 Touch LCD 1.28 whose display and touch controller reset pins are swapped. Write two `[[board_override."waveshare-rp2040-touch-lcd-1.28".pin]]` entries, one with `pin = "GPIO22"` and `name = "lcd_rst"`, the other with `pin = "GPIO13"` and `name = "touch_rst"`. Each `[[pin]]` entry is found by its `pin`, and only the `name` written changes.

`dagon gen` writes the board definition with the override applied to `boot/<id>/board.toml` for each node; the board package and the boot loader build from that file. After changing an override, run `dagon gen` again. An MCU definition is overridden the same way with `[mcu_override.<mcu id>]`. The merge rules, removing (`dagon-remove`), unsetting (`dagon-unset`), order (`dagon-order`) and the fields that may not change are under Project definitions in [MCU and board definitions](/en/reference/definitions/).
