---
title: Reference
description: Generated from DAGON's own sources; each page names its source.
sidebar:
  order: 0
---

Every page in this part is generated from the source DAGON itself reads,
and the repository's checks fail when a page and its source differ.

- [Serve HTTP API](/reference/serve-api/): every route of `dagon serve`.
- [Events](/reference/events/) and [Ports](/reference/ports/): every
  event and port extensions use.
- [Agent definition format](/reference/agent-definition/): the agent files
  DAGON reads.
- [Boards and MCUs](/reference/boards/): every board and MCU definition.
- [Contracts and components](/reference/contracts/): every format number and
  component version, from `compat.toml`.
- [Tools by MCU](/reference/tools/): what `dagon doctor` checks for each MCU.
- [Rust API](/reference/rust-api/): the workspace crates and their rustdoc.

Written by hand, with their lists generated or checked by tests:

- [Agent link: serve API](/reference/agent-api/): linking DAGON's agent, the
  questions and the keys.
- [Extension manifest](/reference/extension-manifest/).
- [Licenses](/reference/licenses/).
