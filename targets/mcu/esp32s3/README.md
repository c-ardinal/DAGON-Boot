# dagon-mcu-esp32s3

DAGON on the ESP32-S3, the MCU layer: everything that depends on the chip
and not on the board. Kind V: the stock ESP-IDF 2nd stage with app rollback
(`targets/esp32s3-2ndstage`) picks the slot, this crate drives otadata.

| Module | Provides |
|---|---|
| `store` | the agent's receiver: OTA slots, detached headers, otadata (below) |
| `node` | `arm_watchdog`, `start` (the link task), `running_version`, `now_ms` |
| `liveness`, `trial` | watchdog policy: no feed before CONFIRM, then deadline-based |
| `rom`, `serve` | ENTER_ROM and what follows a reply |
| `link` | link UART settings (`uart_config(baud)`: 8N1 at the bit rate the board's definition gives) |
| `links` | the node's uplinks as `Link`s (`Uart`, `UsbCdc`) and `LINK_KINDS` |
| `flash`, `boot`, `sys` | `Flash`, `BootSelectV` over otadata, `Clock` / `Watchdog` / `Reset` |
| `layout`, `partitions.csv`, `otadata`, `framer` | partition table (needs 8 MB flash or more), otadata, frame cutter |
| `board` | the `BoardDef` trait a board crate implements |

Reset on panic is on by default (feature `panic-reset`), so a trial image
that panics rolls back.

## Links

Every hop carries whole DAGON frames through one interface,
`dagon_mcu_common::link::Link`; the serve loops are generic over it. The
kinds this package offers (`links::LINK_KINDS`):

| Kind | Up | Down |
|---|---|---|
| `usb-cdc` | Yes | No |
| `uart` | Yes | No |
| `i2c` | NotYet | No |
| `ble` | NotYet | No |
| `wifi-tcp` | NotYet | No |
| `ethernet` | NotYet | No |
| `can` | NotYet | No |

- Up `uart`: UART0, 8N1 at the board's link bit rate (`links::Uart`), and up `usb-cdc`: the
  chip's USB Serial/JTAG (`links::UsbCdc`, `Reconnect::Reenumerates`);
  the node serves both at once.

### Generator contract

The kind names are exactly the 構成's: `usb-cdc`, `uart`, `i2c`, `ble`,
`wifi-tcp`, `ethernet`, `can`. In a project template:

- `{{uplink}}` renders to one Rust path naming the node's uplink kind:
  `dagon_mcu_esp32s3::links::Kind::<Variant>`;
- `{{downlinks}}` renders to the downlink kinds, comma-separated (empty
  for a 子機): `dagon_mcu_esp32s3::links::Kind::<Variant>, ...`.

Variants by name: `usb-cdc` `UsbCdc`, `uart` `Uart`, `i2c` `I2c`, `ble`
`Ble`, `wifi-tcp` `WifiTcp`, `ethernet` `Ethernet`, `can` `Can`. The
template checks them at compile time; a kind this package does not offer
in that direction stops the build with a message naming the hop, e.g.
`uplink ble: not available yet on esp32s3`:

```rust
use dagon_mcu_esp32s3::links::{self, Kind};

#[rustfmt::skip]
const UPLINK: Kind = {{uplink}};
#[rustfmt::skip]
const DOWNLINKS: &[Kind] = &[{{downlinks}}];
const _: () = links::require_uplink(UPLINK);
const _: () = links::require_downlinks(DOWNLINKS);
```

The node serves UART and USB Serial/JTAG at once whichever is named the
uplink (the check above still stops a kind not offered); link types are
constructed as
`dagon_mcu_esp32s3::links::<Kind>::new(..)`.

## Board layer

Pins and wiring are not here. A board crate (`targets/boards/*`, e.g.
`dagon-board-m5stopwatch`) hands `node::start` a `NodePeripherals`: the link
UART already set up on its pins with `link::uart_config(baud)`, USB Serial/JTAG,
the flash, the RTC timer, and a `&'static mut dyn BoardDef`:

- `bring_up()`: runs once in the link task before it serves (IO expander pin
  routing for the link). Displays and other peripherals are the app's.
- `pmic_download_supported()` / `pmic_download()`: ENTER_ROM method 2
  through the board's power manager, if it has one.

All three have defaults, so an ESP32-S3 board without extras needs only its
link UART pins, its node tag (`NodePeripherals::target_tag`) and
`board::PlainBoard`.

## Project template

`template/` is what `dagon gen` writes for an ESP32-S3 node chosen by MCU
only (the package is found by `[package.metadata.dagon] mcu = "esp32s3"`):
the same files as a board template, with a `src/main.rs` that wires the link
UART itself (the TX / RX pins are set there) and uses `node::plain_board()`
and the node tag from the generated `wiring.rs`.

## API notes

- Task liveness is deadline based: a task announces work with
  `Lease::busy(now, budget_ms)` and ends it with `Lease::idle()`; there is no
  bare `feed()`. A task that waits without a deadline is not blamed, a task
  stuck past its budget stops the watchdog feeding (owner-approved design).
- There is no `UserIo` bundle: the app keeps the peripherals returned by
  `esp_hal::init`, and the board package's macro (`dagon_pins!`) moves out
  only what DAGON takes.
- The ESP-IDF 2nd stage ships prebuilt and unmodified in `vendor/`
  (`vendor/bootloader.bin`; provenance and rebuild steps in
  `vendor/PROVENANCE.md`); the factory firmware is a binary of the board
  package, since it needs the board's pins.

## Receiver (`store::EspStore`)

- Body: written to the OTA partition of the slot (A = ota_0, B = ota_1) from
  its start; only the erase units the body covers are erased (64 KiB block
  erases where aligned, the watchdog fed between them).
- Header: detached, 512 B at the start of a 4 KiB sector in the `dagon`
  partition (type 0x40): +0x0000 A, +0x1000 B, +0x2000 factory. It is erased
  before the body and written last.
- SET_PENDING: an otadata trial entry (state New) for the slot, including the
  slot of the previously confirmed version (going back needs no transfer).
- CONFIRM: the running otadata entry becomes Valid (read back).
- STATUS from otadata: New = 2 pending, PendingVerify while running = 3
  trying, Valid / running = 4 confirmed, Aborted / Invalid = 5 reverted with
  last reason TRIAL_EXHAUSTED. On trial, `confirmed` is the slot the 2nd stage
  falls back to (factory when there is none). The 2nd stage tries once.
- Known limit: otadata keeps no image digest, so a slot re-received after a
  rollback keeps showing 5 until it is set pending again.

## Factory recovery agent

`factory` (a binary of the board package, e.g. `dagon-board-m5stopwatch`) is the same agent over the same
receiver, answering HELLO with mode 3. The 2nd stage boots it when otadata selects nothing bootable (blank,
or the only trial aborted). It verifies each image like the app does (header
signature, key, `mcu_id`, `target_tag` at the last XFER_HDR, before
anything is erased; body SHA-256 at XFER_END, before the header is written)
and receives into ota_0 or ota_1; SET_PENDING, REBOOT and CONFIRM then hand
over to the app. Factory itself is never a receive target: it is only
rewritten through the ROM loader.

## ROM entry (ENTER_ROM, cmd 0x24)

Every image (app and factory) answers ENTER_ROM `method` u8 with OK, then
switches to the ESP32 ROM download mode (E_ARG for other methods, nothing
happens). The switch happens only once the OK has left the node (UART
transmitter idle, or USB written and flushed, within 500 ms); if it did not,
the node stays in the app and the host retries.

- First, for either method, the RTC watchdog is stopped and the super
  watchdog left on auto-feed: both survive the software reset and nothing in
  the ROM loader feeds them over UART0, so the 4 s trial watchdog would
  otherwise reset the loader in the middle of flashing.
- method 1: sets `RTC_CNTL_OPTION1_REG.FORCE_DOWNLOAD_BOOT` and does a
  software reset (the RTC domain keeps the bit). If it does not take, the
  same reset brings the agent back; there is no automatic fallback.
- method 2: the board's power manager restarts the chip in download mode
  (`BoardDef::pmic_download`); answered E_UNSUPPORTED when the board reports no
  support (the default). For the M5StopWatch see its board crate.

On boards without external reset (the strapping pins are only sampled at a
chip reset) this cooperative path is the way into the ROM loader. A broken
ESP-IDF 2nd stage cannot be recovered this way.

## Tasks, priorities and the watchdog

- `node::start` runs the link task on an esp-rtos interrupt executor at
  priority 2 (software interrupt 1). The link task is the only owner of the
  link UART, USB Serial/JTAG, the flash, the RTC and super watchdogs and the
  board's `BoardDef` (e.g. an internal I2C bus). It waits on both links with interrupt-driven async reads and
  serves requests one at a time, so the agent is only ever touched there.
- The app's tasks run on the thread-mode executor (`#[esp_rtos::main]`),
  below every interrupt: DAGON traffic preempts them and never waits on them.
- Trial: the RTC WDT (4 s) is started once, right after `esp_hal::init`
  (`node::arm_watchdog`, whose token `node::start` requires). It is neither
  fed nor restarted until the trial status has been read, and not at all
  while the image is on trial. The PC has to CONFIRM within about 4 s of the
  start; otherwise the watch resets and the 2nd stage rolls back (one
  attempt).
- Once confirmed the link task feeds it (every 500 ms and during long flash
  work), but only while every task in the `Liveness` registry is within the
  deadline it announced (`Lease::busy(now, budget)` before a piece of work,
  `Lease::idle()` when waiting). A task stuck in its work stops the feeding
  and the watch resets instead of hanging. The link task announces a budget
  for each request and is registered as preempting: only while it holds the
  CPU in a synchronous section (handling a request, a slot erase of up to
  about 30 s, the CONFIRM check) the other tasks' deadlines are judged at
  the start of the section (one already passed stays passed), and afterwards
  they move later by that section's length.
  Time it spends awaiting (sending the reply, waiting for input) excuses
  nothing. ENTER_ROM stops the watchdogs before its reset (see above).
- Both receivers drop a partial frame after 20 ms or more without a byte and
  look for the next SOF. The bytes of one read share its completion time,
  and after a synchronous section of 20 ms or more any partial frame is
  dropped, since the FIFO may have collected bytes with gaps the read cannot
  see. Residual limit: a whole frame that arrived entirely during such a
  section is accepted even if its own bytes had gaps.

## Build and test

Build-time identity: `DAGON_PUBKEY` (path to the release public key file from
`dagon key gen`, or its 64 hex digits) and `DAGON_TARGET_TAG` (the topology
node tag; overrides the board's default). Without `DAGON_PUBKEY` the build fails, unless
`DAGON_ALLOW_TEST_KEY=1` is set: then the public RFC 8032 test key is built
in (TEST KEY, any image signed with its published secret is accepted) and
the build prints a warning. Use that only for test builds.

SET_PENDING while the running slot is on trial answers E_STATE. To go back
from a trial, REBOOT without CONFIRM: the 2nd stage rolls back.

```sh
# from this directory (espup toolchain)
cargo build --release                       # the library for Xtensa
# receiver, liveness, trial watchdog, ROM entry and factory flow on the host
# (test builds: the host tests sign with their own keys)
DAGON_ALLOW_TEST_KEY=1 cargo +stable test --lib --target x86_64-pc-windows-msvc
```

Features: `conform` answers CONF_RUN (CT-01/02/03/07/08) in front of the
agent, using the first erase unit of the OTA slot that is neither running
nor the fallback as scratch; `panic-reset` (default) resets on panic.

A board package's README covers its build, first install and
conformance runs (`targets/boards/m5stopwatch/README.md`).
