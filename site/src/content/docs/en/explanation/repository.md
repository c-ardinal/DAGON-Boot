---
title: "The repository"
description: "What the repository holds, where, and how to work on DAGON itself."
---

What the code and its tests show:

- Formats and protocol: `dagon-format` and `dagon-proto`, checked against the
  golden vectors in `crates/dagon-format/golden/`.
- The node side: `dagon-agent` (receives an image into the inactive slot,
  checks signature and SHA-256, switches and confirms) and `dagon-core` (the
  boot decision: trial with a bounded number of attempts, then rollback).
- The PC side: `dagon-host` (status, discover, topology push, apply, rollback,
  confirm, recover), `dagon-romlink` (the STM32 I2C ROM loader procedure) and
  the `dagon` CLI, tested against `dagon-sim`, a simulated hub with an F401
  node, flash with power-cut points, lossy links and the ROM loader.
- Projects: `dagon new`, `dagon gen` and `dagon build` make and build the
  node crates on the support packages per MCU (`targets/mcu/*`) and per
  board (`targets/boards/*`).
- `dagon conform`: the 12 conformance tests of `dagon-conform`; 12 of 12 pass
  against the simulated F401, through the hub and directly.
- Topology YAML: 22 validation rules and the supported-board table
  (`dagon-topology`); `topology/sample.yaml` passes them.
- DAGON Console (`web/`, served by `dagon serve`; `web/README.md` lists what
  it does).
- Firmware: the RP2040 hub (`targets/rp2040-hub`), the F401 demo node
  (`samples/f401-app`) and the ESP32-S3 node (`targets/boards/m5stopwatch`,
  `samples/esp32s3-app`). Limitations says which of them have been checked
  on the hardware.


## Repository layout


| Path | Contents |
|---|---|
| `crates/dagon-format` | Image header, manifest, release bundle, state log, topology binary; golden vectors |
| `crates/dagon-proto` | Link protocol v1: frames, commands, payloads |
| `crates/dagon-port` | Board traits (flash, link, reset, watchdog, boot select, I2C, sub MCU lines) |
| `crates/dagon-core` | Boot decision (trial, attempts, rollback) |
| `crates/dagon-agent` | Node-side receiver of writes |
| `crates/dagon-conform` | The 12 conformance tests and the result file format |
| `crates/dagon-sim` | Simulator: flash, links, ROM loader, nodes, hub |
| `crates/dagon-topology` | Topology YAML, validation rules, supported boards |
| `crates/dagon-sign` | Keys, image signing, release build, first-install files |
| `crates/dagon-host` | PC-side procedures shared by the CLI and DAGON Console |
| `crates/dagon-romlink` | STM32 I2C ROM loader procedure |
| `crates/dagon-cli` | The `dagon` command, with DAGON Console built in |
| `crates/dagon-wasm` | WebAssembly bindings for DAGON Console |
| `targets/` | Support packages per MCU (`targets/mcu/*`) and per board (`targets/boards/*`); RP2040 hub firmware; bring-up tools |
| `samples/` | Sample apps built on the support packages (not part of DAGON) |
| `web/` | DAGON Console (Svelte, Vite, TypeScript) |
| `topology/` | Example topologies |
| `plugin/` | Claude Code plugin (agents, skills, hooks) used to port DAGON to new MCUs |
| `scripts/licences.py` | Generates the dependency licence tables below |


## Working on DAGON itself


- Everything in [Prerequisites](/en/getting-started/install/), plus the
  `wasm32-unknown-unknown` target (all targets are in `rust-toolchain.toml`)
- `wasm-pack` 0.15.1 for `crates/dagon-wasm` (it fetches the matching
  `wasm-bindgen` and `wasm-opt` itself), and Node.js and npm for `web/`
- ESP-IDF v6.1 to rebuild the ESP32-S3 2nd stage from
  `targets/esp32s3-2ndstage` (bundled prebuilt as
  `targets/mcu/esp32s3/vendor/bootloader.bin`, unmodified ESP-IDF v6.1;
  only needed to rebuild it, see `vendor/PROVENANCE.md`)
- `elf2uf2-rs` for the RP2040 bring-up image
- `cargo install --path crates/dagon-cli` installs the CLI from your clone;
  projects then use the clone with `dagon new --dagon-path <clone>` (or
  `DAGON_HOME`)
- Release archives: `python scripts/release.py [--target <triple>]` builds
  `out/release/dagon-cli-<version>-<target>.*` with the page built in;
  `.github/workflows/release.yml` does the same for each platform when a
  `cli-v*` tag is pushed, after `scripts/bump_check.py` confirms the
  version is at least what the commits require, and keeps them as workflow
  artifacts (it publishes nothing). Versioning rules: [Versions and releases](/en/explanation/versioning/)
