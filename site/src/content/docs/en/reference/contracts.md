---
title: Contracts and components
description: Every contract's format number and every component's version, from compat.toml.
editUrl: false
---

:::note
Generated from `compat.toml`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

Every contract between DAGON's components and the number it is at, and
every component with its version and the format numbers it reads and
writes, as `compat.toml` holds them. The rules for changing them are in
[Versions and releases](/en/explanation/versioning/).

## Contracts

| Contract | Format | Where the number is written |
|---|---|---|
| `wire` | 1 | frames and payloads between host, hub and nodes: byte 2 of every frame, HELLO proto_ver (dagon-proto PROTO_VERSION) |
| `image-header` | 1 | signed 512-byte image header: offset 0x004 (dagon-format header::HDR_VERSION) |
| `state-log` | 1 | boot core and app state log faces: offset 0x004 (dagon-format statelog::FORMAT_VERSION) |
| `manifest` | 1 | release manifest: offset 4 (dagon-format manifest::MF_VERSION) |
| `bundle` | 1 | release file .dgb: offset 4 (dagon-format bundle::BN_VERSION) |
| `topology-bin` | 1 | compiled topology: offset 4 (dagon-format topology::TP_VERSION) |
| `topology-yaml` | 1 | topology source: its `dagon:` key (dagon-topology SCHEMA_VERSION) |
| `definitions` | 1 | dagon-mcu.toml / dagon-board.toml / [board.&lt;id&gt;]: `format` key, left out = 1 (dagon-topology defs::FORMAT) |
| `serve-api` | 1 | dagon serve HTTP API: GET /v1/hello `api` (dagon-cli versions::SERVE_API) |
| `event` | 1 | lifecycle event kinds, codes, payloads and EventRecord (dagon-events EVENT_FORMAT) |
| `extension-manifest` | 1 | dagon-extension.toml: its `format` key (dagon-extension MANIFEST_FORMAT) |
| `boot-program` | 1 | boot action program ops in a board's [boot]: the generated BOOT_PROGRAM_FORMAT (dagon-topology defs::board_boot::BOOT_PROGRAM_FORMAT) |

## Components

| Component | Version | Packages | Formats |
|---|---|---|---|
| `cli` | 0.1.0 | `dagon-cli` | `wire` 1, `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 1, `topology-yaml` 1, `definitions` 1, `serve-api` 1, `event` 1, `extension-manifest` 1, `boot-program` 1 |
| `boot-rp2040` | 0.1.0 | `dagon-rp2040-stage0`, `dagon-rp2040-bootcore` | `image-header` 1, `state-log` 1, `event` 1 |
| `boot-stm32f401re` | 0.1.0 | `dagon-stm32f401re-stage0`, `dagon-stm32f401re-bootcore` | `image-header` 1, `state-log` 1, `event` 1 |
| `mcu-rp2040` | 0.1.0 | `dagon-mcu-rp2040` | `wire` 1, `image-header` 1, `state-log` 1, `topology-bin` 1, `definitions` 1 |
| `mcu-stm32f401re` | 0.1.0 | `dagon-mcu-stm32f401re` | `wire` 1, `image-header` 1, `state-log` 1, `definitions` 1 |
| `mcu-esp32s3` | 0.1.0 | `dagon-mcu-esp32s3` | `wire` 1, `image-header` 1, `state-log` 1, `definitions` 1 |
| `mcu-common` | 0.1.0 | `dagon-mcu-common` | `wire` 1 |
| `mcu-build` | 0.1.0 | `dagon-mcu-build` |  |
| `board-waveshare-rp2040-touch-lcd-1.28` | 0.1.0 | `dagon-board-waveshare-rp2040-touch-lcd-1-28` | `definitions` 1 |
| `board-nucleo-f401re` | 0.1.0 | `dagon-board-nucleo-f401re` | `definitions` 1 |
| `board-m5stopwatch` | 0.1.0 | `dagon-board-m5stopwatch` | `definitions` 1 |
| `format` | 0.1.0 | `dagon-format` | `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 1 |
| `proto` | 0.1.0 | `dagon-proto` | `wire` 1 |
| `core` | 0.1.0 | `dagon-core` | `image-header` 1, `state-log` 1 |
| `boot` | 0.1.0 | `dagon-boot` | `state-log` 1, `event` 1 |
| `events` | 0.1.0 | `dagon-events` | `event` 1 |
| `port` | 0.1.0 | `dagon-port` |  |
| `agent` | 0.1.0 | `dagon-agent` | `wire` 1, `image-header` 1 |
| `host` | 0.1.0 | `dagon-host` | `wire` 1, `image-header` 1, `state-log` 1, `bundle` 1, `topology-bin` 1 |
| `extension` | 0.1.0 | `dagon-extension` | `extension-manifest` 1, `definitions` 1, `boot-program` 1 |
| `topology` | 0.1.0 | `dagon-topology` | `topology-bin` 1, `topology-yaml` 1, `definitions` 1, `boot-program` 1 |
| `sign` | 0.1.0 | `dagon-sign` | `image-header` 1, `state-log` 1, `manifest` 1, `bundle` 1, `topology-bin` 1 |
| `sim` | 0.1.0 | `dagon-sim` | `wire` 1, `image-header` 1, `state-log` 1 |
| `conform` | 0.1.0 | `dagon-conform` | `wire` 1, `image-header` 1, `state-log` 1 |
| `romlink` | 0.1.0 | `dagon-romlink` |  |
