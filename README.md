# DAGON Boot (Dual-bank Atomic Gateway for On-device Nodes)

[日本語版 / Japanese: README.ja.md](README.ja.md)

Signed A/B firmware updates with automatic rollback for a small network of
microcontrollers, driven from a PC or a browser through one hub.

## Overview

A hub (RP2040) is connected to the PC over USB and to its nodes over I2C
(STM32F401) or UART (ESP32-S3). The PC verifies a signed release, sends each
node its image through the hub, switches the node to the new slot on trial
and confirms it once the node answers with the new version. A node whose new
image never gets confirmed is reset by its watchdog and boots the previous
slot again. An STM32F401 that no longer answers at all can be rewritten
through the STM32 ROM I2C boot loader, driven by the PC through the hub.

All formats (image header, release manifest, `.dgb` release bundle, state
log, topology binary) and the link protocol are defined in this repository;
the same Rust crates are used by the firmware, the `dagon` command line tool
and the browser page (through WebAssembly).

## What works today

What the code and its tests in this repository show:

- Formats and protocol: `dagon-format` and `dagon-proto`, checked against the
  golden vectors in `crates/dagon-format/golden/`.
- The node side: `dagon-agent` (receives an image into the inactive slot,
  checks signature and SHA-256, switches and confirms) and `dagon-core` (the
  boot decision: trial with a bounded number of attempts, then rollback).
- The PC side: `dagon-host` (status, discover, topology push, apply, rollback,
  confirm, recover), `dagon-romlink` (the STM32 I2C ROM loader procedure) and
  the `dagon` CLI. They are tested against `dagon-sim`, a simulated hub with
  an F401 node, flash with power-cut points, lossy links and the ROM loader.
- `dagon sim run`: four end-to-end scenarios in the simulator: an update with
  a power cut during the transfer, an image that never confirms, tampered
  releases (rejected by the PC, and by the node when host checks are
  skipped), and an erased F401 brought back through the ROM loader.
- `dagon conform`: the 12 conformance tests of `dagon-conform`; 12 of 12 pass
  against the simulated F401, through the hub and directly.
- Topology YAML: 21 validation rules and the supported-board table
  (`dagon-topology`); `topology/desk-demo.yaml` passes them.
- Firmware for the RP2040 hub (`targets/rp2040-stage0`, `rp2040-bootcore`,
  `rp2040-hub`), the F401 node (`targets/f401-bootcore`, `f401-app`) and the
  ESP32-S3 node (`targets/esp32s3-port`, `esp32s3-agent`, with the ESP-IDF
  2nd stage). The hub firmware has been flashed and starts on the hub
  hardware; on-device validation of the hub (host communication, display) is
  still in progress. On-device checks of the F401 and ESP32-S3 firmware and
  the full flow on hardware are still pending (see Limitations).
- The browser page in `web/` (under development; `web/README.md` lists what
  it does today).

## Quick start

Build the CLI (Rust stable, see Development prerequisites):

```sh
cargo build --release -p dagon-cli      # binary: target/release/dagon
```

Try everything in the simulator, no hardware needed:

```sh
dagon sim run
```

Keys (the secret key must stay outside any git work tree; the CLI refuses
one inside):

```sh
dagon key gen --out ~/.dagon/keys
export DAGON_SIGNING_KEY=~/.dagon/keys/dagon-signing.key
export DAGON_PUBKEY=~/.dagon/keys/dagon-signing.pub   # embedded by firmware builds
```

First install. Build the firmware in `targets/` (each directory's README has
the commands), point `[provision.<node>]` in `dagon.toml` at the ELF files,
then:

```sh
dagon rescue hub        # out/provision/hub/rescue.uf2: hold BOOT, plug in USB, copy to RPI-RP2
dagon provision seg     # out/provision/seg/flash.bin and the probe-rs commands for ST-LINK
```

Update. Sign the new app for both slots of each node, build the release
against the topology, and apply it:

```sh
dagon image sign app.elf --node seg --version 1.1.0 --slot a --out out/rel/seg-a.img
dagon image sign app-b.elf --node seg --version 1.1.0 --slot b --out out/rel/seg-b.img
# ... the same for the hub
dagon release build out/rel --name r1.1 --out out/release-1.1.dgb
dagon status
dagon apply out/release-1.1.dgb
dagon recover seg       # if a node no longer answers
```

`dagon apply` refuses a release built for a different topology than the one
in `dagon.toml`. The hub is found by USB ID `c0de:cafe` or the product name
"DAGON hub" (or pass `--port`).

Conformance of a node (images `good-a.img`, `good-b.img`, `no-confirm-a.img`,
`no-confirm-b.img` in `out/conform/<node>`):

```sh
dagon conform --node seg            # writes result.json and report.html
```

Topology from a plain-language board description through the Claude API (the
key is read from the environment and never printed):

```sh
export ANTHROPIC_API_KEY=...
dagon ai topology --in boards.txt --out topology/mine.yaml
```

`dagon --help` lists every command and option.

## Browser UI (development)

See `web/README.md`. In short, with Node.js, npm and `wasm-pack` 0.15.1:

```sh
cd web
npm install
npm run dev       # http://127.0.0.1:5173 (Chrome or Edge 89+, Web Serial)
```

## Adopting DAGON in your firmware

1. Add the crates as git dependencies pinned to a release tag, for example
   `dagon-agent = { git = "<repository URL>", tag = "<release tag>" }`, and
   the same for `dagon-format`, `dagon-proto`, `dagon-port` and, for a boot
   core, `dagon-core`.
2. Lay out the flash like the existing boards: a boot core, two state log
   faces, and slots A and B, each starting with the 512-byte signed image
   header and the body at slot start + 0x200. Link the application for the
   body address of its slot (see `targets/f401-app` and `targets/rp2040-hub`;
   their READMEs list the maps). Add the board to the topology YAML.
3. Implement the `dagon-port` traits for your board (`Flash`, `Link`,
   `Reset`, `Watchdog`, and `BootSelectS` or `BootSelectV`) and a
   `dagon_agent::SlotStore`, then start the agent with your link and hooks:

   ```rust
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, hooks); // hooks: dagon_agent::Hooks
   agent.run(&mut link, &mut reset).await;                    // serves requests forever
   ```

   Feed the watchdog while confirmed, and only for a limited time while on
   trial, so an image that is never confirmed is reset and rolled back.
4. Check the port with `dagon conform`, then `dagon image sign` →
   `dagon release build` → `dagon provision` for the first install and
   `dagon apply` afterwards.

## Repository layout

| Path | Contents |
|---|---|
| `crates/dagon-format` | Image header, manifest, release bundle, state log, topology binary; golden vectors |
| `crates/dagon-proto` | Link protocol v1: frames, commands, payloads |
| `crates/dagon-port` | Board traits (flash, link, reset, watchdog, boot select, I2C, sub MCU lines) |
| `crates/dagon-core` | Boot decision (trial, attempts, rollback) |
| `crates/dagon-agent` | Node-side receiver of updates |
| `crates/dagon-conform` | The 12 conformance tests and the result file format |
| `crates/dagon-sim` | Simulator: flash, links, ROM loader, nodes, hub |
| `crates/dagon-topology` | Topology YAML, validation rules, supported boards |
| `crates/dagon-sign` | Keys, image signing, release build, first-install files |
| `crates/dagon-host` | PC-side procedures shared by the CLI and the browser |
| `crates/dagon-romlink` | STM32 I2C ROM loader procedure |
| `crates/dagon-cli` | The `dagon` command |
| `crates/dagon-wasm` | WebAssembly bindings for the browser page |
| `targets/` | Firmware: RP2040 hub, STM32F401 node, ESP32-S3 node, bring-up tools |
| `web/` | Browser page (Svelte, Vite, TypeScript) |
| `topology/` | Example topologies |
| `plugin/` | Claude Code plugin (agents, skills, hooks) used to port DAGON to new boards |
| `scripts/licences.py` | Generates the dependency licence tables below |

## Development prerequisites

- Rust stable with the targets in `rust-toolchain.toml` (`thumbv6m-none-eabi`,
  `thumbv7em-none-eabihf`, `wasm32-unknown-unknown`)
- `wasm-pack` 0.15.1 for `crates/dagon-wasm` (it fetches the matching
  `wasm-bindgen` and `wasm-opt` itself)
- Node.js and npm for `web/` (see `web/README.md`)
- For the ESP32-S3 firmware: the Xtensa Rust toolchain installed with
  `espup` (`targets/esp32s3-*` use the `esp` channel) and ESP-IDF v6.1 to
  build the 2nd stage in `targets/esp32s3-2ndstage`
- `probe-rs` to flash the F401 over ST-LINK, `elf2uf2-rs` for the RP2040
  bring-up image
- A C compiler: the `ring` crate (TLS for `dagon ai topology`) builds C code

## License (MIT / Apache-2.0 dual)

Everything written for DAGON Boot in this repository is licensed under
either of the [MIT license](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.

## Dependencies and their licenses

Generated from every `Cargo.lock` in the repository and
`web/package-lock.json` by `python scripts/licences.py`.

Points to note:

- `serialport` (MPL-2.0) is used unmodified by the `dagon` CLI. If you
  distribute a `dagon` binary, MPL-2.0 requires that the source of
  `serialport` be available: it is the published crate at
  <https://crates.io/crates/serialport> (repository
  <https://github.com/serialport/serialport-rs>).
- `unescaper` is offered under MIT OR GPL-3.0-only; it is used under MIT.
- `ring` is Apache-2.0 AND ISC (both apply).
- `webpki-roots` is CDLA-Permissive-2.0 (it carries the Mozilla root
  certificate data, external data used by the TLS client of
  `dagon ai topology`).
- `subtle`, `rp-pac`, `rp2040-boot2` and `source-map-js` are BSD-3-Clause.
- `lightningcss` (MPL-2.0) is a build-time dependency of the web tooling
  only; nothing from it ships in the page.
- The page ships the Svelte client runtime and the two packages it
  imports, `clsx` and `esm-env` (all MIT); every other npm package is a
  build or test tool (column "Ships in").
- Fonts in `web/public/fonts/` are under the SIL Open Font License 1.1:
  Doto, DotGothic16, Zen Kaku Gothic New and JetBrains Mono (licence texts
  next to the files; see `web/README.md`).

Rust crates: 310 packages, 343 name/version pairs. "Used by": `crates/` is the host workspace, other names are firmware packages under `targets/`.

| Crate | Versions | Licence | Used by |
|---|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| aligned | 0.4.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| ascii-canvas | 4.0.0 | Apache-2.0/MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| az | 1.2.1, 1.3.0 | MIT/Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bare-metal | 0.2.5 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| base64 | 0.23.1 | MIT OR Apache-2.0 | crates/ |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bit_field | 0.10.3 | Apache-2.0/MIT | f401-app, f401-board, f401-bootcore, f401-bringup |
| bitfield | 0.13.2, 0.19.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| bitflags | 1.3.2, 2.13.2 | MIT OR Apache-2.0 / MIT/Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| block-buffer | 0.10.4, 0.12.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| block-device-driver | 0.2.0 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates/ |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| byteorder | 1.5.0 | Unlicense OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| bytes | 1.12.1 | MIT | crates/ |
| cc | 1.10.0 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| codespan-reporting | 0.11.1 | Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | crates/ |
| cordyceps | 0.3.5 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | crates/ |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | crates/ |
| cortex-m | 0.7.10 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cortex-m-macros | 0.7.8 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cortex-m-rt | 0.7.7 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cortex-m-rt-macros | 0.7.7 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| cpufeatures | 0.2.17, 0.3.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| crc-any | 2.5.1 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| crunchy | 0.2.4 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| crypto-common | 0.1.7, 0.2.2 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| darling | 0.20.11, 0.21.3, 0.24.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| darling_core | 0.20.11, 0.21.3, 0.24.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| darling_macro | 0.20.11, 0.21.3, 0.24.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| debug-helper | 0.3.14 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| defmt | 0.3.101, 1.2.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| defmt-rtt | 1.3.1 | MIT OR Apache-2.0 | f401-app, f401-bringup |
| delegate | 0.13.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| digest | 0.10.7, 0.11.3 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| document-features | 0.2.12 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ed25519-compact | 2.6.0 | MIT | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-hub, rp2040-stage0 |
| either | 1.19.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-hal-internal | 0.4.0, 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-net-driver-channel | 0.4.0 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| embassy-rp | 0.10.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-stm32 | 0.6.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| embassy-sync | 0.6.2, 0.7.2, 0.8.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| embassy-usb | 0.6.0 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| embassy-usb-driver | 0.2.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embassy-usb-synopsys-otg | 0.3.3, 0.4.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 | rp2040-hub |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 | rp2040-hub |
| embedded-hal | 0.2.7, 1.0.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-hal-bus | 0.3.0 | MIT OR Apache-2.0 | rp2040-hub |
| embedded-hal-nb | 1.0.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-io | 0.6.1, 0.7.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-io-async | 0.6.1, 0.7.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ena | 0.14.4 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| encode_unicode | 0.3.6 | MIT/Apache-2.0 | rp2040-bringup, rp2040-hub |
| enumset | 1.1.15 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port |
| enumset_derive | 0.15.1 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | crates/, esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| esp-bootloader-esp-idf | 0.6.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-config | 0.8.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-hal | 1.2.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-hal-procmacros | 0.23.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-metadata-generated | 0.5.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-riscv-rt | 0.15.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-rom-sys | 0.1.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-storage | 0.10.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp-sync | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32 | 0.41.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c2 | 0.30.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c3 | 0.33.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c5 | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c6 | 0.24.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32c61 | 0.4.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32h2 | 0.20.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32p4 | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32s2 | 0.32.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32s3 | 0.36.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| esp32s31 | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| fixed | 1.29.0, 1.31.0 | MIT/Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| float-cmp | 0.9.0 | MIT | rp2040-hub |
| fnv | 1.0.7 | Apache-2.0 / MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| fugit | 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup |
| gcd | 2.3.0 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port |
| generator | 0.8.11 | MIT/Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| generic-array | 0.14.7 | MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| getrandom | 0.2.17, 0.4.3 | MIT OR Apache-2.0 | crates/ |
| half | 2.7.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| heapless | 0.8.0, 0.9.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| heck | 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| http | 1.5.0 | MIT OR Apache-2.0 | crates/ |
| httparse | 1.10.1 | MIT OR Apache-2.0 | crates/ |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-hub, rp2040-stage0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | crates/, esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| indoc | 2.0.8 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| instability | 0.3.14 | MIT | esp32s3-agent, esp32s3-port |
| io-kit-sys | 0.4.1 | MIT / Apache-2.0 | crates/ |
| itertools | 0.14.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| itoa | 1.0.18 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port |
| jiff | 0.2.38 | Unlicense OR MIT | esp32s3-agent, esp32s3-port |
| jiff-core | 0.1.1 | Unlicense OR MIT | esp32s3-agent, esp32s3-port |
| jiff-static | 0.2.38 | Unlicense OR MIT | esp32s3-agent, esp32s3-port |
| js-sys | 0.3.107 | MIT OR Apache-2.0 | crates/ |
| keccak | 0.1.6 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lalrpop | 0.22.2 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lalrpop-util | 0.22.2 | Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| libc | 0.2.191 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| libyaml-rs | 0.3.0 | MIT | crates/ |
| litrs | 1.0.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| log | 0.4.35 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| loom | 0.7.2 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | crates/ |
| matchers | 0.2.0 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| memchr | 2.8.3 | Unlicense OR MIT | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| micromath | 2.1.0 | Apache-2.0 OR MIT | rp2040-hub |
| mipidsi | 0.10.0 | MIT | rp2040-hub |
| nb | 0.1.3, 1.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| new_debug_unreachable | 1.0.6 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| nix | 0.26.4 | MIT | crates/ |
| nu-ansi-term | 0.50.3 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| object | 0.37.3 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| panic-halt | 1.0.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| panic-probe | 1.0.0 | MIT OR Apache-2.0 | f401-app, f401-bringup |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| paste | 1.0.15 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | crates/ |
| petgraph | 0.7.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| phf_shared | 0.11.3 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pico-args | 0.5.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bringup, rp2040-hub |
| pio | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pio-core | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pio-parser | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| pio-proc | 0.3.0 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bringup, rp2040-hub |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | esp32s3-agent, esp32s3-port |
| precomputed-hash | 0.1.1 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| proc-macro-error-attr2 | 2.0.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| proc-macro-error2 | 2.0.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| quote | 1.0.47 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | crates/ |
| rand_core | 0.10.1, 0.6.4, 0.9.5 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| redox_syscall | 0.5.18 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| regex | 1.13.1 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| rgb | 0.8.53 | MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ring | 0.17.14 | Apache-2.0 AND ISC | crates/ |
| riscv | 0.15.0 | ISC | esp32s3-agent, esp32s3-port |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| riscv-pac | 0.2.0 | ISC | esp32s3-agent, esp32s3-port |
| riscv-rt | 0.16.0 | ISC | esp32s3-agent, esp32s3-port |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| riscv-target-parser | 0.1.3 | ISC | esp32s3-agent, esp32s3-port |
| rp-pac | 7.0.0 | BSD-3-Clause | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| rp2040-boot2 | 0.3.0 | BSD-3-Clause | rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| rustc_version | 0.2.3 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | crates/ |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | crates/ |
| rustls-webpki | 0.103.16 | ISC | crates/ |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | crates/, esp32s3-agent, esp32s3-port |
| same-file | 1.0.6 | Unlicense/MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | crates/, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| sdio | 0.5.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| sdio-host | 0.9.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| semver | 0.9.0 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| semver-parser | 0.7.0 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| serde | 1.0.229 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, rp2040-bringup, rp2040-hub |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | crates/ |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates/ |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| serialport | 4.10.1 | MPL-2.0 | crates/ |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-hub, rp2040-stage0 |
| sha2-const-stable | 0.1.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| sha3 | 0.10.9 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| sharded-slab | 0.1.7 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| shlex | 2.0.1 | MIT OR Apache-2.0 | crates/, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| slab | 0.4.12 | MIT | crates/ |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| smart-leds | 0.4.0 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| smart-leds-trait | 0.3.2 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| somni-expr | 0.3.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| somni-parser | 0.3.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| somni-template | 0.3.2 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| ssmarshal | 1.0.0 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| static_cell | 2.1.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| stm32-fmc | 0.4.0 | MIT/Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| stm32-metapac | 21.0.0 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| strsim | 0.11.1 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| strum | 0.27.2 | MIT | esp32s3-agent, esp32s3-port |
| strum_macros | 0.27.2 | MIT | esp32s3-agent, esp32s3-port |
| subtle | 2.6.1 | BSD-3-Clause | crates/ |
| syn | 1.0.109, 2.0.119, 3.0.9 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| term | 1.2.1 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| termcolor | 1.4.1 | Unlicense OR MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tokio | 1.54.1 | MIT | crates/ |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates/ |
| toml_datetime | 0.6.11, 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port |
| toml_edit | 0.22.27, 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates/ |
| tracing | 0.1.44 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-attributes | 0.1.31 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-core | 0.1.36 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-log | 0.2.0 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| tracing-subscriber | 0.3.23 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| trait-set | 0.3.0 | MIT | f401-app, f401-board, f401-bootcore, f401-bringup |
| typenum | 1.20.1 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-testimg, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| unescaper | 0.1.10 | MIT OR GPL-3.0-only | crates/ |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| unicode-width | 0.1.14 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| unsafe-libyaml | 0.2.11 | MIT | esp32s3-agent, esp32s3-port |
| untrusted | 0.9.0 | ISC | crates/ |
| ureq | 3.5.0 | MIT OR Apache-2.0 | crates/ |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | crates/ |
| usb-device | 0.3.2 | MIT | rp2040-bringup, rp2040-hub |
| usbd-hid | 0.10.2 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| usbd-hid-descriptors | 0.10.2 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| usbd-hid-macros | 0.10.2 | MIT OR Apache-2.0 | rp2040-bringup, rp2040-hub |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | crates/ |
| valuable | 0.1.1 | MIT | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| vcell | 0.1.3 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| version_check | 0.9.5 | MIT/Apache-2.0 | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| void | 1.0.2 | MIT | esp32s3-agent, esp32s3-port, f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| volatile-register | 0.2.2 | MIT OR Apache-2.0 | f401-app, f401-board, f401-bootcore, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub, rp2040-stage0 |
| walkdir | 2.5.0 | Unlicense/MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-futures | 0.4.80 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | crates/ |
| winapi-util | 0.1.11 | Unlicense OR MIT | esp32s3-agent, esp32s3-port, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | f401-app, f401-bringup, rp2040-bringup, rp2040-hub |
| windows-sys | 0.52.0, 0.61.2 | MIT OR Apache-2.0 | crates/, esp32s3-agent, esp32s3-port, f401-app, f401-bringup, rp2040-bootcore, rp2040-bringup, rp2040-hub |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| winnow | 0.7.14, 1.0.4 | MIT | crates/, esp32s3-agent, esp32s3-port |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| xtensa-lx-rt | 0.23.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 | esp32s3-agent, esp32s3-port |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 | crates/ |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | rp2040-bootcore, rp2040-bringup, rp2040-hub |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | crates/ |
| zmij | 1.0.23 | MIT | crates/ |

npm packages (`web/package-lock.json`): 86. Only those marked "page" ship in the built page (the Svelte client runtime and what it imports); the rest are build and test tools.

| Package | Version | Licence | Ships in |
|---|---|---|---|
| @jridgewell/gen-mapping | 0.3.13 | MIT | build/test |
| @jridgewell/remapping | 2.3.5 | MIT | build/test |
| @jridgewell/resolve-uri | 3.1.2 | MIT | build/test |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT | build/test |
| @jridgewell/trace-mapping | 0.3.31 | MIT | build/test |
| @oxc-project/types | 0.156.0 | MIT | build/test |
| @rolldown/binding-android-arm-eabi | 1.2.16 | MIT | build/test |
| @rolldown/binding-android-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-darwin-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-darwin-x64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-freebsd-x64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm-gnueabihf | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-arm64-musl | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-ppc64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-s390x-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-x64-gnu | 1.2.16 | MIT | build/test |
| @rolldown/binding-linux-x64-musl | 1.2.16 | MIT | build/test |
| @rolldown/binding-openharmony-arm64 | 1.2.16 | MIT | build/test |
| @rolldown/binding-win32-arm64-msvc | 1.2.16 | MIT | build/test |
| @rolldown/binding-win32-x64-msvc | 1.2.16 | MIT | build/test |
| @rolldown/pluginutils | 1.0.1 | MIT | build/test |
| @sveltejs/acorn-typescript | 1.0.13 | MIT | build/test |
| @sveltejs/load-config | 0.2.3 | MIT | build/test |
| @sveltejs/vite-plugin-svelte | 7.3.1 | MIT | build/test |
| @types/chai | 5.2.3 | MIT | build/test |
| @types/deep-eql | 4.0.2 | MIT | build/test |
| @types/estree | 1.0.10 | MIT | build/test |
| @vitest/mocker | 5.0.5 | MIT | build/test |
| @vitest/spy | 5.0.5 | MIT | build/test |
| acorn | 8.19.0 | MIT | build/test |
| aria-query | 5.3.1 | Apache-2.0 | build/test |
| assertion-error | 2.0.1 | MIT | build/test |
| axobject-query | 4.1.0 | Apache-2.0 | build/test |
| chai | 6.3.0 | MIT | build/test |
| chokidar | 4.0.3 | MIT | build/test |
| clsx | 2.1.1 | MIT | page |
| deepmerge | 4.3.1 | MIT | build/test |
| detect-libc | 2.1.2 | Apache-2.0 | build/test |
| devalue | 5.9.4 | MIT | build/test |
| es-module-lexer | 2.3.2 | MIT | build/test |
| esm-env | 1.2.2 | MIT | page |
| esrap | 2.4.0 | MIT | build/test |
| estree-walker | 3.0.3 | MIT | build/test |
| expect-type | 1.4.0 | Apache-2.0 | build/test |
| fdir | 6.5.0 | MIT | build/test |
| fsevents | 2.3.3 | MIT | build/test |
| is-reference | 3.0.3 | MIT | build/test |
| lightningcss | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-android-arm64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-darwin-arm64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-darwin-x64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-freebsd-x64 | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm-gnueabihf | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm64-gnu | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-arm64-musl | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-x64-gnu | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-linux-x64-musl | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-win32-arm64-msvc | 1.33.0 | MPL-2.0 | build/test |
| lightningcss-win32-x64-msvc | 1.33.0 | MPL-2.0 | build/test |
| locate-character | 3.0.0 | MIT | build/test |
| magic-string | 0.30.21 | MIT | build/test |
| magic-string | 1.4.5 | MIT | build/test |
| mri | 1.2.0 | MIT | build/test |
| nanoid | 3.3.20 | MIT | build/test |
| obug | 2.2.1 | MIT | build/test |
| obug | 3.0.0 | MIT | build/test |
| picocolors | 1.1.1 | ISC | build/test |
| picomatch | 4.0.7 | MIT | build/test |
| postcss | 8.5.32 | MIT | build/test |
| readdirp | 4.1.2 | MIT | build/test |
| rolldown | 1.2.16 | MIT | build/test |
| sade | 1.8.1 | MIT | build/test |
| source-map-js | 1.2.2 | BSD-3-Clause | build/test |
| std-env | 4.3.0 | MIT | build/test |
| svelte | 5.57.3 | MIT | page |
| svelte-check | 4.7.8 | MIT | build/test |
| tinybench | 6.2.3 | MIT | build/test |
| tinyexec | 1.3.1 | MIT | build/test |
| tinyglobby | 0.2.17 | MIT | build/test |
| typescript | 6.0.3 | Apache-2.0 | build/test |
| vite | 8.3.5 | MIT | build/test |
| vitefu | 1.1.3 | MIT | build/test |
| vitest | 5.0.5 | MIT | build/test |
| why-is-node-running | 3.2.1 | MIT | build/test |
| zimmerframe | 1.1.5 | MIT | build/test |

## Boundary of self-authored code

- Written for DAGON Boot: everything under `crates/`, `targets/` (except the
  vendor parts below), `web/src/`, `web/scripts/`, `plugin/`, `topology/` and
  `scripts/`.
- Used as dependencies, unmodified, through Cargo and npm: the crates and
  packages listed above (for example embassy, esp-hal, ed25519-compact, sha2,
  serde, Svelte, Vite).
- Unmodified vendor components: see below.

## Components not used (no public bootloader implementation is used)

No code or logic from public bootloader implementations (for example
embassy-boot, MCUboot, wolfBoot) is used. The boot decision, the image and
release formats, the state log, the update protocol and the receivers are
written for this project.

## Unmodified vendor components

- ESP32-S3 2nd stage boot loader: the official ESP-IDF v6.1 bootloader, built
  from the official source with app rollback enabled
  (`targets/esp32s3-2ndstage`: only the project files and `sdkconfig.defaults`
  are ours).
- `esp-bootloader-esp-idf` (crate, MIT OR Apache-2.0): used unmodified by
  the ESP32-S3 port to read and write otadata, which is how a slot is
  selected for that 2nd stage.
- RP2040 boot2: the generic 03h second-stage loader from the `rp2040-boot2`
  crate (BSD-3-Clause), placed unmodified at the start of flash by
  `targets/rp2040-stage0`.
- The STM32F401 system memory boot loader in ROM (used for recovery through
  its documented I2C protocol).

## Limitations

- The ESP32-S3 uses the official ESP-IDF 2nd stage. If that 2nd stage itself
  is broken, DAGON cannot rescue the board; it has to be reflashed over USB
  with the vendor tools.
- Signing keys are files on the PC and the public key is built into the
  firmware. There is no hardware root of trust (no secure boot fuses, no key
  storage in hardware).
- A systematic survey of power cuts at every flash operation on real
  hardware has not been done yet; power cuts are tested in the simulator.
- The USB VID:PID `c0de:cafe` is a test ID, not an assigned one.
- On-device checks are still pending for the F401 node and the ESP32-S3
  node firmware. The hub firmware (RP2040 with LCD) has been flashed and
  starts on the hardware, but its host communication and display are still
  being validated, and so is the full three-board flow.
- `dagon conform` drives the synchronous test sequence and is CLI only; the
  browser does not run conformance tests.
