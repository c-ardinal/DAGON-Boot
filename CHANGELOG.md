# Changelog

Each component of DAGON has its own versions (see
[Versions and releases](site/src/content/docs/explanation/versioning.md)). A
section is named after the component and its version, as its release tag is:
`cli-v0.1.0` is the section `cli 0.1.0`.

## cli 0.1.0

The first release of the `dagon` command and DAGON Console. Every file format,
protocol and API starts at format 1; `dagon --version` prints the numbers this
`dagon` reads and writes.

### Features

- **Projects.** `dagon new` starts a project and `dagon gen` gives every node a
  Rust crate with its start-up, wiring and an empty `app.rs`. `dagon build`
  builds each node named in `dagon.toml`.
- **Boards and MCUs as data.** RP2040, STM32F401RE and ESP32-S3 come as
  definition files. A project adds its own board or MCU in the same format, and
  a board that is not listed can be chosen by its MCU.
- **Writing and going back.** Images and releases are signed. `dagon flash`
  writes a node for the first time, and `dagon deploy` builds, signs, packages
  and writes in one go. A node runs a new version on trial and goes back by
  itself when the version is not confirmed. `dagon rollback` goes back by hand,
  and `dagon deploy` also rewrites a node that no longer answers, through its
  ROM loader after asking.
- **DAGON Console.** `dagon serve` opens the browser tool on your own PC: draw
  the 構成, build, and write to the nodes with each node's progress on its card.
- **Try it without boards.** `dagon demo` runs DAGON Console on a simulated 親機
  with two 子機, and `dagon sim run` runs end-to-end scenarios in the simulator.
- **Extensions.** Boards, libraries and applications observe DAGON's events,
  answer its decision points and replace its ports, declared in a manifest that
  `dagon gen` checks.
- **Agents.** `dagon ai link` links DAGON to an agent once, with your consent.
  Agents are definition files, and API keys stay in the OS credential store.

Known limits are in
[Limitations](site/src/content/docs/explanation/limitations.md).
