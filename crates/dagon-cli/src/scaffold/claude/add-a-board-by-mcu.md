---
name: add-a-board-by-mcu
description: Add a node on a board that is not in DAGON's board table, chosen by its MCU (rp2040, stm32f401re or esp32s3), filling the board facts from the board's documents. Use when the developer wants to use their own or an unlisted board.
argument-hint: <node id> <mcu> [board document URL or text]
---

# Add a board chosen by MCU

Arguments: `$ARGUMENTS` (node id, MCU id, and the board's documents if
given).

## What DAGON needs

- MCU ids and the smallest flash each layout needs: `rp2040` (4 MiB),
  `stm32f401re` (512 KiB), `esp32s3` (16 MiB). A board with less flash
  cannot be used; say so instead of continuing.
- The node in `topology.yaml` with `mcu:` and no `board:`, its `role`
  (`coordinator` for the 親機, `node` for a 子機), `port` (`self-boot`;
  `vendor-stage2` for `esp32s3`), a `tag`, and a link from the 親機. Copy
  the shape of an existing node in the file.
- For an I2C 子機: the address and `node_pins` (SDA and SCL on the node's
  MCU). For a UART 子機: the baud rate and the 親機's pins.
- `flash_bytes` for the node in `[board.<id>]` of `dagon.toml`.

## Steps

1. **Collect facts** from the documents the developer gave: flash size,
   the pins wired to the 親機, reset and boot lines if wired. Write down
   the document and page for each value.
2. **Ask, don't guess.** Everything not in a document is asked in one
   batched question. Never fill in a plausible-looking pin or size.
3. **Edit** `topology.yaml` (the node and its link; add the node to
   `update.order`) and add to `dagon.toml`:

   ```toml
   [board.<id>]
   flash_bytes = <bytes>
   ```

4. **Check.** Run `dagon topo check topology.yaml --json` and fix until
   it reports 0 errors (use the explain-a-topology-problem skill for
   errors you cannot place). Do not edit the checker.
5. **Generate and build.** Run `dagon gen <id>` (it notes a missing
   `flash_bytes`), then `dagon build <id>`.
6. **Report** each value with its source, the check result and the build
   result. Say plainly that the wiring itself is not verified until the
   developer runs `dagon status` with the hardware connected.
