# dagon-mcu-stm32f401re

DAGON for STM32F401RE firmware: the flash layout and linker map, the boot
core, clocks, the flash port, the liveness-gated IWDG, the kind S slot
store, and the DAGON link as an I2C target with the mailbox (the hub reaches
the node over I2C). Topology MCU id `stm32f401re`, flash map
`f401-default`.

Board-specific values (the link's I2C peripheral, pins and address, flash
size, LEDs and displays) live in a board package that implements
[`BoardDef`] (`src/node.rs`), for example `../../boards/nucleo-f401re`. A
board without a package needs only its own `BoardDef`.

## A minimal app

`Cargo.toml` (paths, or git dependencies on a release tag):

```toml
[features]
slot-b = ["dagon-board-nucleo-f401re/slot-b"]

[dependencies]
dagon-board-nucleo-f401re = { path = ".../targets/boards/nucleo-f401re" }
dagon-mcu-stm32f401re = { path = ".../targets/mcu/stm32f401re", features = ["node"] }
embassy-stm32 = { version = "0.6.0", features = ["stm32f401re", "time-driver-any"] }
embassy-executor = { version = "0.10", features = ["platform-cortex-m", "executor-thread"] }
embassy-time = "0.5"
cortex-m = { version = "0.7", features = ["critical-section-single-core"] }
cortex-m-rt = "0.7"
defmt-rtt = "1"
panic-probe = "1"
```

`.cargo/config.toml`:

```toml
[build]
target = "thumbv7em-none-eabihf"

[target.thumbv7em-none-eabihf]
rustflags = ["-C", "link-arg=--nmagic", "-C", "link-arg=-Tlink.x", "-C", "link-arg=-Tdefmt.x"]
```

`src/main.rs` (no build.rs: the memory map comes from this package; this
is what `dagon new` generates, see `../../boards/nucleo-f401re/template`):

```rust
#![no_std]
#![no_main]

mod app;

use dagon_board_nucleo_f401re::{self as board, Board, BoardDef};
use dagon_mcu_stm32f401re::links::{self, Kind};
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Trial};
use dagon_mcu_stm32f401re::{clocks, node};
use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};

// DAGON's link is guard task 0, app.rs's tasks follow. Unconfirmed images
// never feed: they reset after the boot core's trial period unless the
// 親機 confirmed them.
// The hops, as dagon gen writes them from the 構成.
const UPLINK: Kind = Kind::I2c;
const DOWNLINKS: &[Kind] = &[];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);

const TASKS: usize = node::APP_TASKS + app::TASKS;
static GUARD: Guard<TASKS> = Guard::new(node::levels::<TASKS>(), Trial::NeverFeed, now_ms);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_stm32::init(clocks()));
    let tag = dagon_mcu_stm32f401re::target_tag!();
    board::start(spawner, dagon, &GUARD, tag, app::self_check, UPLINK).ok();
    app::start(spawner, io, &GUARD, node::APP_TASKS);
}
```

`src/app.rs` holds `TASKS`, `self_check()` and `start()`, which spawns the
app's tasks, each with a `Lease` (`lease.busy(budget_ms)` before a piece of
work, `lease.idle()` when waiting).

`board::start` (`node::start` for this board) opens the slot store (it
fails if the state log is unreadable), takes over the IWDG with the boot
core's period, opens the board's I2C target, spawns the serving task (guard
task `node::DAGON_TASK`, 0, `Preempting`) on DAGON's high-priority executor
(`exec`, an interrupt executor on the unused USART6 vector at P6) and the
feeder on `spawner`. The app's tasks run in thread mode below it, so the
親機 is answered whatever they do. The node's tag comes from
`DAGON_TARGET_TAG` in `.cargo/config.toml`
(`DAGON_TARGET_TAG = { value = "my-node", force = true }` under `[env]`);
`self_check` decides whether a version on trial may be confirmed. An app
that keeps `embassy_stm32::Peripherals` itself passes `dagon_pins!(p)`
instead of splitting. Build with `DAGON_PUBKEY` set (below);
`--features slot-b` links the same app for slot B. An app that runs
the pieces itself uses `open_store`, `install_watchdog`, `agent`, `exec`
and `serve`. `open_store` returns the slot store and the serving loop's
handle on the same flash driver (`SharedFlash`; a conformance build runs
CONF_RUN through it): the agent keeps its store to itself. A REBOOT reply
is followed by the delay the app's `allow_reboot_now` asked for, the
serving task idle under the guard, then the reset.

## Links

Every hop carries whole DAGON frames through one interface,
`dagon_mcu_common::link::Link`; the serve loops are generic over it. The
kinds this package offers (`links::LINK_KINDS`):

| Kind | Up | Down |
|---|---|---|
| `usb-cdc` | NotYet | No |
| `uart` | NotYet | No |
| `i2c` | Yes | No |
| `ble` | NotYet | No |
| `wifi-tcp` | NotYet | No |
| `ethernet` | NotYet | No |
| `can` | NotYet | No |

- Up `i2c`: the I2C target with the DAGON mailbox (`links::I2c`); the board
  package names the peripheral, pins and address (`uplink_i2c()`).

### Generator contract

The kind names are exactly the 構成's: `usb-cdc`, `uart`, `i2c`, `ble`,
`wifi-tcp`, `ethernet`, `can`. In a project template:

- `{{uplink}}` renders to one Rust path naming the node's uplink kind:
  `dagon_mcu_stm32f401re::links::Kind::<Variant>`;
- `{{downlinks}}` renders to the downlink kinds, comma-separated (empty
  for a 子機): `dagon_mcu_stm32f401re::links::Kind::<Variant>, ...`.

Variants by name: `usb-cdc` `UsbCdc`, `uart` `Uart`, `i2c` `I2c`, `ble`
`Ble`, `wifi-tcp` `WifiTcp`, `ethernet` `Ethernet`, `can` `Can`. The
template checks them at compile time; a kind this package does not offer
in that direction stops the build with a message naming the hop, e.g.
`uplink ble: not available yet on stm32f401re`:

```rust
use dagon_mcu_stm32f401re::links::{self, Kind};

#[rustfmt::skip]
const UPLINK: Kind = {{uplink}};
#[rustfmt::skip]
const DOWNLINKS: &[Kind] = &[{{downlinks}}];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);
```

The board package's `start` takes `UPLINK` (a 子機 has no downlinks) and
builds the link on the board's pins; link types are constructed as
`dagon_mcu_stm32f401re::links::<Kind>::new(..)`.

## Features

| Feature | Adds |
|---|---|
| (none) | layout, slot store, mailbox (host-testable) |
| `hal` | `clocks`, `flash`, `watchdog`; `PUBKEY` / `TEST_KEY` (the boot core stops here) |
| `node` (default) | `node` (board definition, start-up, serving loop) and `exec` (binds USART6's vector); logs through defmt |
| `conform` | the conformance build's CONF_RUN in front of the agent |
| `slot-b` | link the app for APP slot B (the boot core for boot core slot B) |
| `image-stage0`, `image-bootcore` | link the boot images instead of an app |

## Flash layout (fixed; needs 512 KiB)

The board's flash size is check-only: a board with less fails to build.

| Address | Size | Content |
|---|---|---|
| 0x08000000 | 16 KiB | stage0 (`stage0/`, never updated: the reset vector) |
| 0x08004000 | 2 x 16 KiB | state log faces A/B (APP and boot core records) |
| 0x0800C000 | 16 KiB | conformance scratch |
| 0x08010000 | 64 KiB | boot core slot A (`bootcore/`): 512-byte header, body at 0x08010200 |
| 0x08020000 | 128 KiB | APP-A: 512-byte header, body at 0x08020200 |
| 0x08040000 | 128 KiB | APP-B: same, body at 0x08040200 |
| 0x08060000 | 128 KiB | boot core slot B: same, body at 0x08060200 |

Stage0 starts the boot core: the pending one (written by the running app's
agent from a release) on trial, up to 3 starts counted in the RTC backup
registers BKP0R/BKP1R, else the confirmed one. The new core confirms
itself once it can start a verified app; one that cannot is reset and,
after its starts run out, the old core runs again and records the
rollback. The boot core tries a pending APP slot up to 3 times (signature
+ SHA-256, BOOT_ATTEMPT before each start, 4 s watchdog) and then rolls
back; otherwise it starts the confirmed slot (8 s watchdog). Both run at
84 MHz from the HSI (`clocks()`). The ROM loader's full rewrite (the last
resort) writes stage0, boot core A, the state log and APP-A.

## Boot images

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/mcu/stm32f401re/stage0 && cargo build --release)
(cd targets/mcu/stm32f401re/bootcore && DAGON_TARGET_TAG=my-node cargo build --release)
```

`--features slot-b` links the boot core for boot core slot B; a release
carries one for each slot.


It is built per node: it accepts only app images tagged with
`DAGON_TARGET_TAG` (the node's tag, as in the app's `.cargo/config.toml`),
and its build fails without it. With nothing startable it shows the board's
halt pattern on the board's boot LED (`[boot] led` and `halt`): the build
reads the board `DAGON_BOARD=<board id>` names (with
`DAGON_PROJECT=<path of dagon.toml>`, a project's inline `[board.<id>]`
too); it may be left out only while the MCU has a single board definition.

## Keys

`DAGON_PUBKEY` is the public key file of `dagon key gen` (or its 64 hex
digits) and is embedded at build time. Unset, the build fails, unless
`DAGON_ALLOW_TEST_KEY=1` asks for the public RFC 8032 test key
(`TEST_KEY` is then true; apps should show it).

## Tests

```sh
cargo test --no-default-features --target <host triple>
```

[`BoardDef`]: src/node.rs
