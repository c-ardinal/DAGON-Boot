# dagon-mcu-rp2040

DAGON for RP2040 firmware: the flash layout and linker map, the boot images
(stage0 and the boot core), the flash port, the liveness-gated watchdog, the
slot store, DAGON over USB CDC, link drivers (I2C controller with the DAGON
mailbox, PIO UART) and sub-MCU control lines. Topology MCU id `rp2040`,
flash map `rp2040-4m`.

Board-specific values (pins, which link is wired where, flash size, display
and other extras) live in a board package that implements [`BoardDef`]
(`src/node.rs`), for example `../../boards/waveshare-rp2040-touch-lcd-1.28`.
A board without a package needs only its own `BoardDef` (a few lines naming
the flash size and splitting the peripherals).

## A minimal app

`Cargo.toml` (paths, or git dependencies on a release tag):

```toml
[features]
slot-b = ["dagon-board-waveshare-rp2040-touch-lcd-1-28/slot-b"]

[dependencies]
dagon-board-waveshare-rp2040-touch-lcd-1-28 = { path = ".../targets/boards/waveshare-rp2040-touch-lcd-1.28", features = ["usb"] }
dagon-mcu-rp2040 = { path = ".../targets/mcu/rp2040", features = ["usb"] }
embassy-rp = { version = "0.10.0", features = ["rp2040", "time-driver", "critical-section-impl"] }
embassy-executor = { version = "0.10", features = ["platform-cortex-m", "executor-thread"] }
embassy-time = "0.5"
cortex-m-rt = "0.7"
panic-halt = "1"
```

`.cargo/config.toml`:

```toml
[build]
target = "thumbv6m-none-eabi"

[target.thumbv6m-none-eabi]
rustflags = ["-C", "link-arg=--nmagic", "-C", "link-arg=-Tlink.x"]
```

`src/main.rs` (no build.rs: the memory map comes from this package):

```rust
#![no_std]
#![no_main]

use dagon_board_waveshare_rp2040_touch_lcd_1_28::{Board, BoardDef, Io};
use dagon_mcu_rp2040::node;
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Lease, Level, Trial};
use embassy_executor::Spawner;
use embassy_time::Timer;
use panic_halt as _;

// Task 0 serves DAGON, task 1 is the app. Unconfirmed images keep feeding
// for 60 s after boot, then roll back unless the host confirmed them.
static GUARD: Guard<2> = Guard::new(
    [Level::Preempting, Level::Background],
    Trial::FeedUntilMs(60_000),
    now_ms,
);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    node::start::<Board, 2>(spawner, dagon, &GUARD, 0, "my-node", "my node");
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

`node::start` opens the slot store, takes over the watchdog the boot core
started, and spawns the USB device, the agent and the watchdog feeder.
`"my-node"` is the node's image target tag (as in the topology),
`"my node"` the USB product name. Build with `DAGON_PUBKEY` set (below);
`cargo build --release --features slot-b` links the same app for slot B.

The serving task is declared `Preempting` even on the same executor: while
it erases a slot (it does not yield) the app cannot run, and this level
freezes the app's deadline meanwhile instead of counting it as stuck.

DAGON's own hub firmware (`../../rp2040-hub`) uses the same pieces with its
own task layout: `node::open` for the agent, `usb::new` / `usb::run` for
the link, the board package's helpers for its sub-node links.

## Features

| Feature | Adds |
|---|---|
| (none) | layout, image checks, STATUS mapping, frames, liveness (host-testable) |
| `hal` | `node`, `flash`, `watchdog`, `store`, `links`, `sub`; `PUBKEY` / `TEST_KEY` |
| `usb` | `usb` and `node::start` (this package binds `USBCTRL_IRQ`) |
| `slot-b` | link the app for APP slot B |
| `image-stage0`, `image-bootcore` | link the boot images instead of an app |
| `diag-panic` | a panic blinks the last `arm::step` code on the board's LED |

## Flash layout (fixed; needs 4 MiB)

XIP addresses (flash offset + 0x1000_0000). The board's flash size is
check-only: a board with less than 4 MiB fails to build, a larger one keeps
the rest for the app.

| Address | Size | Content |
|---|---|---|
| 0x10000000 | 16 KiB | boot2 (256 B) + stage0 (`stage0/`, never updated) |
| 0x10004000 | 2 x 4 KiB | boot core state log faces (read by stage0) |
| 0x10006000 | 2 x 4 KiB | APP state log faces A/B |
| 0x10008000 | 128 KiB | boot core slot A (`bootcore/`), body at +0x200 |
| 0x10028000 | 128 KiB | boot core slot B (reserved) |
| 0x10048000 | 1 MiB | APP-A: 512-byte header, body at 0x10048200 |
| 0x10148000 | 1 MiB | APP-B: same, body at 0x10148200 |
| 0x10248000 | 4 KiB | topology display copy |

The boot core starts the watchdog for 8 s before every jump.

## Boot images

`stage0/` and `bootcore/` are DAGON's boot images for this MCU, built from
this package (their memory maps and the boot core's key come from here):

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/mcu/rp2040/stage0 && cargo build --release)
(cd targets/mcu/rp2040/bootcore && cargo build --release)
```

The boot core accepts app images tagged `layout::HUB_TAG` (DAGON's hub
firmware). Their status LED is GPIO25 (the hub board's backlight);
`DAGON_DIAG_LED=<gpio>` at build time picks another one. Both builds take
`--features diag` for step blink codes (see the hub README).

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
