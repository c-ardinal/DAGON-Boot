# esp32s3-agent

DAGON node for the M5StopWatch (ESP32-S3, 16 MB): the dagon-agent serving
the rear UART0 (G43 TX / G44 RX, 115200 8N1) and, for direct work over
USB-C, the USB Serial/JTAG port. Requests are answered on the link they came
in on. Built on `targets/esp32s3-port`.

| Binary | Partition | HELLO mode |
|---|---|---|
| `app` | ota_0 / ota_1 | 0 (app) |
| `factory` | factory | 3 (agent), the recovery target when otadata selects nothing bootable |

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

Both binaries answer ENTER_ROM `method` u8 with OK, then switch to the ESP32
ROM download mode (E_ARG for other methods, nothing happens):

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

## Trial watchdog

The RTC WDT (4 s) runs from start-up and the serving loop feeds it. On trial
the app feeds it for 15 s only; a CONFIRM inside that window ends the trial,
otherwise the watch resets and the 2nd stage rolls back.

## Build and test

Build-time identity: `DAGON_PUBKEY` (path to the release public key file from
`dagon key gen`, or its 64 hex digits; unset, the public RFC 8032 test key is
built in and the build prints a warning) and `DAGON_TARGET_TAG` (the topology
node tag; default `watch-s3`).

SET_PENDING while the running slot is on trial answers E_STATE. To go back
from a trial, REBOOT without CONFIRM: the 2nd stage rolls back.

```sh
# from this directory (espup toolchain)
cargo build --release                                             # app, factory
cargo build --release --features conform --target-dir target/conform
cargo build --release --features conform,no-confirm --target-dir target/noconfirm
# receiver and trial watchdog on the host
cargo +stable test --lib --target x86_64-pc-windows-msvc
```

Features: `conform` answers CONF_RUN (CT-01/02/03/07/08) in front of the
agent, using the first erase unit of the OTA slot that is neither running nor
the fallback as scratch. `no-confirm` refuses CONFIRM (E_STATE), so its trial
always rolls back.

## Conformance run over USB-C (direct)

Prerequisites: the rollback-enabled 2nd stage
(`targets/esp32s3-2ndstage/build/bootloader/bootloader.bin`), the three builds
above made with `DAGON_PUBKEY` set to the signing key's public key, and the
`dagon` CLI with that key. Run from the repository root.

1. Raw app images (what goes into the OTA partitions):

   ```sh
   R=targets/esp32s3-agent/target/xtensa-esp32s3-none-elf/release
   C=targets/esp32s3-agent/target/conform/xtensa-esp32s3-none-elf/release
   N=targets/esp32s3-agent/target/noconfirm/xtensa-esp32s3-none-elf/release
   espflash save-image --chip esp32s3 --flash-size 16mb $C/app out/watch-good.bin
   espflash save-image --chip esp32s3 --flash-size 16mb $N/app out/watch-noconfirm.bin
   ```

2. Signed images: `good` twice with different versions, `no_confirm` with a
   third one.

   ```sh
   dagon image sign out/watch-good.bin --node watch --version 1.0.0 --out out/watch-good-1.0.0.img
   dagon image sign out/watch-good.bin --node watch --version 1.1.0 --out out/watch-good-1.1.0.img
   dagon image sign out/watch-noconfirm.bin --node watch --version 1.2.0 --out out/watch-noconfirm-1.2.0.img
   ```

3. First install over USB-C. The 16 MB flash size and the table offset are
   needed every time.

   ```sh
   F="--chip esp32s3 --flash-size 16mb --partition-table-offset 0x8000 \
      --bootloader targets/esp32s3-2ndstage/build/bootloader/bootloader.bin \
      --partition-table targets/esp32s3-agent/partitions.csv"
   espflash flash $F --erase-parts otadata,dagon --target-app-partition factory $R/factory
   # good 1.0.0 into ota_0: body at the partition, header in the dagon partition
   head -c 512 out/watch-good-1.0.0.img > out/hdr.bin
   tail -c +513 out/watch-good-1.0.0.img > out/body.bin
   espflash write-bin 0x110000 out/body.bin
   espflash write-bin 0x710000 out/hdr.bin
   # otadata entry 0: seq 1 (ota_0), state Valid, CRC of seq 1
   { printf '\001\000\000\000'; printf '\377%.0s' $(seq 20); printf '\002\000\000\000\232\230\103\107'; } > out/otadata.bin
   espflash write-bin 0xD000 out/otadata.bin
   ```

4. Check over the USB-C serial port: HELLO answers mode 0, version 1.0.0,
   trial 0; STATUS shows slot A state 4, `active` 0, `confirmed` 0.

5. Run the conformance sequence against the USB-C serial port with
   `good = [watch-good-1.1.0.img, watch-good-1.1.0.img]` and
   `no_confirm = [watch-noconfirm-1.2.0.img, watch-noconfirm-1.2.0.img]`.
   The agent announces the UART `max_payload` (1024) on both links, so the
   PC side must treat this direct link as UART for CT-18. Expected: CT-11
   ends on 1.1.0 in B confirmed (A written, 1.0.0); CT-12 returns to B within
   about 20 s (15 s window + 4 s WDT + two boots) with A state 5 and last
   reason 3 (TRIAL_EXHAUSTED).

The 2nd stage prints its boot log on UART0 and USB at every reset; the frame
cutter skips it.

## Conformance run through the hub (rear UART)

The result that counts is the run through the hub. Same images and first
install as above (steps 1 to 4, over USB-C), then:

1. Wiring: hub PIO UART GPIO26 (TX) to the watch's rear G44 (RX), hub GPIO27
   (RX) to rear G43 (TX), and GND. 115200 8N1. The app routes the rear pins
   to UART0 at start-up.
2. Charge the watch, unplug USB-C (rear UART and USB are not used at the same
   time); the watch runs on its battery.
3. With the hub app on USB to the PC: `dagon conform --node watch` (the node
   id from the topology), images as in step 5 above. The hub relays every
   request over the PIO UART, so `max_payload` matches the UART link.
   Expected as above; the run leaves the watch on the other slot, confirmed.
4. Factory path: select factory (blank otadata, for example by
   `espflash erase-region 0xD000 0x2000` over USB-C), plug out USB-C again,
   then `dagon recover watch`: HELLO answers mode 3, the image goes into
   ota_0 or ota_1, SET_PENDING, REBOOT, and the app answers mode 0 on trial
   until CONFIRM.
