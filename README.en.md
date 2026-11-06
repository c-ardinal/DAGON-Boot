<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/product-symbol/lockup-horizontal-dark-2400w.png">
    <img src="assets/product-symbol/lockup-horizontal-light-2400w.png" alt="DAGON Boot" width="480">
  </picture>
</p>

<p align="center"><b>The distributed microcontroller designs of professionals, made easy for many. However many nodes and however deep inside, writing is the same operation as one node, and safe.</b></p>

<p align="center">
  <a href="https://c-ardinal.github.io/DAGON-Boot/demo/">Try the demo</a> ·
  <a href="https://c-ardinal.github.io/DAGON-Boot/doc/en/">Documentation</a> ·
  <a href="README.md">日本語</a> ·
  License: MIT or Apache-2.0
</p>

> [!IMPORTANT]
> As of November 8, 2026, this is an alpha that supports only three kinds of microcontroller, made for "Built with Claude: Japan 2026". Please understand that breaking changes may be made before the official release.

![DAGON Console writing a new version to the 7-segment node (the Console's screens are in Japanese)](assets/console/console-writing.png)

## What is DAGON?

A beautiful design with a muddy operation. Even a compact build that splits its work across microcontrollers means taking it apart, swapping cables and using one write tool per MCU every time a program changes. Sometimes the design itself is compromised because writing would be hell.

DAGON makes the distributed multi-microcontroller designs of professionals and specialists easy for a wide range of people, from those starting electronics with AI to makers who care for compactness and finish.

Before assembly, you write each node directly once. After that no write pattern is touched and nothing is opened. Through one microcontroller on your PC's USB (the hub, 親機), you write to the others (the children, 子機), however deep inside and however many, with the same operation as one node. More nodes take more time; the operation does not grow.

Writing is safe. Every image is signed, a node runs a new version on trial, and it goes back to the previous one by itself when the new one does not work. If a write stops part-way, the running version stays.

What sets DAGON apart is handling mixed MCUs by drawing the 構成 (the setup). As a developer you choose the boards, draw how they connect and write your application in Rust. Applications are Rust only. DAGON takes care of start-up, flash layout, signing and the writing steps.

## Features

- **Draw the setup**: DAGON Console, in your browser, shows the nodes and their links as a graph and checks wiring and pins as you edit.
- **Get a project that builds**: `dagon new` and `dagon gen` give every node a Rust crate with its start-up, its wiring and an empty `app.rs` for your code.
- **Write in one go**: `dagon deploy` builds, signs, packages and writes in one command. In the Console you write with each node's progress in view.
- **Go back when it fails**: every image is signed. A node runs a new version on trial and goes back when it cannot be confirmed; `dagon rollback` goes back by hand.
- **Update the boot core too**: the boot core itself lives in two slots and is updated like an app (RP2040, STM32F401), going back if the new one fails.
- **Extend it**: without changing DAGON's source, add extensions that receive events, answer decisions and replace parts (ports).
- **Ask an AI**: a new project comes with Claude Code instructions and skills, and `dagon ai link` connects an agent with your consent. The API key stays in the OS credential store.

Every step is also a `dagon` command.

## Supported hardware

| MCU | Boards with a definition |
|---|---|
| RP2040 | Waveshare RP2040-Touch-LCD-1.28 |
| STM32F401RE | NUCLEO-F401RE |
| ESP32-S3 | M5StopWatch |

The hub is an RP2040. Other boards work by choosing their MCU ([your own board](https://c-ardinal.github.io/DAGON-Boot/doc/en/how-to/own-board/)). The full list is in [Boards and MCUs](https://c-ardinal.github.io/DAGON-Boot/doc/en/reference/boards/).

## Try it

In your browser, try the [DAGON Console demo](https://c-ardinal.github.io/DAGON-Boot/demo/). It needs no install and no hardware: you write to a simulated hub and its children and go back to a previous version. File input and output, device connection and API keys are not available in the demo.

On your own PC, also without hardware:

```sh
cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v0.1.0 dagon-cli
dagon demo
```

`dagon demo` opens the Console at the address it prints. `dagon sim run` runs end-to-end scenarios in the simulator.

## Prerequisites

- [Rust](https://rustup.rs): applications are written in Rust only (and a C compiler when building from source)
- Chrome or Edge 89 or later to connect real boards (it uses Web Serial)
- `dagon doctor` checks the tools each MCU needs ([list](https://c-ardinal.github.io/DAGON-Boot/doc/en/reference/tools/))

Installing from the release archives is in [Install](https://c-ardinal.github.io/DAGON-Boot/doc/en/getting-started/install/).

## Start your own project

```sh
dagon new my-fleet
cd my-fleet
dagon doctor
dagon serve
```

`dagon new` makes `dagon.toml`, the topology file (the three-node sample) and a signing key pair, with the secret key outside the project. Open the address `dagon serve` prints to go on in the Console. From building to the first write, see [From a new project to an update](https://c-ardinal.github.io/DAGON-Boot/doc/en/getting-started/first-project/).

You can also add DAGON to existing firmware ([how](https://c-ardinal.github.io/DAGON-Boot/doc/en/how-to/existing-firmware/)).

## How it works

```mermaid
flowchart LR
  PC["PC<br>dagon / DAGON Console"] -- USB --> HUB["Hub<br>RP2040"]
  HUB -- I2C --> SEG["Child<br>STM32F401RE"]
  HUB -- UART --> WATCH["Child<br>ESP32-S3"]
```

1. The PC checks the signed package's signatures and topology, and sends it to the hub.
2. The hub relays to the children. Children are written first and the hub last.
3. A node writes the new image beside the running one and starts it on trial.
4. When its self-check passes it confirms; otherwise a watchdog reset takes it back to the previous version.

> [!WARNING]
> What has been checked on hardware is in [Limitations](https://c-ardinal.github.io/DAGON-Boot/doc/en/explanation/limitations/). Boot core updates are unproven on hardware: the last hardware run came before the write flow changed, and the current scenario set has not been run since. Power cuts and writing the F401 through its ROM have not been run on hardware either. Signing keys are files on the PC; there is no hardware root of trust.

## Dependencies and external data

- Written for this project: `crates/`, `targets/` (except the vendor parts below), `web/`, `site/`, `samples/`, `plugin/`, `topology/`, `scripts/`
- Libraries: Rust crates and npm packages, used unmodified (embassy, esp-hal, ed25519-compact, Svelte, Vite, Astro and others)
- Vendor parts: the ESP-IDF 2nd stage, `rp2040-boot2`, the STM32F401 ROM boot loader
- Fonts and icons: Doto, DotGothic16, Zen Kaku Gothic New, JetBrains Mono (SIL OFL 1.1), Lucide (ISC)
- Values in the MCU and board definitions: taken from the makers' datasheets and board documents, with the source in comments in the definition files

The full list and licences are in [Licenses](https://c-ardinal.github.io/DAGON-Boot/doc/en/reference/licenses/).

## Documentation

- [Getting started](https://c-ardinal.github.io/DAGON-Boot/doc/en/getting-started/): try it, install, from a new project to an update
- [How-to guides](https://c-ardinal.github.io/DAGON-Boot/doc/en/how-to/): your own board, swapping a link, AI help
- [Explanation](https://c-ardinal.github.io/DAGON-Boot/doc/en/explanation/): words, events and extensions, versioning rules
- [Reference](https://c-ardinal.github.io/DAGON-Boot/doc/en/reference/): commands, HTTP API, events, definition formats, Rust API
