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

`dagon --help` lists every command, and `dagon <command> --help` its
arguments and options; the same reference is the Command line page of
DAGON's documentation (Reference, Command line).
The ones a project uses most: `dagon topo check topology.yaml`, `dagon gen`,
`dagon build`, `dagon serve` (DAGON Console), `dagon flash <id>` (a node's
first write) and `dagon deploy`.

## Rules

- Never open, print, copy or move the signing secret key (the file named
  by `DAGON_SIGNING_KEY`) or any API key. API keys live only in the OS
  credential store; registering or deleting one (`dagon ai key set`,
  `dagon ai key delete`) is the developer's step. Never ask for a key, put
  one in an environment variable, or read it from the store. Do not put
  keys or paths from outside this folder into project files.
- Signing, building releases and anything that writes to hardware
  (`image sign`, `release build`, `deploy`, `apply`, `flash`,
  `provision`, `rescue`, `rollback`, `recover`, `topo push`) are the developer's steps. Suggest
  the command; run it only when the developer asks.
- Ask for hardware facts (pins, I2C addresses, flash size, baud rate)
  instead of guessing, and say which board document each value came from.
- Do not edit the DAGON crates, the 構成 checker or the conformance tests
  to make a problem go away.
