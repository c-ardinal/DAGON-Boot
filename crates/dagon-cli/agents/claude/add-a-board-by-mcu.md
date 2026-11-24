---
name: add-a-board-by-mcu
description: Add a node on a board that is not in DAGON's board table, chosen by its MCU, writing the board's definition in dagon.toml from the board's documents. Use when the developer wants to use their own or an unlisted board.
argument-hint: <node id> <mcu> [board document URL or text]
---

# Add a board chosen by MCU

Arguments: `$ARGUMENTS` (node id, MCU id, and the board's documents if
given).

## What DAGON needs

- The MCU ids, and each MCU's flash maps with their sizes, are in the
  MCU definitions of the DAGON this project builds with (the MCU and
  board definitions page of its documentation, Reference); this file
  copies none of them. The board's `flash_bytes` must hold one of its
  MCU's flash maps, and `dagon topo check` reports a board that does not
  and an MCU id DAGON does not know. A board with too little flash
  cannot be used; say so instead of continuing.
- A board definition in `dagon.toml`, `[board.<board id>]`: the board
  schema without `id` (the table name is the board id, never a node id).
  It needs `product`, `mcu`, `flash_bytes` and `drawing`; an I2C node's
  board also gives `i2c_target = true` and its `i2c` pairs. The fields are
  on the MCU and board definitions page of DAGON's documentation
  (Reference).
- A board that is a listed board with a few differences (another pin
  assignment, another flash part) needs no whole definition: write only
  the fields that differ in `[board_override.<listed board id>]` of
  `dagon.toml`, and name the listed board in the node's `board:`. It
  applies to every node on that board; when only some nodes differ, use a
  new `[board.<board id>]` instead. The rules are on the MCU and board
  definitions page (Reference, Project definitions).
- The board's clock input, when a crystal or an oscillator drives it:
  `clock = { source = { crystal_hz = <Hz> } }` (or `external_hz`), from the
  schematic or the datasheet. Left out, the node runs on its MCU's
  internal oscillator; an RP2040 then has no USB clock, so a coordinator on it
  does not build.
- The node in `topology.yaml` with `mcu:` and `board: <board id>`, its
  `role` (`coordinator` or `node`), `port`
  (the `port` of its MCU definition, `self-boot` or `vendor-stage2`), a `tag`, and a link from
  the node nearer the PC. Copy the shape of an existing node in the file.
- On the link, each end's `port`: a `[[link_port]]` name of that node's MCU.
  For a node linked over I2C: its end's address and `node_pins` (SDA and SCL on the
  node's MCU). For a node linked over UART: the baud rate and the pins at the end
  nearer the PC.

## Steps

1. **Collect facts** from the documents the developer gave: flash size,
   the pins wired to the node nearer the PC, reset and boot lines if wired, and the
   crystal on the MCU's clock input if there is one. Write down the
   document and page for each value.
2. **Ask, don't guess.** Everything not in a document is asked in one
   batched question. Never fill in a plausible-looking pin or size.
3. **Edit** `dagon.toml` (add the board definition) and `topology.yaml`
   (the node naming that board, and its link; add the node to
   `update.order`):

   ```toml
   [board.<board id>]
   product = "<product name>"
   mcu = "<mcu id>"
   flash_bytes = <bytes>
   drawing = "<drawing>"
   # Only with a crystal (or an external clock) on the MCU's clock input:
   clock = { source = { crystal_hz = <Hz> } }
   ```

4. **Check.** Run `dagon topo check topology.yaml --json` and fix until
   it reports 0 errors (use the explain-a-topology-problem skill for
   errors you cannot place). Do not edit the checker.
5. **Generate and build.** Run `dagon gen <id>` (it writes the board's
   `flash_bytes` into the node's `wiring.rs`), then `dagon build <id>`.
6. **Report** each value with its source, the check result and the build
   result. Say plainly that the wiring itself is not verified until the
   developer runs `dagon status` with the hardware connected.
