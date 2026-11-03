# DAGON project

This folder is a DAGON Boot project: firmware for a small group of
microcontrollers that one hub updates with signed A/B images, rolling a
node back by itself when a new version never gets confirmed.

## Words

- ノード (node): one microcontroller in the 構成.
- 親機 (hub): the node on the PC's USB; it relays updates to the others.
- 子機 (child): a node the hub reaches over I2C or UART.
- 構成 (topology): `topology.yaml`, the nodes, their links and the update
  order.

Use these words with the developer, and count in ノード ("3 ノード").

## Layout

- `dagon.toml`: project settings. `topology`, `pubkey` (the public key the
  firmware embeds), `[dagon]` (where the DAGON crates come from),
  `[build.<id>]` (how `dagon build` builds a node), `[board.<id>]` (facts
  for a node chosen by MCU, such as `flash_bytes`), `[provision.<id>]`
  (first-write inputs).
- `topology.yaml`: the 構成. A node with `board:` uses that board's
  package; a node with only `mcu:` uses the MCU package.
- `nodes/<id>/`: one Rust crate per node, made by `dagon gen`.
  - `src/app.rs`: the developer's application. Code changes go here (and
    in files the developer added).
  - `src/main.rs`: start-up. `dagon gen` keeps it; only
    `dagon gen --force-main` writes it again.
  - `src/wiring.rs`: generated from the 構成. Never edit it; change
    `topology.yaml` and run `dagon gen`.
  - `Cargo.toml`: `dagon gen` rewrites only the `dagon-board-*` and
    `dagon-mcu-*` lines; other dependencies are kept.
- `out/`: build, signing and release output (not committed).

## Commands

- `dagon topo check topology.yaml` (add `--json` for the rule ids and paths)
- `dagon gen [<id>...]`, `dagon build [<id>...]` (ELF files in
  `out/build/<id>/`)
- `dagon serve`: DAGON Console, the browser page for the 構成, builds and
  updates
- `dagon flash <id> [--dry-run]`: the first write of one node over its own
  cable (RP2040: UF2 to the RPI-RP2 drive; STM32F401RE: probe-rs through
  the ST-LINK; ESP32-S3: espflash over USB-C, the bundled ESP-IDF 2nd
  stage at 0x0, the partition table, the factory agent, slot A and
  otadata, with exactly one ESP32-S3 connected). `--dry-run` makes the
  files and prints the commands only.
- `dagon deploy [<id>...]`: build, sign, release and apply in one go, every
  family included (an ESP32-S3 build becomes one ESP image via
  `espflash save-image`).
- `dagon status`, `dagon apply <dgb>`, `dagon rollback --node <id>`,
  `dagon recover <node>`
- `dagon --help` lists every command and option.

## Rules

- Never open, print, copy or move the signing secret key (the file named
  by `DAGON_SIGNING_KEY`) or any API key. Do not put keys or paths from
  outside this folder into project files.
- Signing, building releases and anything that writes to hardware
  (`image sign`, `release build`, `deploy`, `apply`, `flash`,
  `provision`, `rescue`, `rollback`, `recover`, `topo push`) are the developer's steps. Suggest
  the command; run it only when the developer asks.
- Ask for hardware facts (pins, I2C addresses, flash size, baud rate)
  instead of guessing, and say which board document each value came from.
- Do not edit the DAGON crates, the 構成 checker or the conformance tests
  to make a problem go away.
