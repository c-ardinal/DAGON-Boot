# dagon-mcu-rp2040

DAGON for RP2040 firmware: the flash layout and linker map, the boot images
(stage0 and the boot core), the flash port, the liveness-gated watchdog, the
slot store, DAGON over USB CDC, DAGON's high-priority executor, the 親機
relay (PC link, relaying to 子機 over I2C0 and a PIO UART, the STM32 ROM
recovery path) and the link drivers. Topology MCU id `rp2040`, flash map
`rp2040-4m`. RP2040 nodes are 親機.

Board-specific values (pins, which link is wired where, flash size, display
and other extras) live in a board package that implements [`BoardDef`]
(`src/node.rs`), for example `../../boards/waveshare-rp2040-touch-lcd-1.28`.
A board without a package needs only its own `BoardDef` and links (see
`template/src/board.rs`).

## A 親機 with your own task

This is what `dagon new` generates for the Waveshare board
(`../../boards/waveshare-rp2040-touch-lcd-1.28/template`). `Cargo.toml`
names the two packages without features (paths, or git dependencies on a
release tag); `.cargo/config.toml` sets the target, the link arguments and
the node's tag:

```toml
[build]
target = "thumbv6m-none-eabi"

[target.thumbv6m-none-eabi]
rustflags = ["-C", "link-arg=--nmagic", "-C", "link-arg=-Tlink.x"]

[env]
DAGON_TARGET_TAG = { value = "my-hub", force = true }
```

`src/main.rs` (no build.rs: the memory map comes from this package):

```rust
use dagon_board_waveshare_rp2040_touch_lcd_1_28::{self as board, Board, BoardDef};
use dagon_mcu_rp2040::relay::{self, RelayConfig};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};

const TASKS: usize = board::APP_TASKS + app::TASKS;
static GUARD: Guard<TASKS> = Guard::new(
    relay::levels::<TASKS>(),
    Trial::FeedUntilMs(60_000),
    now_ms,
);

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(Default::default()));
    let cfg = RelayConfig {
        tag: dagon_mcu_rp2040::TARGET_TAG,
        name: "my-hub",
        fallback: &[],
        self_check: app::self_check,
    };
    board::start(spawner, dagon, io, &GUARD, cfg);
    app::start(spawner, &GUARD, board::APP_TASKS);
}
```

`src/app.rs`, the developer's file:

```rust
pub const TASKS: usize = 1;

pub fn self_check() -> bool {
    true
}

pub fn start<const N: usize>(spawner: Spawner, guard: &'static Guard<N>, first: usize) {
    spawner.spawn(work(guard.lease(first)).expect("task pool"));
}

#[embassy_executor::task]
async fn work(lease: Lease) {
    loop {
        lease.busy(500); // a piece of work stuck past its budget resets the chip
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_secs(1)).await;
    }
}
```

`board::start` runs the relay: it opens the slot store and the agent
(taking over the watchdog the boot core started), spawns the PC task and
one task per sub-node link on DAGON's high-priority executor (`exec`, an
interrupt executor on SWI_IRQ_0 at P1, guard tasks 0..3 `Preempting`), and
the watchdog feeder and the display on `spawner` (thread mode,
`Background`). Your tasks run in thread mode below the relay, so they never
delay it; a stuck one stops the feeding and the chip resets (a version on
trial then rolls back). Build with `DAGON_PUBKEY` set (below);
`--features slot-b` links the same app for slot B.

DAGON's own hub firmware (`../../rp2040-hub`) is this with the demo's
fallback node list and its tag `hub-rp2040-lcd`. Without the relay,
`node::start` serves a plain USB node the same way.

## Features

| Feature | Adds |
|---|---|
| (none) | layout, image checks, STATUS mapping, frames, liveness, tag check (host-testable) |
| `hal` | `node`, `flash`, `watchdog`, `store`, `links`, `sub`; `PUBKEY` / `TEST_KEY` |
| `usb` | `usb`, `exec` and `node::start` (this package binds `USBCTRL_IRQ` and `SWI_IRQ_0`) |
| `relay` (default) | `relay`: the 親機 relay |
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
(cd targets/mcu/rp2040/bootcore && DAGON_TARGET_TAG=my-hub cargo build --release)
```

The boot core is built per node: it accepts only app images tagged with
`DAGON_TARGET_TAG` (the node's tag, as in the app's `.cargo/config.toml`;
`hub-rp2040-lcd` for DAGON's hub firmware), and its build fails without
it. A host test (`tests/hub_boot.rs`) shows another node's image refused.
Their status LED is GPIO25 (the hub board's backlight);
`DAGON_DIAG_LED=<gpio>` at build time picks another one. Both builds take
`--features diag` for step blink codes (see the hub README).

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
