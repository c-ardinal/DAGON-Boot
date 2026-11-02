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

`src/main.rs` (no build.rs: the memory map comes from this package):

```rust
#![no_std]
#![no_main]

use dagon_board_nucleo_f401re::{Board, BoardDef, Io};
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Lease, Level, Trial};
use dagon_mcu_stm32f401re::{clocks, node};
use embassy_executor::Spawner;
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

// Task 0 serves DAGON, task 1 is the app. Unconfirmed images never feed:
// they reset after the boot core's trial period unless the hub confirmed.
static GUARD: Guard<2> = Guard::new(
    [Level::Preempting, Level::Background],
    Trial::NeverFeed,
    now_ms,
);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_stm32::init(clocks()));
    node::start::<Board, 2>(spawner, dagon, &GUARD, 0, "my-node").unwrap();
    spawner.spawn(app(io, GUARD.lease(1)).unwrap());
}

#[embassy_executor::task]
async fn app(_io: Io, live: Lease) {
    loop {
        live.feed(); // stop calling this and the watchdog resets the chip
        Timer::after_millis(500).await;
    }
}
```

`node::start` opens the slot store (it fails if the state log is
unreadable), takes over the IWDG with the boot core's period, opens the
board's I2C target and spawns the serving task and the feeder. `"my-node"`
is the node's image target tag. Build with `DAGON_PUBKEY` set (below);
`--features slot-b` links the same app for slot B.

The serving task is declared `Preempting` even on the same executor: a
request that erases a sector blocks the executor, and this level freezes
the app's deadline meanwhile instead of counting it as stuck. The demo node
(`../../../samples/f401-app`) runs serving on an interrupt executor instead,
from the same pieces (`open_store`, `install_watchdog`, `serve`).

## Features

| Feature | Adds |
|---|---|
| (none) | layout, slot store, mailbox (host-testable) |
| `hal` | `clocks`, `flash`, `watchdog`; `PUBKEY` / `TEST_KEY` (the boot core stops here) |
| `node` | `node` (board definition, start-up, serving loop); logs through defmt |
| `conform` | the conformance build's CONF_RUN in front of the agent |
| `slot-b` | link the app for APP slot B |
| `image-bootcore` | link the boot core instead of an app |

## Flash layout (fixed; needs 512 KiB)

The board's flash size is check-only: a board with less fails to build.

| Address | Size | Content |
|---|---|---|
| 0x08000000 | 32 KiB | boot core (`bootcore/`), signed header in the last 512 B |
| 0x08008000 | 2 x 16 KiB | APP state log faces A/B |
| 0x08010000 | 64 KiB | reserved |
| 0x08020000 | 128 KiB | APP-A: 512-byte header, body at 0x08020200 |
| 0x08040000 | 128 KiB | APP-B: same, body at 0x08040200 |
| 0x08060000 | 128 KiB | reserved |

The boot core tries a pending slot up to 3 times (signature + SHA-256,
BOOT_ATTEMPT before each start, 4 s watchdog) and then rolls back;
otherwise it starts the confirmed slot (8 s watchdog). Both run at 84 MHz
from the HSI (`clocks()`).

## Boot core

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/mcu/stm32f401re/bootcore && cargo build --release)
```

It accepts app images tagged `layout::TARGET_TAG` (the demo node's
`seg-f401-7seg`); with nothing startable it blinks PA5 (LD2 on NUCLEO
boards).

## Keys

`DAGON_PUBKEY` is the public key file of `dagon key gen` (or its 64 hex
digits) and is embedded at build time. Unset, the build fails, unless
`DAGON_ALLOW_TEST_KEY=1` asks for the public RFC 8032 test key
(`TEST_KEY` is then true; apps should show it).

## Tests

```sh
cargo test --target <host triple>
```

[`BoardDef`]: src/node.rs
