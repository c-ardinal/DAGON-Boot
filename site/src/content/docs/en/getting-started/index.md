---
title: "Try DAGON"
description: "DAGON Console on a simulator, in the browser or with the dagon command."
sidebar:
  order: 0
---

## In the browser

The [DAGON Console demo](~/demo/) runs in the browser, against a simulated node connected to the PC and two nodes linked to it.

1. Make a 構成 from the sample.
2. Switch to 書込 (write) and press 接続 (connect).
3. Press 書込 (write) to write the prepared new version to the nodes.

The demo reads and writes no files, connects to no device and holds no keys. Those controls say why when you point at them.

## With the dagon command

First [install](/en/getting-started/install/) the `dagon` command.

### `dagon sim`

```sh
dagon sim
```

It runs end-to-end scenarios in the simulator:

- A write with a power cut during the transfer
- An image that never confirms
- Tampered packages
- An erased F401 brought back through its ROM boot loader

With `--topology`, it simulates the update of a 構成 of your own, each node modelled on your project's MCU definitions, and `--fault <node id>=<kind>` injects a fault into the node you name:

```sh
dagon sim --topology topology.yaml --fault seg=power-cut --fault hub=tamper
```

| Kind | What happens to the node |
|---|---|
| `power-cut` | It loses power during the write |
| `no-confirm` | Its new version never confirms |
| `tamper` | One bit of its image is flipped |
| `erased` | Its flash is erased, and it is rewritten through its ROM boot loader |

Every node starts on 1.0.0 and the simulated release is 1.1.0. Each fault runs on a fresh simulation, and a line says whether DAGON kept the node safe. The simulator models the node the PC is connected to and the nodes one I2C or UART link away from it.
