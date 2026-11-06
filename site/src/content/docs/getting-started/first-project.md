---
title: "From a new project to an update"
description: "Start a project, draw its topology, build, write the first version, then update and go back."
---

The example is the sample: an RP2040 hub with a round LCD, a
NUCLEO-F401RE (I2C) and an M5StopWatch (UART). What their sample apps do
is in `samples/f401-app` and `samples/esp32s3-app`. Replace it with your
own topology at step 2.

## 1. Start a project

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new` creates `dagon.toml`, `topology.yaml` (the sample, or your
own with `--from <yaml>`), `.gitignore`, the Claude Code files (below) and,
if none exists yet, a signing key pair in `~/.dagon/keys/my-fleet` (or
`--keys <dir>`). The secret key stays outside the project and outside any
git work tree; `dagon.toml` only names the public key. Tell the CLI where
the secret key is when you sign:

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

## 2. Define the topology in DAGON Console

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
dagon ai key set                    # the API key, typed without echo, into the OS credential store
dagon ai link --way api             # consent to send, asked once
dagon ai topology --in boards.txt --out topology.yaml
```

The API key lives only in the OS credential store (Windows Credential
Manager, macOS Keychain, or a Secret Service such as GNOME Keyring on Linux),
never in an environment variable or a file. `dagon ai key status` shows
whether it is registered and `dagon ai key delete` removes it; DAGON Console
registers it too (CLI 連携). Without a credential store, use Claude Code with
your own login instead (`dagon ai link`). See [Agent link: serve API](/reference/agent-api/).

Definitions are not a place for secrets. Names in them (ids, product
names, pin labels, partition names) appear in DAGON Console and are sent
to Claude when you ask it; commands, hints, comments and file paths are
never sent. See [MCU and board definitions](/reference/definitions/).

## 3. Generate the node crates

```sh
dagon gen
```

One crate per node in `nodes/<id>/`. Write your application in
`nodes/<id>/src/app.rs`: a task that does the work and a `self_check` that
says whether a version on trial may be confirmed. `dagon gen` can run
again any time the topology changes: it rewrites `src/wiring.rs` and the
DAGON package lines of `Cargo.toml`, and never touches `app.rs` (nor
`main.rs`, unless `--force-main`). DAGON Console does the same with
"プロジェクトを作る". `dagon new` and `dagon gen` never write
or remove anything through a symbolic link or a junction: a node whose
`nodes/<id>` (or `nodes`, or any file gen writes) is one is skipped with
the reason, and `dagon gen` then exits with an error.

## 4. Build

```sh
dagon build            # or: dagon build seg
```

Each node is built into `out/build/<id>/app-a.elf` and, for slot B,
`app-b.elf` (an ESP32-S3 image serves both slots, so it has one), with the
public key from `dagon.toml` embedded. A node's version is the `version`
in its crate's `Cargo.toml` (its boot core's, in its boot crate's
`Cargo.toml`); raise it there for every change you write. The first write
signs with the same versions. `--version` is not needed; if given, it must
be the same or the command stops and says which. A node's boot core goes into the
package with its app, and the node writes it only when its version differs
from the boot core it runs: a changed boot core is written once its version
is raised, as with apps. The build brings the crate's own entry in its
`Cargo.lock` to the raised version and changes nothing else there; every
other package stays as locked. `dagon gen` keeps the boot crate's
`version`: it is the one field of that file that is yours.

## 5. Write the first version (once per node)

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
with the public half of your signing key. The boot cores accept the sample's
tags (`hub-rp2040-lcd`, `seg-f401-7seg`), so keep those tags in
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

Each node's line `versions:` names its stage0, boot core slots A/B (and the
running one), app slots, wire version and MCU id; "unknown" is a node that
cannot tell (an agent or stage0 built before it reported them). With the hub
connected, `dagon doctor` prints the same lines after its tool checks.

Without `dagon flash` (boot images you built yourself): `dagon rescue hub`
and `dagon provision <id>` make the same files from `[provision.<id>]`
tables in `dagon.toml` (`stage0` for the hub, `bootcore`, `app`,
`version`); the MCU package READMEs show how to build the boot images.

## 6. Update, and go back

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
(`dagon --help`). In DAGON Console, 「ビルドする」 in 準備 builds every node,
signs and makes the package; in 書込 you check the nodes on their cards,
pick the version in the header (the latest build, or a kept one to go
back) and 「書き込む」 writes it over Web Serial. Each card shows its
current → new version and one % for the whole write, then the result.

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
dagon deploy seg              # also a node that no longer answers: says so, asks (`--yes` answers), then rewrites it through its ROM loader, or names `dagon flash` when it has none
```

`dagon --help` lists every command and option.
