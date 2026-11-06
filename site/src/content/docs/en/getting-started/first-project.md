---
title: "From a new project to a new version"
description: "Start a project, draw its topology, build, write the first version, then write a new one and go back."
---

The example on this page is the sample topology: three nodes, an RP2040 hub with a round LCD, a NUCLEO-F401RE (I2C) and an M5StopWatch (UART). You can replace it with your own topology at step 2.

What the children's sample apps do is in `samples/f401-app` and `samples/esp32s3-app`.

Set up the `dagon` command and the tools first, as in [Install](/en/getting-started/install/).

## 1. Start a project

```sh
dagon new my-fleet
cd my-fleet
```

`dagon new` creates:

- `dagon.toml`
- `topology.yaml`: the sample topology, or your own with `--from <yaml>`.
- `.gitignore`
- The Claude Code files ([Claude help in your project](/en/how-to/claude-help/))
- A signing key pair, if none exists yet: in `~/.dagon/keys/my-fleet`, or elsewhere with `--keys <dir>`.

The secret key stays outside the project and outside any git work tree. `dagon.toml` only names the public key.

Tell `dagon` where the secret key is when you sign:

```sh
export DAGON_SIGNING_KEY=~/.dagon/keys/my-fleet/dagon-signing.key
```

## 2. Make the topology

### In DAGON Console

```sh
dagon serve
```

DAGON Console opens in the browser, on 127.0.0.1 only, with a one-time passphrase. Without a project, it starts on a screen that creates or opens one.

1. Place nodes from the node list, by product or by MCU.
2. Link them.
3. Fix what the check reports.

Every edit is saved to `topology.yaml`.

### Checking from the command line

```sh
dagon topo check topology.yaml
```

### From a description

A plain-language description can give a first draft through the Claude API:

```sh
dagon ai key set                    # the API key, typed without echo, into the OS credential store
dagon ai link --way api             # consent to send, asked once
dagon ai topology --in boards.txt --out topology.yaml
```

The API key lives only in the OS credential store, never in an environment variable or a file:

- Windows: Credential Manager
- macOS: Keychain
- Linux: a Secret Service such as GNOME Keyring

`dagon ai key status` shows whether it is registered and `dagon ai key delete` removes it. DAGON Console registers it too (CLI 連携).

Without a credential store, use Claude Code with your own login instead (`dagon ai link`). See [Agent link: serve API](/en/reference/agent-api/).

### No secrets in definitions

Names in definitions (ids, product names, pin labels, partition names) appear in DAGON Console, and are sent to Claude when you ask it. Commands, hints, comments and file paths are never sent. See [MCU and board definitions](/en/reference/definitions/).

## 3. Generate the node crates

```sh
dagon gen
```

Each node gets a crate in `nodes/<id>/`. DAGON Console does the same with 「プロジェクトを作る」.

Write your application in `nodes/<id>/src/app.rs`. It has two things:

- A task that does the work
- A `self_check` that says whether a version on trial may be confirmed

`dagon gen` can run again any time the topology changes:

- It rewrites only `src/wiring.rs` and the DAGON crate lines of `Cargo.toml`.
- It never touches `app.rs`, nor `main.rs` unless `--force-main`.

`dagon new` and `dagon gen` never write or remove anything through a symbolic link or a junction. A node whose `nodes/<id>` (or `nodes`, or any file `dagon gen` writes) is one is skipped with the reason, and `dagon gen` then exits with an error.

## 4. Build

```sh
dagon build            # or: dagon build seg
```

Each node is built into `out/build/<id>/app-a.elf` and, for slot B, `app-b.elf`. An ESP32-S3 image serves both slots, so it has one. The public key from `dagon.toml` is embedded.

### Choosing the version

A node's version is the `version` in its crate's `Cargo.toml`. Its boot loader's version is the `version` in its boot crate's `Cargo.toml`. Raise it there for every change you write.

- The first write signs with the same versions.
- `--version` is not needed. If given, it must be the same, or the command stops and says which differs.
- The build brings the crate's own entry in its `Cargo.lock` to the raised version. Every other crate stays as locked.

### The boot loader

A node's boot loader goes into the package with its app. It is written only when its version differs from the boot loader the node runs. So raise its version when you change a boot loader.

`dagon gen` keeps the boot crate's `version`. It is the one field of that file that is yours, and raising it needs no new `dagon gen`.

### Seeing a node refuse a bad boot loader

Build a boot loader that fails its own check:

1. Add `boot_features = ["no-confirm"]` to `[build.<id>]`. `boot_features` names features of the node's boot crate. `dagon deploy` and DAGON Console build the package's boot loader with them; a first write never does.
2. Raise the boot crate's version and write. The write stops, and the node keeps running its confirmed boot loader with its app as it was.
3. Take the line out afterwards, or the next write sends a failing boot loader again.

## 5. Write the first version (once per node)

Write the boot loader and first application once, over the node's own cable. With `DAGON_SIGNING_KEY` set (step 1), connect the node and run:

```sh
dagon flash hub        # RP2040: hold BOOT while plugging in USB; rescue.uf2 is copied to the RPI-RP2 drive
dagon flash seg        # STM32F401RE: through the ST-LINK with probe-rs
dagon flash watch      # ESP32-S3: over USB-C with espflash
```

`dagon flash` prints its plan first and asks before it touches the board. `--yes` skips the question.

### Which device it writes to

It chooses among the devices the writer reports: every probe-rs probe, every serial port, every UF2 drive. USB IDs and the UF2 Board-ID only label them and put the matching ones first.

- One device: it is taken and shown before the question.
- Several devices: it asks which. Enter takes the first match, if any.
- Naming one: `--probe <vid:pid[:serial]>`, `--port <port>` or `--drive <path>`. On Windows these compare without regard to case, so `--drive e:` names `E:\`.
- It stops before writing with no device at all, or with several devices and no terminal (or `--yes`) and no option.

### What gets built

`dagon flash` builds the boot loader (and the hub's `stage0`) from the project's DAGON source (`[dagon]` in `dagon.toml`), with the node's app, all with the public half of your signing key.

The boot loaders accept the sample's tags (`hub-rp2040-lcd`, `seg-f401-7seg`), so keep those tags in `topology.yaml` for these boards.

### Trying it without touching the board

`--dry-run` does everything up to the write. It builds the parts and the app and makes the first-write files in the provision folder, and only leaves the board untouched.

The provision folder is `out/provision/<id>/` beside `dagon.toml`, or under `provision_dir` when `dagon.toml` sets it. `--out <dir>` chooses another.

### An ESP32-S3 node

`dagon flash` builds the board's factory agent and the app. It turns the app into an ESP image (`espflash save-image --chip esp32s3 --flash-size 16mb`), signs it, and writes these with `espflash`:

- The bundled unmodified ESP-IDF 2nd stage (0x0): refused unless its SHA-256 matches `targets/mcu/esp32s3/vendor/SHA256SUMS`.
- The partition table (`targets/mcu/esp32s3/partitions.csv`, 0x8000)
- The factory agent
- The app (ota_0, 0x110000)
- The app's signed header (the `dagon` partition, 0x710000)
- The otadata entry that starts it as confirmed (0xD000)

`--dry-run` lists the files with their offsets and the `espflash` commands (`commands.txt` in the same provision folder).

### Checking that nodes answer

Connect the hub and check that every node answers:

```sh
dagon status
```

Each node's `versions:` line names:

- Its stage0 (the small program that runs first after a reset)
- Its boot loader slots A/B and the running one
- Its app slots
- Its wire version
- Its MCU id

`unknown` is a node that cannot tell (an agent or stage0 built before it reported them). With the hub connected, `dagon doctor` prints the same lines after its tool checks.

### Without `dagon flash`

For boot images you built yourself, `dagon rescue hub` and `dagon provision <id>` make the same files. They read `[provision.<id>]` in `dagon.toml` (`stage0` for the hub, `bootcore`, `app`, `version`). The MCU definition READMEs show how to build the boot images.

## 6. Write a new version, and go back

### From the command line

Change your code, then run:

```sh
dagon deploy                       # or: dagon deploy seg watch
```

`dagon deploy` goes in this order:

1. It builds slots A and B and signs them.
2. It makes the package.
3. It lists each node with its current and new version and asks before writing. `--yes` skips the question.
4. It writes.

The versions it shows:

- Current: the node's version in the last package (`out/release-latest.dgb`, or `release` in `dagon.toml`), once that package verifies under your key. It is not read from the boards.
- New: each node's crate version.

Without node ids it takes every node in `[build.<id>]`.

An ESP32-S3 node is built once, as its image runs from either OTA slot. `dagon deploy` turns the build into an ESP image (`espflash save-image`) and signs that, and the hub relays it like any other node's.

The single steps are `dagon build`, `dagon image sign`, `dagon release build` and `dagon apply` (`dagon --help`).

### In DAGON Console

1. In 準備, press 「ビルドする」. It builds every node, signs and makes the package.
2. In 書込, check the nodes to write on their cards.
3. Pick the version in the header: the latest build, or a kept one to go back.
4. Press 「書き込む」. It writes over Web Serial.

Each card shows its current → new version and one % for the whole write, then the result.

### What a write does

1. It checks the signatures.
2. It checks that the package was built for this topology.
3. It sends the topology to the hub.
4. It writes the children and then the hub.
5. A node runs the new version on trial. If it is not confirmed, its watchdog resets it and it boots the previous version again.

### Going back

Every package written to nodes (by DAGON Console, `dagon apply` or `dagon deploy`) is kept in the project's `out/deploy/` folder. Its name is `v<version>.dgb`, or `v<newest>-<digest>.dgb` when its nodes get different versions.

To go back, write a kept older version again. DAGON Console lists them, and `dagon apply out/deploy/v1.0.0.dgb` writes one from the command line.

`dagon rollback` goes back to the previously confirmed version:

```sh
dagon rollback --node seg
```

### A node that no longer answers

`dagon deploy` also takes a node that no longer answers:

```sh
dagon deploy seg
```

It says so, asks (`--yes` answers), then writes the node through its ROM loader. For an MCU with no ROM loader it names `dagon flash`.

`dagon --help` lists every command and option.
