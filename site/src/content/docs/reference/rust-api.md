---
title: Rust API
description: The workspace crates and their rustdoc.
editUrl: false
---

:::note
Generated from `Cargo.toml`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

The Rust API documentation (rustdoc) of every crate in the workspace is
published with these pages; each name below opens its crate.

| Crate | What it holds |
|---|---|
| [`dagon-agent`](/api/dagon_agent/) | DAGON agent: embedded in an application, it answers HELLO/STATUS/VERSION, receives images into the inactive slot, switches the boot target and confirms. |
| [`dagon-boot`](/api/dagon_boot/) | The boot core's sequence as a state machine: every step from the armed watchdog to the jump or the halt is an event the boot core's extensions observe |
| [`dagon-catalog`](/api/dagon_catalog/) | Every DAGON state machine's catalog in one registry, and the generated Events and Ports pages of the documentation |
| [`dagon-cli`](/api/dagon/) | DAGON Boot command line tool |
| [`dagon-conform`](/api/dagon_conform/) | DAGON conformance tests: the 12 normal-path checks a port must pass. |
| [`dagon-core`](/api/dagon_core/) | Board-independent DAGON logic over the port contract: the state log (fold, load, append, compaction), the kind S boot decision (verify, trial, rollback), the boot core A/B rules of a stage0 MCU and build-time key helpers. |
| [`dagon-events`](/api/dagon_events/) | DAGON lifecycle events: state machines whose transitions emit events, extensions, ports and the published event state |
| [`dagon-extension`](/api/dagon_extension/) | DAGON extension manifests (dagon-extension.toml), their validation against the port and event catalogs and the node's definitions, the protection-kind map generators, and the files dagon gen writes from them |
| [`dagon-format`](/api/dagon_format/) | Byte layouts shared by every DAGON component: image header, manifest, release bundle (`.dgb`), state log, topology canonical binary, plus CRC-32, SHA-256 and Ed25519 verification. All multi-byte fields are little-endian. |
| [`dagon-host`](/api/dagon_host/) | Host-side procedures shared by the CLI and the browser: discover, status, topology push, apply, rollback, confirm, recover. |
| [`dagon-port`](/api/dagon_port/) | Port contract: what a board provides to DAGON. |
| [`dagon-proto`](/api/dagon_proto/) | DAGON link v1: frame format (shared by USB CDC, UART and I2C), the command set, status codes, request/response payloads and the I2C mailbox state. |
| [`dagon-retained-model`](/api/dagon_retained_model/) | Executable model of DAGON's retained and persistent formats under fault injection |
| [`dagon-romlink`](/api/dagon_romlink/) | STM32 system-memory boot loader on I2C (ST AN4221), driven from the host through the hub's NRST/BOOT0 lines and raw I2C transactions. |
| [`dagon-sign`](/api/dagon_sign/) | Signing for DAGON: keys, signed images, release bundles and first-install flash contents. |
| [`dagon-sim`](/api/dagon_sim/) | DAGON simulator: deterministic stand-ins for the hardware so the host tools, the browser UI and tests have a partner without boards. |
| [`dagon-topology`](/api/dagon_topology/) | Topology YAML: schema, validation rules and the supported-board table. Shared by the CLI and the browser build so both apply the same rules. |
| [`dagon-wasm`](/api/dagon_wasm/) | Browser build of the shared Rust rules: MCU and board definitions, topology check and compile, release verification, HELLO payloads and version text. Each export is a thin wrapper so the page and the CLI apply the same code. |
