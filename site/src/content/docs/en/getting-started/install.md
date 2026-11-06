---
title: "Install"
description: "The tools DAGON needs and the dagon command."
---

| Tool | When | How |
|---|---|---|
| Rust (`rustup`) | always | <https://rustup.rs> |
| A C compiler | building the `dagon` CLI from source (the `ring` crate) | MSVC Build Tools on Windows, `cc` elsewhere |
| Chrome or Edge 89+ | DAGON Console (Web Serial) | — |

The tools each MCU needs are in [Tools by MCU](/en/reference/tools/); `dagon doctor` checks them.


Install the `dagon` CLI, one of:

- Download the archive for your system (`dagon-cli-<version>-<target>.zip` for Windows,
  `dagon-cli-<version>-<target>.tar.gz` otherwise) from the GitHub release `cli-v<version>`
  you want, check it against its `.sha256` file, and put `dagon` on your
  `PATH`.
- `cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`
  (fetches that same archive; needs `cargo-binstall`).
- Build it: `cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli`.

No clone of this repository and no git submodule is needed:

```sh
dagon --help
```

A project made by `dagon new` takes DAGON from the same release as your
`dagon` (`[dagon] git` and `tag = "cli-v<version>"` in `dagon.toml`). cargo
fetches it when you build, and `dagon flash` builds the boot loader, stage0
and factory agent from cargo's copy of that tag, so it needs the network
the first time. Working on DAGON itself, keep a clone instead:
`dagon new my-fleet --dagon-path <clone>` (or set `DAGON_HOME`) writes
`[dagon] path`.

Then let `dagon` check the tools your topology needs (run it in a project
folder; elsewhere it checks every MCU family):

```sh
dagon doctor           # what is there and what is missing
dagon doctor --fix     # installs what needs no administrator rights
```

For an STM32F401RE node, `dagon doctor` also checks that an ST-LINK is
plugged in (`probe-rs list`); that one it cannot fix for you.

DAGON Console is built into `dagon`; Node.js is only needed to work on
DAGON Console itself (`web/README.md`).
