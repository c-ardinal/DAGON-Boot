---
title: "Install"
description: "The tools DAGON needs and the dagon command."
---

## The tools you need

| Tool | When | How |
|---|---|---|
| Rust (`rustup`) | always | <https://rustup.rs> |
| A C compiler | building the `dagon` command from source (the `ring` crate) | MSVC Build Tools on Windows, `cc` elsewhere |
| Chrome or Edge 89+ | DAGON Console (Web Serial) | — |

The tools each MCU needs are in [Tools by MCU](/en/reference/tools/).

## Install the `dagon` command

Use one of these:

- Prebuilt files: from the GitHub release `cli-v<version>` you want, download the archive for your system (`dagon-cli-<version>-<target>.zip` for Windows, `dagon-cli-<version>-<target>.tar.gz` otherwise). Check it against its `.sha256` file, and put `dagon` on your `PATH`.
- `cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`: fetches that same archive, and needs `cargo-binstall`.
- Build from source: `cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli`

No clone of this repository and no git submodule is needed.

Check that it is there:

```sh
dagon --help
```

DAGON Console is built into `dagon`. Node.js is only needed to work on DAGON Console itself (`web/README.md`).

## Check the tools

Let `dagon` check for missing tools:

```sh
dagon doctor           # what is there and what is missing
dagon doctor --fix     # installs what needs no administrator rights
```

In a project folder it checks what that topology needs. Elsewhere it checks what every MCU needs.

For an STM32F401RE node, `dagon doctor` also checks that an ST-LINK is plugged in (`probe-rs list`). `--fix` cannot fix that one.

## When the network is needed

A project made by `dagon new` takes DAGON from the same release as your `dagon` (`[dagon] git` and `tag = "cli-v<version>"` in `dagon.toml`).

- cargo fetches it when you build.
- `dagon flash` builds the boot loader, stage0 and factory agent from cargo's copy of that tag.

So the first time needs the network.

To work on DAGON itself, see [The repository](/en/explanation/repository/).
