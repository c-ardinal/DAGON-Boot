# esp32s3-agent

DAGON side of the M5StopWatch (ESP32-S3, 16 MB): the library every watch
image links, and the factory recovery agent. The dagon-agent serves the rear
UART0 (G43 TX / G44 RX, 115200 8N1) and, for direct work over USB-C, the USB
Serial/JTAG port; requests are answered on the link they came in on. Built on
`targets/esp32s3-port`; the flashed partition table is `partitions.csv` here.

| Image | Where | Partition | HELLO mode |
|---|---|---|---|
| `factory` | this crate | factory | 3 (agent), the recovery target when otadata selects nothing bootable |
| `app` | `targets/esp32s3-app` | ota_0 / ota_1 | 0 (app) |

An app uses the public API only: `node::start(peripherals, link_irq,
NodeConfig::app(self_check), &LIVENESS)` after `esp_rtos::start`, and a
`liveness::Lease` per task of its own.

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

`factory` is the same agent over the same receiver, answering HELLO with
mode 3. The 2nd stage boots it when otadata selects nothing bootable (blank,
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
- method 2: the M5PM1 power manager's Download command (`SYS_CMD` 0x0C) on
  the internal I2C (G47/G48). Off by default and answered E_UNSUPPORTED: it
  waits for the M5PM1 I2C address and command byte to be confirmed by M5Stack
  documentation or on the hardware. The non-default feature `pmic-download`
  builds it with the assumed values (address 0x6E, byte 0xA3).

The watch has no external reset and its strapping pins are only sampled at a
chip reset, so holding G0 low from outside does not reach the ROM loader:
this cooperative path (or the power button held about 2 s) is the only one.
Not yet checked on the watch: that the ROM loader listens on the rear UART0
after method 1. A broken ESP-IDF 2nd stage cannot be recovered this way.

## Tasks, priorities and the watchdog

- `node::start` runs the link task on an esp-rtos interrupt executor at
  priority 2 (software interrupt 1). The link task is the only owner of
  UART0, USB Serial/JTAG, the flash, the RTC and super watchdogs and the
  internal I2C. It waits on both links with interrupt-driven async reads and
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
  about 30 s, the CONFIRM check) its own deadline alone counts, and
  afterwards the other tasks' deadlines move later by that section's length.
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
node tag; default `watch-s3`). Without `DAGON_PUBKEY` the build fails, unless
`DAGON_ALLOW_TEST_KEY=1` is set: then the public RFC 8032 test key is built
in (TEST KEY, any image signed with its published secret is accepted) and
the build prints a warning. Use that only for test builds.

SET_PENDING while the running slot is on trial answers E_STATE. To go back
from a trial, REBOOT without CONFIRM: the 2nd stage rolls back.

```sh
# from this directory (espup toolchain)
cargo build --release                       # factory
# receiver, liveness, trial watchdog, ROM entry and factory flow on the host
# (test builds: the host tests sign with their own keys)
DAGON_ALLOW_TEST_KEY=1 cargo +stable test --lib --target x86_64-pc-windows-msvc
```

Library features (the app forwards them): `conform` answers CONF_RUN
(CT-01/02/03/07/08) in front of the agent, using the first erase unit of the
OTA slot that is neither running nor the fallback as scratch;
`pmic-download` builds ENTER_ROM method 2.

The app build and the conformance runs (USB-C direct and through the hub)
are in `targets/esp32s3-app/README.md`.
