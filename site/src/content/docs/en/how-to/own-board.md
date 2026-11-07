---
title: "Your own board (chosen by MCU)"
description: "Use a board that is not in the list by choosing its MCU."
---

A board in the [Boards and MCUs](/en/reference/boards/) list only needs choosing. Its definition holds its pins, default link and extras.

Choose a board that is not in the list by its MCU. You give its flash size and the pins wired to the hub.

Leave out `board:` for the node in `topology.yaml`:

```yaml
  - id: seg
    mcu: stm32f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

Give the board's values in `dagon.toml`:

```toml
[board.seg]
flash_bytes = 524288
```

For an I2C child, the link names the pins on the node (`node_pins`).

The check and `dagon gen` tell you what is still missing.

To go through it with Claude Code, use the `add-a-board-by-mcu` skill.
