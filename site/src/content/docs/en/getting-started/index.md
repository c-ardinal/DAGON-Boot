---
title: "Try DAGON"
description: "DAGON Console on a simulator, in the browser or with the dagon command."
sidebar:
  order: 0
---

## In the browser

The [DAGON Console demo](~/demo/) runs DAGON Console on a simulated 親機
(hub) with two 子機 (nodes), compiled for the browser. Make a 構成 from the
sample, switch to 書込 (write), connect and write the prepared version to
the nodes. The demo reads and writes no files, connects to no device and
holds no keys; those controls say why when you point at them.

## With the dagon command


```sh
dagon demo                         # DAGON Console with a simulator
dagon demo --sim-fail watch        # the same, and watch's update fails and falls back
dagon demo --sim-speed 4           # four times faster (for recordings)
```

`dagon demo` opens DAGON Console against a simulated 親機 with its two
子機 (the sample 構成: hub, seg, watch, all on 1.0.0), the simulator
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
