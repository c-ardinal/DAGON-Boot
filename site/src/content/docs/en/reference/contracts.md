---
title: Contracts and components
description: Every contract's format number and every component's version, from compat.toml.
editUrl: false
---

:::note
Generated from `compat.toml`, `crates/dagon-proto/src/lib.rs`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

Every contract between DAGON's components and the number it is at, and
every component with its version and the format numbers it reads and
writes, as `compat.toml` holds them. The rules for changing them are in
[Versions and releases](/en/explanation/versioning/).

A component's version is its last release (0.1.0 before its first); the
format numbers are this tree's. Changes since the tag
`<component>-v<version>` are unreleased until its next version.

## Contracts

| Contract | Format | Where the number is written |
|---|---|---|
| `wire` | 2 | frames and payloads between host, hub and nodes: byte 2 of every frame, HELLO proto_ver (dagon-proto PROTO_VERSION) |
| `image-header` | 1 | signed 512-byte image header: offset 0x004 (dagon-format header::HDR_VERSION) |
| `state-log` | 1 | boot core and app state log faces: offset 0x004 (dagon-format statelog::FORMAT_VERSION) |
| `manifest` | 1 | release manifest: offset 4 (dagon-format manifest::MF_VERSION) |
| `bundle` | 1 | release file .dgb: offset 4 (dagon-format bundle::BN_VERSION) |
| `topology-bin` | 2 | compiled topology: offset 4 (dagon-format topology::TP_VERSION) |
| `topology-yaml` | 2 | topology source: its `dagon:` key (dagon-topology SCHEMA_VERSION) |
| `definitions` | 3 | dagon-mcu.toml / dagon-board.toml / [board.&lt;id&gt;]: `format` key, left out = 1 (dagon-topology defs::FORMAT) |
| `serve-api` | 1 | dagon serve HTTP API: GET /v1/hello `api` (dagon-cli versions::SERVE_API) |
| `layout` | 1 | the layout file beside the 構成: its `format` (dagon-cli versions::LAYOUT_FORMAT) |
| `event` | 1 | lifecycle event kinds, codes, payloads and EventRecord (dagon-events EVENT_FORMAT) |
| `extension-manifest` | 1 | dagon-extension.toml: its `format` key (dagon-extension MANIFEST_FORMAT) |
| `boot-program` | 1 | boot action program ops in a board's [boot]: the generated BOOT_PROGRAM_FORMAT (dagon-topology defs::board_boot::BOOT_PROGRAM_FORMAT) |
| `project` | 1 | dagon.toml: its `format` key, required (dagon-topology project::FORMAT) |
| `agent-def` | 1 | agent definition (agents/*.toml, ~/.dagon/agents/*.toml): its `dagon_agent` key (dagon-cli agents::FORMAT) |
| `agent-link` | 1 | the consent to an agent, ~/.dagon/agent.json: its `version` (dagon-cli consent::FILE_VERSION) |
| `check-json` | 1 | the check report as JSON (dagon topo check --json, the page's check): its `format` (dagon-topology CHECK_JSON_FORMAT) |
| `conform-result` | 1 | conformance result file: the N of its `schema` "dagon-conform/N" (dagon-conform report::SCHEMA_VERSION) |

## Components

| Component | Version (last release) | Packages | Formats (this tree) |
|---|---|---|---|
| `cli` | 0.1.0 | `dagon-cli` | `wire` 2, `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 2, `topology-yaml` 2, `definitions` 3, `serve-api` 1, `layout` 1, `event` 1, `extension-manifest` 1, `boot-program` 1, `project` 1, `agent-def` 1, `agent-link` 1, `check-json` 1, `conform-result` 1 |
| `boot-rp2040` | 0.1.0 | `dagon-rp2040-stage0`, `dagon-rp2040-bootcore` | `image-header` 1, `state-log` 1, `event` 1 |
| `boot-stm32f401re` | 0.1.0 | `dagon-stm32f401re-stage0`, `dagon-stm32f401re-bootcore` | `image-header` 1, `state-log` 1, `event` 1 |
| `mcu-rp2040` | 0.1.0 | `dagon-mcu-rp2040` | `wire` 2, `image-header` 1, `state-log` 1, `definitions` 3 |
| `mcu-stm32f401re` | 0.1.0 | `dagon-mcu-stm32f401re` | `wire` 2, `image-header` 1, `state-log` 1, `definitions` 3 |
| `mcu-esp32s3` | 0.1.0 | `dagon-mcu-esp32s3` | `wire` 2, `image-header` 1, `state-log` 1, `definitions` 3 |
| `mcu-common` | 0.1.0 | `dagon-mcu-common` | `wire` 2 |
| `node` | 0.1.0 | `dagon-node` | `wire` 2, `image-header` 1, `state-log` 1 |
| `mcu-build` | 0.1.0 | `dagon-mcu-build` |  |
| `board-waveshare-rp2040-touch-lcd-1.28` | 0.1.0 | `dagon-board-waveshare-rp2040-touch-lcd-1-28` | `definitions` 3 |
| `board-nucleo-f401re` | 0.1.0 | `dagon-board-nucleo-f401re` | `definitions` 3 |
| `board-m5stopwatch` | 0.1.0 | `dagon-board-m5stopwatch` | `definitions` 3 |
| `format` | 0.1.0 | `dagon-format` | `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 2 |
| `ed25519` | 0.1.0 | `dagon-ed25519` |  |
| `proto` | 0.1.0 | `dagon-proto` | `wire` 2 |
| `core` | 0.1.0 | `dagon-core` | `image-header` 1, `state-log` 1 |
| `boot` | 0.1.0 | `dagon-boot` | `state-log` 1, `event` 1 |
| `events` | 0.1.0 | `dagon-events` | `event` 1 |
| `port` | 0.1.0 | `dagon-port` |  |
| `agent` | 0.1.0 | `dagon-agent` | `wire` 2, `image-header` 1 |
| `relay` | 0.1.0 | `dagon-relay` | `wire` 2 |
| `host` | 0.1.0 | `dagon-host` | `wire` 2, `image-header` 1, `state-log` 1, `bundle` 1, `topology-bin` 2 |
| `extension` | 0.1.0 | `dagon-extension` | `extension-manifest` 1, `definitions` 3, `boot-program` 1 |
| `topology` | 0.1.0 | `dagon-topology` | `topology-bin` 2, `topology-yaml` 2, `definitions` 3, `boot-program` 1, `project` 1, `check-json` 1 |
| `sign` | 0.1.0 | `dagon-sign` | `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 2 |
| `sim` | 0.1.0 | `dagon-sim` | `wire` 2, `image-header` 1, `state-log` 1 |
| `conform` | 0.1.0 | `dagon-conform` | `wire` 2, `image-header` 1, `state-log` 1, `conform-result` 1 |
| `romlink` | 0.1.0 | `dagon-romlink` |  |

## Frames (`wire`)

The bits of a frame's `flags` byte, as `dagon-proto`'s `flags` holds them.

| Flag | Value | Meaning |
|---|---|---|
| `RESP` | `0x01` | The frame answers the request with the same `seq` and `cmd`. |
| `EVENT` | `0x02` | Reserved for an event a node would send unasked; nothing sends it, and every receiver ignores a frame that carries it. |
| `RETRY` | `0x04` | The request is sent again after an answer did not arrive in time. |

The payload limit per link (`max_payload`):

| Link | Bytes | Meaning |
|---|---|---|
| `USB` | 1088 | USB CDC, between the PC and the hub. |
| `UART` | 1024 | A UART link. |
| `I2C` | 256 | An I2C link. |
| `SMALLEST` | 64 | The least a link offers, whatever its medium: a link that carries less in one piece puts frames together below the frame, up to at least this. Every node's HELLO and CAPS answers fit it, so a requester learns over any link what the node serves. |
