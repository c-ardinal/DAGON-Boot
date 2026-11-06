---
title: "Your own board (chosen by MCU)"
description: "Use a board that is not in the list by choosing its MCU."
---

**Choose by board or by MCU.** A board from the table brings its pins,
default link and extras; you only pick it. Any other board with one of
these MCUs can be chosen by its MCU: you give its flash size and the pins
wired to the hub (see [Your own board](/en/reference/boards/)).


Leave out `board:` for the node in `topology.yaml` and give its facts in
`dagon.toml`:

```yaml
  - id: seg
    mcu: stm32f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

```toml
[board.seg]
flash_bytes = 524288
```

For an I2C child, the link names the pins on the node (`node_pins`). The
check and `dagon gen` tell you what is still missing; the
`add-a-board-by-mcu` skill walks through it with you.
