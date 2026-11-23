# rp2040-round-lcd (sample app with its own round LCD)

A sample, not part of DAGON: the `hub` node of the sample topology
(`../../topology/sample.yaml`) on the Waveshare RP2040-Touch-LCD-1.28. DAGON
runs the relay on it, as on any RP2040 node that relays; the app draws the
board's round LCD itself, from what DAGON hands every app. DAGON draws no
screen and does not assume a node has one.

| Folder | What it is |
|---|---|
| `app/` | The app firmware (built for `thumbv6m-none-eabi`), its tag `hub-rp2040-lcd` in `app/.cargo/config.toml` |
| `screen/` | The one implementation of the screens: the panel's commands, the Japanese glyphs and fonts, the app's home screen, and a trusted boot extension that shows the boot core's steps (`screen/README.md`) |
| `uf2check/` | A host tool that reads the first-install UF2 the way the node will boot it |

## What the app draws, and from what

- This node's name, version and state: the name from `app/src/wiring.rs`
  (written by `dagon gen` from the topology), the version and state from
  the node's events (`AppEvents`, the latest `AppSnapshot`) that the board
  package's `start` returns. While a new image is written to this node,
  its ring becomes an arc of the write's progress (the percentage the
  PC tells the node, else the bytes received).
- The nodes this node's links reach: `wiring::REACHED`, up to three of
  them, each with the port its link runs on.
- The state in Japanese below the ring (起動中, 確定, 書込中, 検証中,
  書込済, 再起動待機, 再起動, 試行中, ロールバック, 失敗), so colour is
  never the only cue; "TEST KEY" at the bottom of a build with the public
  test key.

The app takes the display's bus and pins from the `Spare` the board
package hands it (`Spare::display`), sets them up with `Display::pins`, and
owns them in one task (`app/src/display.rs`) after the relay's guard tasks.
The layout runs in host tests (`screen/src/home/tests.rs`).

`app/src/wiring.rs` is copied as `dagon gen` writes it for the `hub` node
of the sample topology. After a change to the topology, run `dagon gen hub`
in a project of that topology and copy `nodes/hub/src/wiring.rs` here.

## Build

The verification key is embedded at build time from the public key file of
`dagon key gen` (`DAGON_PUBKEY`). Without it the build fails, unless
`DAGON_ALLOW_TEST_KEY=1` asks for the public RFC 8032 test key (the build
warns and the LCD shows TEST KEY). The flash layout, stage0 and the boot
core are the RP2040 MCU package's (`../../targets/mcu/rp2040/README.md`).

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/mcu/rp2040/stage0 && DAGON_BOOT_CRATE=<project>/boot/<node> cargo build --release)
(cd targets/mcu/rp2040/bootcore && DAGON_TARGET_TAG=hub-rp2040-lcd cargo build --release)
(cd samples/rp2040-round-lcd/app && cargo build --release)
(cd samples/rp2040-round-lcd/app && cargo build --release --features slot-b --target-dir target/slot-b)
```

## First install UF2

Point `[provision.hub]` in `dagon.toml` at the three release ELFs
(stage0, boot core, app slot A) with `version = "1.0.0"`, then:

```sh
dagon provision hub --key /path/outside/the/repo/dagon-signing.key
(cd samples/rp2040-round-lcd/uf2check && cargo run --release --target <host triple> -- ../../../out/provision/hub/rescue.uf2 /path/outside/the/repo/dagon-signing.pub hub-rp2040-lcd)
```

`uf2check` reads the UF2 the way stage0 and the boot core will (boot core
log, image checks, dagon-core's boot decision) and fails if the node would
not start. Hold BOOT, plug in USB (or press RESET), and copy `rescue.uf2` to
the `RPI-RP2` drive.

## On the device

- USB: CDC ACM device, product "DAGON hub", manufacturer "DAGON", serial
  number = flash unique ID (16 hex digits), VID:PID c0de:cafe.
- Frames on USB: RELAY over the links the RP2040 definition lists in
  `[[relay_link]]` (`targets/mcu/rp2040/dagon-mcu.toml`) is forwarded;
  SUBCTL and I2C_RAW drive an STM32 node's ROM path; everything else (HELLO,
  STATUS, SHOW, XFER_*, SET_PENDING, REBOOT, CONFIRM) is answered by this
  node's own agent.
- The relay sends nothing to the other nodes unless the PC asks.
