---
title: Command line (dagon)
description: Every dagon command, from its clap definitions.
editUrl: false
---

:::note
Generated from `crates/dagon-cli/src/cli.rs`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

Every `dagon` command, its arguments and options, as `dagon <command> --help`
prints them. Most commands take their defaults from `./dagon.toml` (or
`--config <file>`).

## dagon

```text
DAGON Boot command line tool

Usage: dagon
       dagon <COMMAND>

Commands:
  key        Make or show the signing key pair
  topo       Check, compile or send a topology
  image      Sign firmware images
  release    Build a signed release package (.dgb)
  provision  Make the first-install files of a node from [provision.<node>] in dagon.toml
  rescue     Make the hub's rescue UF2 (boot2, stage0, logs, boot core and app)
  status     Show HELLO, STATUS and VERSION of the hub and each topology node
  apply      Write a release package to the nodes
  conform    Run the 12 conformance tests on a node
  recover    Rewrite a node from the release
  rollback   Go back to the node's previously confirmed version
  new        Make a project folder
  gen        Make one Rust crate per node in nodes/<id> on the board support packages
  build      Build each node of [build.<id>] in dagon.toml
  deploy     Build, sign, package and write the nodes in one go
  flash      First write of a node, as its MCU definition says
  doctor     Check the tools each MCU family of the topology needs
  serve      Start DAGON Console, the browser tool, and its API on 127.0.0.1
  sim        Run the simulator
  demo       DAGON Console against a simulated hub with two child nodes
  ai         Work with an AI agent
  help       Print this message or the help of the given subcommand(s)

Options:
  -V, --version
          Print the version and the format numbers this dagon reads and writes

  -h, --help
          Print help

Most commands take their defaults from ./dagon.toml (or --config <file>).
The hub is found by USB ID c0de:cafe or product "DAGON hub" unless --port is given.
The secret key file is --key <file> or $DAGON_SIGNING_KEY and must lie outside any git
work tree.

`dagon <command> --help` describes a command and each of its options.
```

## dagon key

```text
Make or show the signing key pair

Usage: dagon key <COMMAND>

Commands:
  gen   Generate a signing key pair
  pub   Print the public key (hex) and key id of the signing key
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## dagon key gen

```text
Generate a signing key pair

Usage: dagon key gen --out <DIR>

Options:
      --out <DIR>
          Folder for the key pair; must lie outside any git work tree

  -h, --help
          Print help
```

## dagon key pub

```text
Print the public key (hex) and key id of the signing key

Usage: dagon key pub [OPTIONS]

Options:
      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

  -h, --help
          Print help
```

## dagon topo

```text
Check, compile or send a topology

Usage: dagon topo <COMMAND>

Commands:
  check    Validate a topology file
  compile  Write the topology's canonical binary
  push     Send the topology's canonical binary to the hub
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## dagon topo check

```text
Validate a topology file

Usage: dagon topo check [OPTIONS] <YAML>

Arguments:
  <YAML>
          Topology YAML

Options:
      --json
          Print the report as JSON

  -h, --help
          Print help
```

## dagon topo compile

```text
Write the topology's canonical binary

Usage: dagon topo compile [OPTIONS] <YAML>

Arguments:
  <YAML>
          Topology YAML

Options:
      --out <BIN>
          Output file [default: the YAML's name with .bin]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help
```

## dagon topo push

```text
Send the topology's canonical binary to the hub

Usage: dagon topo push [OPTIONS] [YAML]

Arguments:
  [YAML]
          Topology YAML [default: topology in dagon.toml]

Options:
      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help
```

## dagon image

```text
Sign firmware images

Usage: dagon image <COMMAND>

Commands:
  sign  Build and sign an image for a topology node
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## dagon image sign

```text
Build and sign an image for a topology node

An input that already is a signed image is verified and checked to belong to the node, kind and slot instead.

Usage: dagon image sign [OPTIONS] --node <ID> --version <X.Y.Z> --out <IMG> <ELF|BIN>

Arguments:
  <ELF|BIN>
          ELF or raw binary to sign

Options:
      --node <ID>
          Node id in the topology

      --version <X.Y.Z>
          Version in the image header

      --out <IMG>
          Output image file

      --slot <SLOT>
          Slot the image is built for: a or b
          
          [default: a]

      --kind <KIND>
          Image kind: app, bootcore or agent
          
          [default: app]

      --seq <N>
          Release sequence number
          
          [default: 1]

      --min-bootcore <X.Y.Z>
          Oldest boot core version the image runs on [default: 0.0.0]

      --proto <N>
          Protocol version
          
          [default: 1]

      --flags <LIST>
          Build flags, comma-separated [default: the kind's]: verify-enforced, debug, sim-only

      --caps <LIST>
          Capabilities, comma-separated [default: the kind's]: confirm-by-hub, reboot-to-update, rom-entry-coop, sig-verifier

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon release

```text
Build a signed release package (.dgb)

Usage: dagon release <COMMAND>

Commands:
  build  Check the images, sign the manifest and write the release package
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## dagon release build

```text
Check the images, sign the manifest and write the release package

Usage: dagon release build [OPTIONS] --name <NAME> <IMG|DIR>...

Arguments:
  <IMG|DIR>...
          Signed images, or folders whose .img files are taken

Options:
      --name <NAME>
          Release name in the manifest

      --out <DGB>
          Output package [default: release in dagon.toml]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --topology-bin <BIN>
          Topology binary to package [default: compiled from the topology]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help
```

## dagon provision

```text
Make the first-install files of a node from [provision.<node>] in dagon.toml

Usage: dagon provision [OPTIONS] <NODE>

Arguments:
  <NODE>
          Node id in the topology

Options:
      --out <DIR>
          Output folder [default: <provision_dir>/<node>]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help
```

## dagon rescue

```text
Make the hub's rescue UF2 (boot2, stage0, logs, boot core and app)

Usage: dagon rescue [OPTIONS] <TARGET>

Arguments:
  <TARGET>
          What to rescue (the topology's coordinator)
          
          [possible values: hub]

Options:
      --out <DIR>
          Output folder [default: <provision_dir>/<node>]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help
```

## dagon status

```text
Show HELLO, STATUS and VERSION of the hub and each topology node

Versions of stage0, boot core, app, wire format and MCU.

Usage: dagon status [OPTIONS]

Options:
      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon apply

```text
Write a release package to the nodes

Verifies the release, checks it matches the topology, sends the topology to the hub, then updates the children and then the hub.

Usage: dagon apply [OPTIONS] [DGB]

Arguments:
  [DGB]
          Release package [default: release in dagon.toml]

Options:
      --node <ID>
          Update only this node (repeat for more) [default: every node]

      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --pubkey <FILE>
          Public key file the release is verified with [default: pubkey in dagon.toml, else the public half of the signing key]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --no-host-verify
          Leave signature and digest checks to the nodes (for demonstration only)

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon conform

```text
Run the 12 conformance tests on a node

Runs through the hub, or with --usb on a node plugged in directly. The images folder holds good-a.img, good-b.img, no-confirm-a.img and no-confirm-b.img. Writes result.json and report.html into --out, else into the images folder (default for both: out/conform/<id> next to dagon.toml).

Usage: dagon conform [OPTIONS] --node <ID>

Options:
      --node <ID>
          Node id in the topology

      --usb
          Test a node plugged in directly over USB instead of through the hub

      --images <DIR>
          Folder of the test images [default: out/conform/<id>]

      --out <DIR>
          Folder for result.json and report.html [default: the images folder]

      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon recover

```text
Rewrite a node from the release

Uses the normal transfer if the node answers, else the MCU's ROM loader (boot core from <provision_dir>/<node>/bootcore.img by default).

Usage: dagon recover [OPTIONS] <NODE>

Arguments:
  <NODE>
          Node id in the topology

Options:
      --release <DGB>
          Release package [default: release in dagon.toml]

      --bootcore <IMG>
          Signed boot core image for the ROM path [default: <provision_dir>/<node>/bootcore.img]

      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --pubkey <FILE>
          Public key file the release is verified with [default: pubkey in dagon.toml, else the public half of the signing key]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon rollback

```text
Go back to the node's previously confirmed version

Boots the node's other slot and confirms it there.

Usage: dagon rollback [OPTIONS] --node <ID>

Options:
      --node <ID>
          Node id in the topology

      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon new

```text
Make a project folder

Writes dagon.toml and topology.yaml (the sample unless --from), and a signing key pair in --keys if none is there. The project takes DAGON from this CLI's release (git and tag cli-v<version>) unless --dagon-path or DAGON_HOME names a local clone.

Usage: dagon new [OPTIONS] <DIR>

Arguments:
  <DIR>
          Project folder to make; its name is the project's

Options:
      --from <YAML>
          Topology YAML to start from [default: the sample topology]

      --keys <DIR>
          Folder of the signing key pair [default: ~/.dagon/keys/<name>]

      --dagon-path <CLONE>
          Local clone of DAGON to build with [default: $DAGON_HOME]

      --dagon-git <URL>
          Git URL of DAGON [default: this CLI's repository]

      --dagon-tag <TAG>
          Git tag of DAGON [default: cli-v<this CLI's version>]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon gen

```text
Make one Rust crate per node in nodes/<id> on the board support packages

Rewrites only wiring.rs and the package line, never app.rs (main.rs only with --force-main).

Usage: dagon gen [OPTIONS] [ID]...

Arguments:
  [ID]...
          Node ids [default: every node]

Options:
      --force-main
          Also rewrite main.rs

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon build

```text
Build each node of [build.<id>] in dagon.toml

Writes out/build/<id>/app-a.elf (and app-b.elf).

Usage: dagon build [OPTIONS] [ID]...

Arguments:
  [ID]...
          Node ids [default: every [build.<id>] of dagon.toml]

Options:
      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon deploy

```text
Build, sign, package and write the nodes in one go

Each node's version is its crate's Cargo.toml version. Lists node and version and asks first; keeps the package in out/deploy. Each node's boot core comes from its boot crate and is written (before its app) only when its version differs from the running boot core's.

Usage: dagon deploy [OPTIONS] [ID]...

Arguments:
  [ID]...
          Node ids [default: every [build.<id>] of dagon.toml]

Options:
      --version <X.Y.Z>
          Version to deploy; must equal each node crate's version

      --yes
          Write without asking

      --bootcore
          Refuse a node with boot core slots that has no boot crate

      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon flash

```text
First write of a node, as its MCU definition says

Writes through a UF2 drive, probe-rs or espflash. Builds the parts (boot core, stage0, factory agent) with the key and prints the plan first. Writes to the one device the writer reports, or the one you choose or name.

Usage: dagon flash [OPTIONS] <NODE>

Arguments:
  <NODE>
          Node id in the topology

Options:
      --dry-run
          Make the files and print the plan without writing

      --yes
          Write without asking

      --probe <VID:PID[:SERIAL]>
          Debug probe to write through (probe-rs writers)

      --port <PORT>
          Serial port to write through (espflash writers)

      --drive <PATH>
          UF2 drive to copy to (UF2 writers)

      --out <DIR>
          Folder for the files [default: <provision_dir>/<node>]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon doctor

```text
Check the tools each MCU family of the topology needs

Then shows each node's versions when a hub is connected.

Usage: dagon doctor [OPTIONS]

Options:
      --fix
          Install what needs no administrator rights

      --port <NAME>
          Serial port of the hub [default: the USB device c0de:cafe or named "DAGON hub"]

      --topology <YAML>
          Topology YAML [default: topology in dagon.toml, else every MCU family]

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon serve

```text
Start DAGON Console, the browser tool, and its API on 127.0.0.1

Opens it with a one-time passphrase. The Console verifies releases with the public key given here (--pubkey, pubkey in dagon.toml, or the signing key's public half).

Usage: dagon serve [OPTIONS]

Options:
      --port <N>
          TCP port on 127.0.0.1 [default: a free port]

      --web <DIR>
          Folder of the Console page to serve [default: the page built into dagon]

      --pubkey <FILE>
          Public key file the release is verified with [default: pubkey in dagon.toml, else the public half of the signing key]

      --key <FILE>
          Secret signing key file, outside any git work tree [default: $DAGON_SIGNING_KEY]

      --no-open
          Do not open the browser; the address and passphrase are printed

      --config <FILE>
          Project file the defaults come from [default: ./dagon.toml]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon sim

```text
Run the simulator

Usage: dagon sim <COMMAND>

Commands:
  run   Run the simulator scenarios (no hardware)
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## dagon sim run

```text
Run the simulator scenarios (no hardware)

Usage: dagon sim run

Options:
  -h, --help
          Print help
```

## dagon demo

```text
DAGON Console against a simulated hub with two child nodes

Uses a demo key made for it: try updates without boards.

Usage: dagon demo [OPTIONS]

Options:
      --sim-speed <X>
          Simulation speed factor, 0.1 to 20
          
          [default: 1]

      --sim-fail <ID>
          Child node whose update fails and falls back
          
          [possible values: seg, watch]

      --uplink <KIND>
          Link from the PC to the simulated hub
          
          [default: usb-cdc]
          [possible values: usb-cdc, uart, wifi-tcp]

      --port <N>
          TCP port on 127.0.0.1 [default: a free port]

      --no-open
          Do not open the browser; the address and passphrase are printed

  -h, --help
          Print help (see a summary with '-h')
```

## dagon ai

```text
Work with an AI agent

Usage: dagon ai <COMMAND>

Commands:
  link      Link DAGON to an agent one way, with your consent
  unlink    Unlink DAGON and withdraw the consent
  key       Manage an agent's API key in the OS credential store
  ask       Ask DAGON about the topology of ./dagon.toml
  topology  Turn a board description into topology YAML
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## dagon ai link

```text
Link DAGON to an agent one way, with your consent

Agents are TOML files, built in or in ~/.dagon/agents. Shows where it sends and what, and asks your consent once. Logins stay with the agent; API keys go in the OS credential store.

Usage: dagon ai link [OPTIONS]

Options:
      --agent <ID>
          Agent id [default: the first agent]

      --way <ID>
          Way id of the agent [default: its first way]

  -h, --help
          Print help (see a summary with '-h')
```

## dagon ai unlink

```text
Unlink DAGON and withdraw the consent

Usage: dagon ai unlink

Options:
  -h, --help
          Print help
```

## dagon ai key

```text
Manage an agent's API key in the OS credential store

Windows Credential Manager, macOS Keychain or Secret Service.

Usage: dagon ai key <COMMAND>

Commands:
  set     Type the key in; it is read without echo
  status  Show registered or not for every way that takes a key
  delete  Delete the key
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## dagon ai key set

```text
Type the key in; it is read without echo

Usage: dagon ai key set [OPTIONS]

Options:
      --agent <ID>
          Agent id [default: the first agent]

      --way <ID>
          Way id of the agent [default: its first way]

  -h, --help
          Print help
```

## dagon ai key status

```text
Show registered or not for every way that takes a key

Usage: dagon ai key status

Options:
  -h, --help
          Print help
```

## dagon ai key delete

```text
Delete the key

Usage: dagon ai key delete [OPTIONS]

Options:
      --agent <ID>
          Agent id [default: the first agent]

      --way <ID>
          Way id of the agent [default: its first way]

  -h, --help
          Print help
```

## dagon ai ask

```text
Ask DAGON about the topology of ./dagon.toml

A proposed change is shown as a diff, never applied.

Usage: dagon ai ask <QUESTION>...

Arguments:
  <QUESTION>...
          The question, in one or more words

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## dagon ai topology

```text
Turn a board description into topology YAML

The description comes from --in or stdin; the YAML is checked to 0 errors. Needs `dagon ai link` first.

Usage: dagon ai topology [OPTIONS]

Options:
      --in <FILE>
          Board description file [default: stdin]

      --out <YAML>
          Output topology YAML
          
          [default: topology.yaml]

      --model <ID>
          Model id to ask [default: the agent's]

  -h, --help
          Print help (see a summary with '-h')
```
