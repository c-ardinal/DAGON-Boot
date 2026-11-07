---
title: "Try DAGON"
description: "DAGON Console on a simulator, in the browser or with the dagon command."
sidebar:
  order: 0
---

## In the browser

The [DAGON Console demo](~/demo/) runs in the browser, against a simulated 親機 (hub) and two 子機 (children).

1. Make a 構成 from the sample.
2. Switch to 書込 (write) and press 接続する (connect).
3. Press 書き込む (write) to write the prepared new version to the nodes.

The demo reads and writes no files, connects to no device and holds no keys. Those controls say why when you point at them.

## With the dagon command

First [install](/en/getting-started/install/) the `dagon` command.

### `dagon sim run`

```sh
dagon sim run
```

It runs end-to-end scenarios in the simulator:

- A write with a power cut during the transfer
- An image that never confirms
- Tampered packages
- An erased F401 brought back through its ROM boot loader
