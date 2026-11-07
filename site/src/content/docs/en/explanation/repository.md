---
title: "The repository"
description: "What the repository holds, where, and how to work on DAGON itself."
---

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | DAGON's crates, listed in [Rust API](/en/reference/rust-api/) |
| `targets/` | Definitions per MCU (`targets/mcu/*`) and per board (`targets/boards/*`); RP2040 hub firmware |
| `samples/` | Sample apps built on the definitions (not part of DAGON) |
| `web/` | DAGON Console (Svelte, Vite, TypeScript); `web/README.md` lists what it does |
| `topology/` | Example topologies |
| `plugin/` | Claude Code plugin (agents, skills, hooks) used to port DAGON to new MCUs |
| `scripts/licences.py` | Generates the dependency tables in [Licenses](/en/reference/licenses/) |

## Working on DAGON itself

### What you need

- All the tools in [Install](/en/getting-started/install/)
- The `wasm32-unknown-unknown` target (all targets are in `rust-toolchain.toml`)
- `wasm-pack` 0.15.1 for `crates/dagon-wasm` (it fetches the matching `wasm-bindgen` and `wasm-opt` itself)
- Node.js and npm for `web/`
- ESP-IDF v6.1 to rebuild the ESP32-S3 2nd stage from `targets/esp32s3-2ndstage`. An unmodified prebuilt one is bundled as `targets/mcu/esp32s3/vendor/bootloader.bin` (see `vendor/PROVENANCE.md`).

### Using a clone

`cargo install --path crates/dagon-cli` installs the `dagon` command from your clone.

Projects use the clone with `dagon new --dagon-path <clone>` (or by setting `DAGON_HOME`), which writes `[dagon] path` in `dagon.toml`.

### Building the release archives

`python scripts/release.py [--target <triple>]` builds `out/release/dagon-cli-<version>-<target>.*` with the page built in.

`.github/workflows/release.yml` does the same for each platform when a `cli-v*` tag is pushed:

1. `scripts/bump_check.py` confirms the version is at least what the commits require.
2. It builds each system's archive and its `.sha256`.
3. Once every build has passed, it publishes a GitHub release carrying each system's archive and its `.sha256`.

A failed build publishes nothing, and neither does a manual run.

The versioning rules are in [Versions and releases](/en/explanation/versioning/).
