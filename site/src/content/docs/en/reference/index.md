---
title: Reference
description: Generated from DAGON's own sources; each page names its source.
sidebar:
  order: 0
---

Every page in this part is generated from the source DAGON itself reads,
and the repository's checks fail when a page and its source differ.

- [Command line](/en/reference/cli/): every `dagon` command, from its clap definitions.
- [Serve HTTP API](/en/reference/serve-api/): every route of `dagon serve`.
- [Events](/en/reference/events/) and [Ports](/en/reference/ports/): every
  event and port extensions use.
- [Agent definition format](/en/reference/agent-definition/): the agent files
  DAGON reads.
- [Boards and MCUs](/en/reference/boards/): every board and MCU definition.
- [Contracts and components](/en/reference/contracts/): every format number and
  component version, from `compat.toml`.
- [Tools by MCU](/en/reference/tools/): what `dagon doctor` checks for each MCU.
- [Rust API](/en/reference/rust-api/): the workspace crates and their rustdoc.

Written by hand, with their lists generated or checked by tests:

- [Agent link: serve API](/en/reference/agent-api/): linking DAGON's agent, the
  questions and the keys.
- [Extension manifest](/en/reference/extension-manifest/) and
  [MCU and board definitions](/en/reference/definitions/), their key tables
  generated from the types DAGON reads the files into.
- [Licenses](/en/reference/licenses/).
