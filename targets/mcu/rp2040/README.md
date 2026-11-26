# dagon-mcu-rp2040

DAGON for RP2040 firmware: the flash layout and linker map, the boot images
(stage0 and the boot core), the flash port, the liveness-gated watchdog, the
slot store, DAGON over USB CDC, DAGON's high-priority executor, the link
drivers, and what runs the relay on this MCU: the relay itself is
`dagon-relay` (`../../../crates/dagon-relay`), independent of the MCU; this
package builds its links from RP2040 peripherals (USB CDC to the PC, I2C0
and a PIO UART to other nodes, the NRST / BOOT0 lines of an STM32 node for
the ROM recovery path) and runs it in its tasks (`relay`). The `[coordinator]`
section of `dagon-mcu.toml` is what lets an RP2040 node relay. Topology MCU
id `rp2040`, flash map `rp2040-4m`.

Board-specific values (pins, which link is wired where, flash size, display
and other extras) live in a board package that implements two traits:
[`BoardCore`] (`../common/src/hooks.rs`: board id, flash size, bring-up,
extension), which every MCU package shares, and [`BoardDef`]
(`src/node.rs`: the diagnostic LED and the peripheral split), for example
`../../boards/waveshare-rp2040-touch-lcd-1.28`. A board without a package
needs only its own `BoardCore` and `BoardDef` and links (see
`template/src/board.rs`).

## A coordinator with your own task

This is what `dagon new` generates for the Waveshare board
(`../../boards/waveshare-rp2040-touch-lcd-1.28/template`, its manifest kept
as `Cargo.toml.tpl` so cargo does not read it as a package). `Cargo.toml`
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
use dagon_mcu_rp2040::clock::Setup as Clock;
use dagon_mcu_rp2040::links::{self, Kind};
use dagon_mcu_rp2040::relay::{self, Setup};
use dagon_mcu_rp2040::watchdog::{now_ms, Guard, Trial};

// The hops, as dagon gen writes them from the 構成.
const UPLINK: Kind = Kind::UsbCdc;
const DOWNLINKS: &[Kind] = &[Kind::I2c, Kind::Uart];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);

// The clock the definitions choose for the node (dagon gen writes it into
// wiring.rs; a board package has its own in `def::app_clock`). A USB
// uplink needs the USB clock, which only a crystal gives.
const CLOCK: Clock = dagon_mcu_rp2040::clock_setup!(wiring::clock);
dagon_mcu_rp2040::require_uplink_clock!(CLOCK, UPLINK, "my-hub");

const TASKS: usize = relay::TASKS + app::TASKS;
static GUARD: Guard<TASKS> = Guard::new(
    relay::levels::<TASKS>(),
    Trial::FeedUntilMs(60_000),
    now_ms,
);

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) {
    let (dagon, io) = Board::split(embassy_rp::init(CLOCK.hal_config()));
    let setup = Setup {
        tag: dagon_mcu_rp2040::target_tag!(),
        // The role the 構成 gives this node (wiring.rs).
        role: if wiring::NODE.hub { relay::role::HUB } else { relay::role::NODE },
        name: "my-hub",
        app: app::dagon(),
        // dagon_relay::Config: the relay's defaults.
        relay: Default::default(),
    };
    let (spare, _events) = board::start(spawner, dagon, io, &GUARD, setup, UPLINK, DOWNLINKS);
    app::start(spawner, spare, &GUARD, relay::TASKS);
}
```

`src/app.rs`, the developer's file:

```rust
pub const TASKS: usize = 1;

pub fn self_check() -> bool {
    true
}

/// What the app hands the start: the self-check, its extensions
/// (`exts`) and who confirms (`confirm`).
pub fn dagon() -> App {
    App::new(self_check)
}

/// `spare`: the pins and peripherals DAGON does not use.
pub fn start<const N: usize>(spawner: Spawner, spare: Spare, guard: &'static Guard<N>, first: usize) {
    spawner.spawn(work(spare, guard.lease(first)).expect("task pool"));
}

#[embassy_executor::task]
async fn work(spare: Spare, lease: Lease) {
    // e.g. a free GPIO: let mut out = Output::new(spare.pin_0, Level::Low);
    let _ = spare;
    loop {
        lease.busy(500); // a piece of work stuck past its budget resets the chip
        // Your work here.
        lease.idle();
        Timer::after(Duration::from_secs(1)).await;
    }
}
```

`board::start` runs the relay: it opens the slot store and the agent
(taking over the watchdog the boot core started), spawns the PC task
(`dagon_relay::serve`) and one task per link (`dagon_relay::run_link`) on
DAGON's high-priority executor (`exec`, an interrupt executor on SWI_IRQ_0
at P1, guard tasks 0..3 `Preempting`), and the watchdog feeder on
`spawner` (thread mode, `Background`), and returns the board's `Spare` pins
and peripherals (free GPIOs, the display's pins, I2C1, SPI0, UART0/1, PIO1,
ADC, DMA channels) for the app, with the node's events (`AppEvents`: what
the app draws a display or writes a log from). Your tasks run in thread mode below the relay, so they never
delay it; a stuck one stops the feeding and the chip resets (a version on
trial then rolls back). Build with `DAGON_PUBKEY` set (below);
`--features slot-b` links the same app for slot B.

Without the relay, `node::start` serves a plain USB node the same way.

## Links

Every hop carries whole DAGON frames through one interface,
`dagon_mcu_common::link::Link`; the serve loops are generic over it. The
kinds this package offers (`links::LINK_KINDS`):

| Kind | Up | Down |
|---|---|---|
| `usb-cdc` | Yes | NotYet |
| `uart` | NotYet | Yes |
| `i2c` | NotYet | Yes |
| `ble` | NotYet | NotYet |
| `wifi-tcp` | NotYet | NotYet |
| `ethernet` | NotYet | NotYet |
| `can` | NotYet | NotYet |

- Up `usb-cdc`: the PC link of the coordinator (`links::UsbCdc`, the USB CDC
  device; `Reconnect::Reenumerates`).
- Down `i2c`: I2C0 controller with the DAGON mailbox (`links::I2c`), and
  down `uart`: a PIO UART (`links::Uart`); the board package names the
  pins (`downlink_i2c()`, `downlink_uart()`).

### Generator contract

The kind names are exactly the 構成's: `usb-cdc`, `uart`, `i2c`, `ble`,
`wifi-tcp`, `ethernet`, `can`. In a project template:

- `{{uplink}}` renders to one Rust path naming the node's uplink kind:
  `dagon_mcu_rp2040::links::Kind::<Variant>`;
- `{{downlinks}}` renders to the downlink kinds, comma-separated (empty
  for a node without downlinks): `dagon_mcu_rp2040::links::Kind::<Variant>, ...`.

Variants by name: `usb-cdc` `UsbCdc`, `uart` `Uart`, `i2c` `I2c`, `ble`
`Ble`, `wifi-tcp` `WifiTcp`, `ethernet` `Ethernet`, `can` `Can`. The
template checks them at compile time; a kind this package does not offer
in that direction stops the build with a message naming the hop, e.g.
`uplink ble: not available yet on rp2040`:

```rust
use dagon_mcu_rp2040::links::{self, Kind};

#[rustfmt::skip]
const UPLINK: Kind = {{uplink}};
#[rustfmt::skip]
const DOWNLINKS: &[Kind] = &[{{downlinks}}];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);
```

The board package's `start` takes `UPLINK` and `DOWNLINKS` and builds each
link on the board's pins; link types are constructed as
`dagon_mcu_rp2040::links::<Kind>::new(..)`.

## Features

| Feature | Adds |
|---|---|
| (none) | layout, image checks, frames, liveness, tag check (host-testable) |
| `hal` | `node`, `flash`, `watchdog`, `store`, `links`, `sub`; `PUBKEY` / `TEST_KEY` |
| `usb` | `usb`, `exec` and `node::start` (this package binds `USBCTRL_IRQ` and `SWI_IRQ_0`) |
| `relay` (default) | `relay`: runs `dagon-relay` on this MCU's links |
| `slot-b` | link the app for APP slot B |
| `image-stage0`, `image-bootcore` | link the boot images instead of an app |
| `diag-panic` | a panic blinks the last `arm::step` code on the board's LED |

## Flash layout (the built-in definition; needs 4 MiB)

These are the built-in definition's slots. A project's definition may
move them or give the boot core one slot; the app keeps two here,
since only the app writes the app on this MCU (`[writers]`).

XIP addresses (flash offset + 0x1000_0000). The board's flash size is
check-only: a board with less than 4 MiB fails to build, a larger one keeps
the rest for the app.

| Address | Size | Content |
|---|---|---|
| 0x10000000 | 16 KiB | 256 B head the boot ROM reads, then stage0 |
| 0x10004000 | 2 x 4 KiB | boot core state log faces A/B |
| 0x10006000 | 2 x 4 KiB | app state log faces A/B |
| 0x10008000 | 128 KiB | boot core slot A: 512-byte header, body at 0x10008200 |
| 0x10028000 | 128 KiB | boot core slot B: 512-byte header, body at 0x10028200 |
| 0x10048000 | 1 MiB | app slot A: 512-byte header, body at 0x10048200 |
| 0x10148000 | 1 MiB | app slot B: 512-byte header, body at 0x10148200 |

The table is the definition's (a test in `targets/mcu/build` writes it).
The first bytes of the stage0 region are boot2, which the boot ROM loads
and checks; stage0 (`stage0/`) follows and is never updated, and it reads
the boot core state log. The boot core is built from `bootcore/`.

The boot core starts the watchdog for 8 s before every jump. Stage0 starts
the pending boot core (written by the running app's agent from a release)
on trial, up to 3 starts counted in the watchdog's SCRATCH0/SCRATCH1, with
the watchdog started before the jump; else the confirmed one. The new core
confirms itself once it can start a verified app; one that cannot is reset
and, after its starts run out, the old core runs again and records the
rollback.
Stage0 starts a core only as a record names it: never a core no record
names, nor a pending one whose starts ran out. When no core may start
(with one boot core slot, a new core cut by a power loss or one that
failed its trial), stage0 starts the app of the state log's confirmed
APP record after checking it as the boot core would (signature, kind,
MCU, target tag, slot, load range, body digest, entry point inside the
body); that app writes the boot core again. With one boot core slot the
app's agent refuses a boot core transfer while the app is on trial.
Stage0 halts only when there is no such app.

The boot core receives on the board's DAGON buses (`dagon = true`): its
I2C bus as a target at the node's address in the topology (else the
board's `[default_link] addr`), its UART bus
(a hardware UART or a PIO). Right after its start it listens on them for
the listen window the MCU definition gives; a STAY there, or the app's
hand-over (REBOOT mode 1 writes UPDATE_STAY), keeps it in update mode,
which it leaves on REBOOT or after 60 s without a request. With nothing
to start it serves recovery mode. In both modes it also serves USB, as
the same USB device the app presents, on a board with a crystal (the
48 MHz USB clock); USB takes no part in the window.

## Boot images

`stage0/` and `bootcore/` are DAGON's boot images for this MCU (their
memory maps and the boot core's key come from this package). Both are
built for one node of a project, after `dagon gen`:

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd <project>/boot/<node> && cargo build --release)    # the boot core
(cd targets/mcu/rp2040/stage0 && DAGON_BOOT_CRATE=/abs/path/to/<project>/boot/<node> cargo build --release)
```

`--features slot-b` links the boot core for boot core slot B; a release
carries one for each boot core slot the node's MCU definition gives.

`dagon gen` writes the node's boot crate into `boot/<node>/` of the
project from `bootcore/template/`: it links `bootcore` (a library) with
the node's boot extensions (`nodes/<node>/dagon/boot_wiring.rs`, each
declared trusted in dagon.toml, see site/src/content/docs/reference/extension-manifest.md) and sets
`DAGON_BOOT_CRATE` (that folder) and `DAGON_TARGET_TAG` (the node's tag)
for its build. The boot core accepts only app images tagged with the
node's tag (a host test, `tests/hub_boot.rs`, shows another node's image
refused); its status LED, the LED's polarity and the halt pattern are the
node's board definition's `[boot] led`, `led_active` and `halt` (a node
without a board has none). Builds without `DAGON_BOOT_CRATE` fail, and a
boot crate that links anything else than DAGON's packages and its
trusted extensions with the source hash dagon.toml declares does not
build. The boot core build takes `--features diag` for step blink codes
(see the RP2040 README). Stage0 checks a boot core's signature under the key
built into its own flash and starts no core whose signature does not
verify; with neither a core nor a confirmed app to start, it blinks the reason's code on the board's
`[boot] led` (a node without a board, or a board without the LED or with
its halt pattern off, shows nothing).

The addresses and lengths every image links for (slots, logs, stage0)
and the watchdog and trial values come from the node's MCU definition in
that folder, `mcu.toml`, which `dagon gen` writes with the project's
overrides applied (the app's `.cargo/config.toml` names the same folder),
so a project that moves its slots gets matching firmware without a code
edit. `DAGON_BOOT_CRATE` is an absolute path; an `mcu.toml` of another MCU
or another definition format is refused, and a build without the variable
uses this package's `dagon-mcu.toml`.


## Keys

`DAGON_PUBKEY` is the public key file of `dagon key gen` (or its 64 hex
digits) and is embedded at build time. Unset, the build fails, unless
`DAGON_ALLOW_TEST_KEY=1` asks for the public RFC 8032 test key
(`TEST_KEY` is then true; apps should show it).

## Tests

```sh
cargo test --no-default-features --target <host triple>
```

`tests/explore.rs` (with `tests/cut/`) is the power-cut acceptance
criterion on this MCU: every sequence of an app or boot core update, a
CONFIRM, a boot and a power loss up to a bound, with a power cut or a
reset at every flash operation (each slot's erase, body and header; each
record of the boot core log and the app log; each step of either log's
compaction), not landed or half landed, and with one flash operation that
fails while the power stays. After each, stage0 starts something, no
broken image is confirmed, and boots end at a good app. It runs
`store::Store`, the app's slot store, over a flash that cuts the power;
what the store asks of the chip (`store::Chip`) is given by the test.
With the folder in `DAGON_POWERCUT_REPORT` it writes the cuts per flash
operation.

[`BoardCore`]: ../common/src/hooks.rs
[`BoardDef`]: src/node.rs
