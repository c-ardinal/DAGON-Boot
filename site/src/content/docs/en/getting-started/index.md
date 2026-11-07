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

### `dagon demo`

```sh
dagon demo                         # DAGON Console with simulated nodes
dagon demo --sim-fail watch        # the same, and the write to watch fails and goes back
dagon demo --sim-speed 4           # four times faster
```

`dagon demo` opens DAGON Console against a simulated 親機 and its two 子機, three nodes (the sample 構成: hub, seg, watch, all on 1.0.0). It runs the simulator `dagon sim run` uses, driven by the same dagon-host code as real boards.

- Connect and choose the new version (demo-1.1.0) to write all three, with progress in view.
- With `--sim-fail seg` or `--sim-fail watch`, that 子機's new version crashes at boot and it goes back to 1.0.0.
- The links and flash take about their real time. `--sim-speed` scales it from 0.1 to 20.
- The header shows 模擬 the whole time.

The demo makes its own project in the temporary folder (`dagon-demo-<hash of your user name>`), afresh at each start. It also makes a DEMO key for it. Your keys are not used, and nothing it signs runs on real boards.

### `dagon sim run`

```sh
dagon sim run
```

It runs end-to-end scenarios in the simulator:

- A write with a power cut during the transfer
- An image that never confirms
- Tampered packages
- An erased F401 brought back through its ROM boot loader
