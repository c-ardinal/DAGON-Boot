---
title: "Install"
description: "Installing the dagon command from a release, and building it yourself."
---

## From a release

Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.ps1 | iex
```

The script does this:

1. It takes the archive for your system from the latest release (`cli-v<version>`).
2. It checks it against its `.sha256` file, and stops if they differ.
3. It puts `dagon` in a folder that needs no administrator rights: `~/.local/bin` on Linux and macOS, `%LOCALAPPDATA%\Programs\dagon` on Windows.
4. On Windows it adds that folder to your user's PATH. On Linux and macOS it says how.
5. It prints what it did.

The script reads these environment variables:

| Variable | Meaning |
|---|---|
| `DAGON_VERSION` | The version to install. `install.sh` also takes it as an argument (`sh -s -- <version>`) |
| `DAGON_INSTALL_DIR` | The folder to put it in |
| `DAGON_NO_MODIFY_PATH` | With `install.ps1`, leaves PATH as it is and says how to add the folder |

`cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot` takes the same archive, and needs `cargo-binstall`.

No clone of this repository and no git submodule is needed.

The browser for DAGON Console is Chrome or Edge 89+ (Web Serial).

Check that it is there:

```sh
dagon --help
```

DAGON Console is built into `dagon`. Node.js is not needed.

## Build it yourself

These are the steps to build the `dagon` command from source, whether or not you change DAGON.

| What | How |
|---|---|
| Rust 1.95.0 or later | <https://rustup.rs> |
| A C compiler (the `ring` crate) | the Visual Studio 2022 C++ build tools on Windows, gcc 13 or later on Linux, Xcode Command Line Tools 16.4 or later on macOS |

```sh
cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli
```

From a clone, it is `cargo install --path crates/dagon-cli`.

## The tools for developing firmware

Developing firmware needs [Rust](https://rustup.rs) and the [tools each MCU needs](/en/reference/tools/). Let `dagon` check for missing ones:

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
