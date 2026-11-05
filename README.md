<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/product-symbol/lockup-horizontal-dark-2400w.png">
    <img src="assets/product-symbol/lockup-horizontal-light-2400w.png" alt="DAGON Boot" width="480">
  </picture>
</p>

# DAGON Boot (DIY Assured-update Gateway for Own Nodes)

[日本語版 / Japanese: README.ja.md](README.ja.md)

**Make hardware that works, easily, and keep growing it after it ships.**

DAGON Boot is for developers, from first-timers to experienced firmware
engineers, who build something out of a few microcontrollers and want to
keep improving it once it is on the desk, the wall or the wrist. You choose
the boards (or just their MCUs), draw how they connect, and write your
application in Rust. DAGON takes care of the rest:

- **A project that builds.** `dagon new` and `dagon gen` give every node a
  Rust crate with its start-up, its wiring and an empty `app.rs` for your
  code. Flash layout, image headers, linker addresses and the boot loader
  are decided for you.
- **Safe updates for the whole group.** One hub on your PC's USB sends each
  node its new version. Every image is signed; a node runs a new version on
  trial and goes back to the previous one by itself when the new one never
  gets confirmed. A node that no longer answers can be rewritten through the
  hub.
- **A browser tool and a command line that do the same.** DAGON Console
  (`dagon serve`) shows the nodes and links as a graph, checks them as you
  edit, builds, signs and updates. Every step is also a `dagon` command.
- **Help from Claude.** A new project comes with Claude Code instructions
  and skills for the common questions: a failing build, a board that is not
  in the list, a problem in the topology.

## Words used here

- **node**: one microcontroller of your setup.
- **hub**: the node on the PC's USB. It relays updates to the other nodes.
- **child**: a node the hub reaches over I2C or UART.
- **topology**: the nodes, how they are linked, and the update order
  (`topology.yaml`).

## Supported hardware

| MCU | Role in the desk demo | Board in the table | Flash it needs |
|---|---|---|---|
| RP2040 (`rp2040`) | hub | Waveshare RP2040-Touch-LCD-1.28 (`waveshare-rp2040-touch-lcd-1.28`) | 4 MiB |
| STM32F401RE (`stm32f401re`) | child, I2C | NUCLEO-F401RE (`nucleo-f401re`) | 512 KiB |
| ESP32-S3 (`esp32s3`) | child, UART | M5StopWatch (`m5stopwatch`) | 16 MiB |

**Choose by board or by MCU.** A board from the table brings its pins,
default link and extras; you only pick it. Any other board with one of
these MCUs can be chosen by its MCU: you give its flash size and the pins
wired to the hub (see [Your own board](#your-own-board-chosen-by-mcu)).

## Prerequisites

| Tool | When | How |
|---|---|---|
| Rust (`rustup`) | always | <https://rustup.rs> |
| Rust targets `thumbv6m-none-eabi`, `thumbv7em-none-eabihf` | RP2040, STM32F401RE | `rustup target add thumbv6m-none-eabi thumbv7em-none-eabihf` |
| `espup` and its `esp` toolchain | ESP32-S3 | `cargo install espup`, then `espup install` |
| `probe-rs` and an ST-LINK | first write of an STM32F401RE | `cargo install probe-rs-tools` (ST-LINK driver on Windows) |
| `espflash` | first write of an ESP32-S3 | `cargo install espflash` |
| A C compiler | building the `dagon` CLI from source (the `ring` crate) | MSVC Build Tools on Windows, `cc` elsewhere |
| Chrome or Edge 89+ | DAGON Console (Web Serial) | — |

Install the `dagon` CLI, one of:

- Download the archive for your system (`dagon-cli-<version>-<target>.zip` for Windows,
  `dagon-cli-<version>-<target>.tar.gz` otherwise) from the GitHub release `cli-v<version>`
  you want, check it against its `.sha256` file, and put `dagon` on your
  `PATH`.
- `cargo binstall dagon-cli --git https://github.com/c-ardinal/DAGON-Boot`
  (fetches that same archive; needs `cargo-binstall`).
- Build it: `cargo install --git https://github.com/c-ardinal/DAGON-Boot --tag cli-v<version> dagon-cli`.

No clone of this repository and no git submodule is needed:

```sh
dagon --help
```

A project made by `dagon new` takes DAGON from the same release as your
`dagon` (`[dagon] git` and `tag = "cli-v<version>"` in `dagon.toml`). cargo
fetches it when you build, and `dagon flash` builds the boot core, stage0
and factory agent from cargo's copy of that tag, so it needs the network
the first time. Working on DAGON itself, keep a clone instead:
`dagon new my-fleet --dagon-path <clone>` (or set `DAGON_HOME`) writes
`[dagon] path`.

Then let `dagon` check the tools your topology needs (run it in a project
folder; elsewhere it checks every MCU family):

```sh
dagon doctor           # what is there and what is missing
dagon doctor --fix     # installs what needs no administrator rights
```

For an STM32F401RE node, `dagon doctor` also checks that an ST-LINK is
plugged in (`probe-rs list`); that one it cannot fix for you.

DAGON Console is built into `dagon`; Node.js is only needed to work on
DAGON Console itself (`web/README.md`).

## The journey

The example is the desk demo: an RP2040 hub with a round LCD, a
NUCLEO-F401RE (I2C) and an M5StopWatch (UART). What their sample apps do
is in `samples/f401-app` and `samples/esp32s3-app`. Replace it with your
own topology at step 2.

### 1. Start a project

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new` creates `dagon.toml`, `topology.yaml` (the desk demo, or your
own with `--from <yaml>`), `.gitignore`, the Claude Code files (below) and,
if none exists yet, a signing key pair in `~/.dagon/keys/my-fleet` (or
`--keys <dir>`). The secret key stays outside the project and outside any
git work tree; `dagon.toml` only names the public key. Tell the CLI where
the secret key is when you sign:

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

### 2. Define the topology in DAGON Console

```sh
dagon serve
```

DAGON Console opens in the browser, on 127.0.0.1 only, with a one-time
passphrase. Place nodes from the node list (by product or by MCU), link
them, and fix what the check reports; every edit is saved to
`topology.yaml`. Without a project, `dagon serve` starts on a screen that
creates or opens one.

From the command line the same is `dagon topo check topology.yaml`, or a
first draft from a plain-language description through the Claude API:

```sh
export ANTHROPIC_API_KEY=...        # read by the CLI only, never shown or saved
dagon ai topology --in boards.txt --out topology.yaml
```

Definitions are not a place for secrets. Names in them (ids, product
names, pin labels, partition names) appear in DAGON Console and are sent
to Claude when you ask it; commands, hints, comments and file paths are
never sent. See `targets/README.md`.

### 3. Generate the node crates

```sh
dagon gen
```

One crate per node in `nodes/<id>/`. Write your application in
`nodes/<id>/src/app.rs`: a task that does the work and a `self_check` that
says whether a version on trial may be confirmed. `dagon gen` can run
again any time the topology changes: it rewrites `src/wiring.rs` and the
DAGON package lines of `Cargo.toml`, and never touches `app.rs` (nor
`main.rs`, unless `--force-main`). DAGON Console does the same with
"プロジェクトを作る". `dagon new`, `dagon init` and `dagon gen` never write
or remove anything through a symbolic link or a junction: a node whose
`nodes/<id>` (or `nodes`, or any file gen writes) is one is skipped with
the reason, and `dagon gen` then exits with an error.

### 4. Build

```sh
dagon build            # or: dagon build seg
```

Each node is built into `out/build/<id>/app-a.elf` and, for slot B,
`app-b.elf` (an ESP32-S3 image serves both slots, so it has one), with the
public key from `dagon.toml` embedded. A node's version is the `version`
in its crate's `Cargo.toml` (its boot core's, in its boot crate's
`Cargo.toml`); raise it there for every change you write. A `version` in
`[build.<id>]` or `--version` is not needed; if given, it must be the same
or the command stops and says which. A node's boot core goes into the
package with its app, and the node writes it only when its version differs
from the boot core it runs: a changed boot core is written once its version
is raised, as with apps.

### 5. Write the first version (once per node)

A node needs its boot core and first application written once over its own
cable. With `DAGON_SIGNING_KEY` set (step 1), connect the node and run:

```sh
dagon flash hub        # RP2040: hold BOOT while plugging in USB; rescue.uf2 is copied to the RPI-RP2 drive
dagon flash seg        # STM32F401RE: through the ST-LINK with probe-rs
dagon flash watch      # ESP32-S3: over USB-C with espflash
```

`dagon flash` prints its plan first and asks before it touches the board
(`--yes` skips the question). It writes to a device the writer reports:
every probe-rs probe, every serial port, every UF2 drive (USB IDs and
the UF2 Board-ID only label them and put the matching ones first). One device is taken and shown before
the question; with several it asks which (Enter takes the first match, if any), or takes `--probe
<vid:pid[:serial]>`, `--port <port>` or `--drive <path>` (on Windows
compared without regard to case, so `--drive e:` names `E:\`); with several
and no terminal (or `--yes`) and no option, or with none, it stops
before writing. `--dry-run` still does everything up to
the write: it builds the parts and the app and makes the first-install
files in the provision folder (`out/provision/<id>/` beside `dagon.toml`,
or under `provision_dir` when `dagon.toml` sets it; `--out <dir>`
chooses another), and only leaves the board untouched. It builds the boot core (and the hub's `stage0`) from the DAGON
clone the CLI was built from (or `DAGON_HOME`) and the node's app, all
with the public half of your signing key. The boot cores accept the desk
demo's tags (`hub-rp2040-lcd`, `seg-f401-7seg`), so keep those tags in
`topology.yaml` for these boards.

For an ESP32-S3 node, `dagon flash` builds the board's factory agent and
the app, turns the app into an ESP image (`espflash save-image --chip
esp32s3 --flash-size 16mb`), signs it, and writes with `espflash`: the
bundled unmodified ESP-IDF 2nd stage at 0x0 (refused unless its SHA-256
matches `targets/mcu/esp32s3/vendor/SHA256SUMS`), the partition table
(`targets/mcu/esp32s3/partitions.csv`, at 0x8000), the factory agent, the
app in ota_0 (0x110000) with its signed header in the `dagon` partition
(0x710000), and the otadata entry that starts it as confirmed (0xD000).
`--dry-run`
lists the files with their offsets and the `espflash` commands
(`commands.txt` in the same provision folder).

Then connect the hub and check that every node answers:

```sh
dagon status
```

Without `dagon flash` (boot images you built yourself): `dagon rescue hub`
and `dagon provision <id>` make the same files from `[provision.<id>]`
tables in `dagon.toml` (`stage0` for the hub, `bootcore`, `app`,
`version`); the MCU package READMEs show how to build the boot images.

### 6. Update, and go back

Change your code, then:

```sh
dagon deploy                       # or: dagon deploy seg watch
```

`dagon deploy` builds slots A and B, signs them, makes the release, lists
each node with its old and new version and asks before applying (`--yes`
skips the question). The old version is the node's version in the last
release (`out/release-latest.dgb`, or `release` in `dagon.toml`), read once
that release verifies under your key; it is not read from the boards. The
new version is each node's crate version. Without node ids it takes every node in `[build.<id>]`. An
ESP32-S3 node is built once (its image runs from either OTA slot); deploy
turns the build into an ESP image (`espflash save-image`) and signs that,
and the hub relays it like any other node's. The single steps are
`dagon build`, `dagon image sign`, `dagon release build` and `dagon apply`
(`dagon --help`). In DAGON Console the same is ビルド → 署名 → リリース →
渡す, then the update in 運用 mode, over Web Serial.

Every package written to nodes (by the Console, `dagon apply` or
`dagon deploy`) is kept in the project's `out/deploy/` folder as
`v<version>.dgb` (`v<newest>-<digest>.dgb` when its nodes get different
versions). To go back, write a kept older version again: the Console lists
them, and `dagon apply out/deploy/v1.0.0.dgb` writes one from the command
line.

Applying checks the signatures and that the release was built for this
topology, sends the topology to the hub, updates the children and then the
hub. A node runs the new version on trial; if it is not confirmed, its
watchdog resets it and it boots the previous version again.

```sh
dagon rollback --node seg     # back to the previously confirmed version
dagon recover seg             # a node that no longer answers
```

`dagon --help` lists every command and option.

## Claude help in your project

`dagon new` also writes Claude Code files into the project, so Claude Code
opened in the project folder knows the layout, the commands and the rules:

| File | What it is for |
|---|---|
| `CLAUDE.md` | The project layout, which files are yours and which are generated, the commands, and the rules (keys stay out, hardware facts are asked, not guessed) |
| `.claude/skills/fix-a-failing-build/SKILL.md` | A `dagon build` fails: find the cause, fix only your code in `nodes/<id>/src`, build again |
| `.claude/skills/add-a-board-by-mcu/SKILL.md` | Add a board that is not in the table, chosen by its MCU, with each value taken from the board's documents |
| `.claude/skills/explain-a-topology-problem/SKILL.md` | Explain a problem the topology check reports, and propose the smallest fix |

The files hold no keys and no paths of your PC. Signing keys and the
Claude API key stay with the `dagon` CLI: Claude is told never to open
them, and signing or writing to hardware is left to you.

In this repository, `plugin/` is the Claude Code plugin used to port DAGON
to a new MCU (`/dagon-port`), checked by the conformance tests
(`dagon conform`).

## Your own board (chosen by MCU)

Leave out `board:` for the node in `topology.yaml` and give its facts in
`dagon.toml`:

```yaml
  - id: seg
    mcu: stm32f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

```toml
[board.seg]
flash_bytes = 524288
```

For an I2C child, the link names the pins on the node (`node_pins`). The
check and `dagon gen` tell you what is still missing; the
`add-a-board-by-mcu` skill walks through it with you.

## Try it without hardware

```sh
dagon demo                         # DAGON Console with a simulated desk
dagon demo --sim-fail watch        # the same, and watch's update fails and falls back
dagon demo --sim-speed 4           # four times faster (for recordings)
```

`dagon demo` opens DAGON Console against a simulated 親機 with its two
子機 (the desk-demo 構成: hub, seg, watch, all on 1.0.0), the simulator
`dagon sim run` uses, driven by the same dagon-host code as real boards.
Connect, choose the latest release (demo-1.1.0) and update all three with
live progress; with `--sim-fail seg` or `--sim-fail watch` that 子機's new
version crashes at boot and it goes back to 1.0.0. The links and flash take
about their real time (`--sim-speed` from 0.1 to 20 scales it). The header
shows 模擬 the whole time. The demo makes its own project in the temporary
folder (`dagon-demo-<hash of your user name>`, made afresh at each start) with a DEMO key made for it;
your keys are not used, and nothing it signs runs on real boards.

```sh
dagon sim run
```

Four end-to-end scenarios in the simulator: an update with a power cut
during the transfer, an image that never confirms, tampered releases, and
an erased F401 brought back through its ROM boot loader.

### Swapping a link (つながりを差し替える)

Every hop in a 構成 has a kind: `usb-cdc`, `uart`, `i2c`, `ble`,
`wifi-tcp`, `ethernet` or `can`. A 親機 can also declare its uplink, the
hop towards whatever drives it, and what is at the other end:

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

Without `uplink` it is `usb-cdc` to dagon on a PC, as before; the uplink is
not part of the compiled topology, so existing 構成 and their releases stay
the same. In DAGON Console's 定義 the inspector offers each kind for a link
and, on the 親機, for its uplink. Kinds the firmware of either end does not
implement yet are marked ◇ まだ使えない; the 構成 may still name them
(problem まだ使えないつながり), and `dagon gen` refuses them with the hop's
name (`uplink of hub over ble: not available yet on rp2040`). Today the
firmware offers, as each MCU's definition lists it (`[links]`; up: towards
the PC or the 親機, down: to 子機):

- RP2040: up usb-cdc; down i2c, uart
- STM32F401RE: up i2c
- ESP32-S3: up uart, usb-cdc

The demo shows the swap without new radios:

```sh
dagon demo --uplink uart          # or usb-cdc (default), wifi-tcp
```

The simulated uplink follows the demo 構成: change the 親機's uplink in
定義 (USB, UART, Wi-Fi) and the same update runs over the other medium, with
its own frame size, speed and behaviour while the 親機 reboots (USB CDC
and Wi-Fi reconnect; a UART adapter stays).

## What works today

What the code and its tests in this repository show:

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
  (`dagon-topology`); `topology/desk-demo.yaml` passes them.
- DAGON Console (`web/`, served by `dagon serve`; `web/README.md` lists what
  it does today).
- Firmware: the RP2040 hub (`targets/rp2040-hub`), the F401 demo node
  (`samples/f401-app`) and the ESP32-S3 node (`targets/boards/m5stopwatch`,
  `samples/esp32s3-app`). The hub firmware has been flashed and starts on
  the hub hardware; on-device checks of the rest are still in progress (see
  Limitations).

## Adding DAGON to existing firmware

For firmware that is not a `dagon new` project:

1. Add the crates as git dependencies pinned to a release tag, for example
   `dagon-agent = { git = "<repository URL>", tag = "<release tag>" }`, and
   the same for `dagon-format`, `dagon-proto`, `dagon-port` and, for a boot
   core, `dagon-core`.
2. On a supported MCU, use the MCU package (`targets/mcu/<mcu>`: flash
   layout, linker map, boot images, watchdog, links) and a board package
   (`targets/boards/<board id>`), or implement the MCU package's `BoardDef`
   for your own board; the MCU package READMEs show a complete app in about
   20 lines. On another MCU, lay out the flash the same way: a boot core,
   two state log faces, and slots A and B, each starting with the 512-byte
   signed image header and the body at slot start + 0x200.
3. Implement the `dagon-port` traits for your board (`Flash`, `Link`,
   `Reset`, `Watchdog`, and `BootSelectS` or `BootSelectV`) and a
   `dagon_agent::SlotStore`, compose the app layer's chain (your
   extensions answer the decision points and observe the events, see
   `docs/events.md`), then start the agent with your link:

   ```rust
   let chain = dagon_events::compose(Layer::App, events, board_exts, user_exts, bounds);
   let cfg = dagon_agent::Config { pubkey, mcu_id, port_kind, target_tag, role, mode, max_payload };
   let mut agent = dagon_agent::Agent::new(cfg, store, chain);
   agent.run(&mut link, &mut reset).await;                    // serves requests forever
   ```

   Feed the watchdog while confirmed, and only for a limited time while on
   trial, so an image that is never confirmed is reset and rolled back.
4. Check the port with `dagon conform`.

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

- Everything in [Prerequisites](#prerequisites), plus the
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
  artifacts (it publishes nothing). Versioning rules: docs/versioning.md

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
  only; nothing from it ships in DAGON Console.
- DAGON Console ships the Svelte client runtime and the two packages it
  imports, `clsx` and `esm-env` (all MIT); every other npm package is a
  build or test tool (column "Ships in").
- Fonts in `web/public/fonts/` are under the SIL Open Font License 1.1:
  Doto, DotGothic16, Zen Kaku Gothic New and JetBrains Mono (licence texts
  next to the files; see `web/README.md`).
- The hub display's Japanese state words use bitmaps rasterised from Noto
  Sans JP (SIL Open Font License 1.1; licence in
  `targets/boards/waveshare-rp2040-touch-lcd-1.28/fonts/`).

Rust crates: 316 packages, 350 name/version pairs. "Used by": `crates/` is the host workspace, `samples/...` the sample apps, other names are firmware packages under `targets/`.

| Crate | Versions | Licence | Used by |
|---|---|---|---|
| aho-corasick | 1.1.5 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| aligned | 0.4.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| allocator-api2 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| as-slice | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| ascii-canvas | 4.0.0 | Apache-2.0/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| az | 1.2.1, 1.3.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bare-metal | 0.2.5 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| base64 | 0.23.1 | MIT OR Apache-2.0 | crates/ |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| bit_field | 0.10.3 | Apache-2.0/MIT | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| bitfield | 0.13.2, 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| bitflags | 1.3.2, 2.13.2 | MIT OR Apache-2.0 / MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| block-buffer | 0.10.4, 0.12.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| block-device-driver | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates/ |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| byteorder | 1.5.0 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| bytes | 1.12.1 | MIT | crates/ |
| cc | 1.10.0, 1.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| codespan-reporting | 0.11.1 | Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| const-default | 1.0.0 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | crates/ |
| cordyceps | 0.3.5 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | crates/ |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | crates/ |
| cortex-m | 0.7.10 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| cortex-m-macros | 0.7.8 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| cortex-m-rt | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| cortex-m-rt-macros | 0.7.7 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| cpufeatures | 0.2.17, 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| crc-any | 2.5.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| crunchy | 0.2.4 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| crypto-common | 0.1.7, 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| darling | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| darling_core | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| darling_macro | 0.20.11, 0.21.3, 0.24.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| debug-helper | 0.3.14 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| defmt | 0.3.101, 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| defmt-rtt | 1.3.1 | MIT OR Apache-2.0 | f401-bringup, samples/f401-app |
| delegate | 0.13.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| digest | 0.10.7, 0.11.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| document-features | 0.2.12 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ed25519-compact | 2.6.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| either | 1.19.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-hal-internal | 0.4.0, 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| embassy-net-driver-channel | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-rp | 0.10.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-stm32 | 0.6.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| embassy-sync | 0.6.2, 0.7.2, 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, f401-bringup, mcu/esp32s3, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-usb | 0.6.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| embassy-usb-driver | 0.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embassy-usb-synopsys-otg | 0.3.3, 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| embedded-hal | 0.2.7, 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-hal-nb | 1.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| embedded-io | 0.6.1, 0.7.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-io-async | 0.6.1, 0.7.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ena | 0.14.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| encode_unicode | 0.3.6 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| enumset | 1.1.15 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| enumset_derive | 0.15.1 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| esp-alloc | 0.11.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-bootloader-esp-idf | 0.6.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-config | 0.8.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal | 1.2.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-hal-co5300 | 0.1.0 | MIT OR Apache-2.0 | samples/esp32s3-app |
| esp-hal-procmacros | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-metadata-generated | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-riscv-rt | 0.15.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-rom-sys | 0.1.5 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-rtos | 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-storage | 0.10.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp-sync | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32 | 0.41.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c2 | 0.30.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c3 | 0.33.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c5 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c6 | 0.24.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32c61 | 0.4.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32h2 | 0.20.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32p4 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s2 | 0.32.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s3 | 0.36.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| esp32s31 | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| fixed | 1.29.0, 1.31.0 | MIT/Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| float-cmp | 0.9.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| fnv | 1.0.7 | Apache-2.0 / MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| fugit | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/esp32s3-app, samples/f401-app |
| gcd | 2.3.0 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| generator | 0.8.11 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| generic-array | 0.14.7 | MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| getrandom | 0.2.17, 0.4.3 | MIT OR Apache-2.0 | crates/ |
| half | 2.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| heapless | 0.8.0, 0.9.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| heck | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| http | 1.5.0 | MIT OR Apache-2.0 | crates/ |
| httparse | 1.10.1 | MIT OR Apache-2.0 | crates/ |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| ident_case | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| indoc | 2.0.8 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| instability | 0.3.14 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| io-kit-sys | 0.4.1 | MIT / Apache-2.0 | crates/ |
| itertools | 0.14.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| itoa | 1.0.18 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| jiff | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-core | 0.1.1 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| jiff-static | 0.2.38 | Unlicense OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| js-sys | 0.3.107 | MIT OR Apache-2.0 | crates/ |
| keccak | 0.1.6 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lalrpop | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lalrpop-util | 0.22.2 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| libc | 0.2.191 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| libyaml-rs | 0.3.0 | MIT | crates/ |
| linked_list_allocator | 0.10.6 | Apache-2.0/MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| litrs | 1.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| log | 0.4.35 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| loom | 0.7.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| mach2 | 0.4.3 | BSD-2-Clause OR MIT OR Apache-2.0 | crates/ |
| matchers | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| memchr | 2.8.3 | Unlicense OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| micromath | 2.1.0 | Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, rp2040-hub |
| nb | 0.1.3, 1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| new_debug_unreachable | 1.0.6 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| nix | 0.26.4 | MIT | crates/ |
| nu-ansi-term | 0.50.3 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| object | 0.37.3 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| panic-halt | 1.0.0 | MIT OR Apache-2.0 | mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump |
| panic-probe | 1.0.0 | MIT OR Apache-2.0 | f401-bringup, samples/f401-app |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| paste | 1.0.15 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | crates/ |
| petgraph | 0.7.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| phf_shared | 0.11.3 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pico-args | 0.5.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| pio | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-core | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-parser | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| pio-proc | 0.3.0 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| precomputed-hash | 0.1.1 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| proc-macro-error-attr2 | 2.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro-error2 | 2.0.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| quote | 1.0.47 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | crates/ |
| rand_core | 0.10.1, 0.6.4, 0.9.5 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| redox_syscall | 0.5.18 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| regex | 1.13.1 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/f401-app |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| rgb | 0.8.53 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| ring | 0.17.14 | Apache-2.0 AND ISC | crates/ |
| riscv | 0.15.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-pac | 0.2.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt | 0.16.0 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| riscv-target-parser | 0.1.3 | ISC | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rlsf | 0.2.3 | MIT/Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| rp-pac | 7.0.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| rp2040-boot2 | 0.3.0 | BSD-3-Clause | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, rp2040-bringup, rp2040-dump, rp2040-hub |
| rustc_version | 0.2.3 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | crates/ |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | crates/ |
| rustls-webpki | 0.103.16 | ISC | crates/ |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| same-file | 1.0.6 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sdio | 0.5.3 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| sdio-host | 0.9.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| semver | 0.9.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| semver-parser | 0.7.0 | MIT/Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| serde | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, crates/, mcu/esp32s3, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | crates/ |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates/ |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| serialport | 4.10.1 | MPL-2.0 | crates/ |
| sha1 | 0.11.0 | MIT OR Apache-2.0 | crates/ |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| sha2-const-stable | 0.1.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sha3 | 0.10.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| sharded-slab | 0.1.7 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| shlex | 2.0.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| slab | 0.4.12 | MIT | crates/ |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| smart-leds | 0.4.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| smart-leds-trait | 0.3.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| somni-expr | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-parser | 0.3.1 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| somni-template | 0.3.2 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| ssmarshal | 1.0.0 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| static_cell | 2.1.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| stm32-fmc | 0.4.0 | MIT/Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| stm32-metapac | 21.0.0 | MIT OR Apache-2.0 | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| string_cache | 0.8.9 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| strsim | 0.11.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| strum | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| strum_macros | 0.27.2 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| subtle | 2.6.1 | BSD-3-Clause | crates/ |
| syn | 1.0.109, 2.0.119, 3.0.9 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| term | 1.2.1 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| termcolor | 1.4.1 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, crates/, f401-bringup, mcu/esp32s3, mcu/stm32f401re, samples/esp32s3-app, samples/f401-app |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tokio | 1.54.1 | MIT | crates/ |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates/ |
| toml_datetime | 0.6.11, 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_edit | 0.22.27, 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates/ |
| tracing | 0.1.44 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-attributes | 0.1.31 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-core | 0.1.36 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-log | 0.2.0 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| tracing-subscriber | 0.3.23 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| trait-set | 0.3.0 | MIT | boards/nucleo-f401re, f401-bringup, mcu/stm32f401re, mcu/stm32f401re/bootcore, samples/f401-app |
| typenum | 1.20.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-testimg, mcu/common, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| unescaper | 0.1.10 | MIT OR GPL-3.0-only | crates/ |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| unicode-width | 0.1.14 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| unsafe-libyaml | 0.2.11 | MIT | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| untrusted | 0.9.0 | ISC | crates/ |
| ureq | 3.5.0 | MIT OR Apache-2.0 | crates/ |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | crates/ |
| usb-device | 0.3.2 | MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid-descriptors | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| usbd-hid-macros | 0.10.2 | MIT OR Apache-2.0 | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, rp2040-bringup, rp2040-dump, rp2040-hub |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | crates/ |
| valuable | 0.1.1 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| vcell | 0.1.3 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| version_check | 0.9.5 | MIT/Apache-2.0 | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| void | 1.0.2 | MIT | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/esp32s3-app, samples/f401-app |
| volatile-register | 0.2.2 | MIT OR Apache-2.0 | boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/rp2040, mcu/rp2040/bootcore, mcu/rp2040/stage0, mcu/stm32f401re, mcu/stm32f401re/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, rp2040-hub/uf2check, samples/f401-app |
| walkdir | 2.5.0 | Unlicense/MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | crates/ |
| wasm-bindgen | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-futures | 0.4.80 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-macro-support | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| wasm-bindgen-shared | 0.2.130 | MIT OR Apache-2.0 | crates/ |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | crates/ |
| winapi-util | 0.1.11 | Unlicense OR MIT | boards/m5stopwatch, boards/waveshare-rp2040-touch-lcd-1.28, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-sys | 0.52.0, 0.61.2 | MIT OR Apache-2.0 | boards/m5stopwatch, boards/nucleo-f401re, boards/waveshare-rp2040-touch-lcd-1.28, crates/, f401-bringup, mcu/esp32s3, mcu/rp2040, mcu/rp2040/bootcore, mcu/stm32f401re, rp2040-bringup, rp2040-dump, rp2040-hub, samples/esp32s3-app, samples/f401-app |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | crates/ |
| winnow | 0.7.14, 1.0.4 | MIT | boards/m5stopwatch, crates/, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt | 0.23.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 | boards/m5stopwatch, mcu/esp32s3, samples/esp32s3-app |
| yaml_serde | 0.10.7 | MIT OR Apache-2.0 | crates/ |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | boards/waveshare-rp2040-touch-lcd-1.28, mcu/rp2040, mcu/rp2040/bootcore, rp2040-bringup, rp2040-dump, rp2040-hub |
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
  (`targets/esp32s3-2ndstage`: only the project files and the sdkconfig
  options in `vendor/sdkconfig.defaults` are ours). Bundled unmodified as
  `targets/mcu/esp32s3/vendor/bootloader.bin` with its provenance and licence
  (`vendor/PROVENANCE.md`, `vendor/LICENSE-ESP-IDF`).
- `esp-bootloader-esp-idf` (crate, MIT OR Apache-2.0): used unmodified by
  the ESP32-S3 port to read and write otadata, which is how a slot is
  selected for that 2nd stage.
- RP2040 boot2: the generic 03h second-stage loader from the `rp2040-boot2`
  crate (BSD-3-Clause), placed unmodified at the start of flash by
  `targets/mcu/rp2040/stage0`.
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
