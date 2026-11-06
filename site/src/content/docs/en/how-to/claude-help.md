---
title: "Claude help in your project"
description: "The Claude Code instructions and skills a new project comes with, and the plugin used to port DAGON."
---

`dagon new` also writes Claude Code files into the project. Claude Code opened in the project folder starts out knowing the layout, the commands and the rules.

| File | What it is for |
|---|---|
| `CLAUDE.md` | The project layout, which files are yours and which are generated, the commands, and the rules (keys stay out, hardware facts are asked, not guessed) |
| `.claude/skills/fix-a-failing-build/SKILL.md` | A `dagon build` fails: find the cause, fix only your code in `nodes/<id>/src`, build again |
| `.claude/skills/add-a-board-by-mcu/SKILL.md` | Add a board that is not in the list, chosen by its MCU, with each value taken from the board's documents |
| `.claude/skills/explain-a-topology-problem/SKILL.md` | Explain a problem the topology check reports, and propose the smallest fix |

The files hold no keys and no paths of your PC.

Signing keys and the Claude API key stay with `dagon`. Claude is told never to open them. Signing and writing to hardware are left to you.

## Porting to a new MCU

In this repository, `plugin/` is the Claude Code plugin used to port DAGON to a new MCU (`/dagon-port`). The port is checked by the conformance tests (`dagon conform`).
