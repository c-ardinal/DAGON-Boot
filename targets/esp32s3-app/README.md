# esp32s3-app

Demo app of the M5StopWatch (ESP32-S3), the image that goes into ota_0 and
ota_1. DAGON comes in through the public API of `targets/esp32s3-agent`
only: `node::start` runs the agent's link task, and the app's own tasks
register in the `Liveness` registry.

| Task | Executor | Owns |
|---|---|---|
| DAGON link (`node::start`) | interrupt executor, priority 2 | UART0 (rear G43/G44), USB Serial/JTAG, flash, RTC/super watchdogs, internal I2C |
| `heartbeat` (demo) | thread mode (`#[esp_rtos::main]`), below every interrupt | nothing |
| `display_task` | thread mode | SPI2, DMA channel 0, AMOLED pins (CS G39, SCK G40, D0-D3 G41/G42/G46/G45) |

`heartbeat` is a placeholder: it counts a beat per second, each beat
announced to the watchdog with a 500 ms budget. CONFIRM is accepted once at
least one beat has completed. Watchdog, trial and ENTER_ROM behaviour
are those of the DAGON side (`targets/esp32s3-agent/README.md`).

`display_task` shows the running version (`node::running_version()`, from
the image header) in large seven-segment digits on the 466×466 AMOLED
(CO5300 over QSPI, driver crate `esp-hal-co5300`). The panel reset is on the
internal I2C expander, which the DAGON side owns: it pulses the reset once at
start and then sets `node::display_ready()`; the task waits for that, runs
the panel initialisation (about 1.5 s of fixed waits, once) and draws once,
with a liveness budget per step. It never talks to the internal I2C and the
link task preempts it at any time. If the panel does not answer, the screen
stays dark and nothing else changes. `src/digits.rs` lays the digits out
(host tested): light strokes on true black, kept inside the round screen,
as large as the line allows; `---` when the version is unknown.

Not yet checked on the watch: the panel initialisation (the crate's default
sequence, column offset 6) and whether the AMOLED supply is on by default.

## Build

Build-time identity as for the DAGON side: `DAGON_PUBKEY` (path to the
release public key file, or its 64 hex digits) and `DAGON_TARGET_TAG`
(default `watch-s3`). Without `DAGON_PUBKEY` the build fails unless
`DAGON_ALLOW_TEST_KEY=1` opts in to the public RFC 8032 test key (test builds
only; the build then warns).

```sh
# from this directory (espup toolchain)
cargo build --release                                              # app
cargo build --release --features conform --target-dir target/conform
cargo build --release --features conform,no-confirm --target-dir target/noconfirm
# digit layout on the host
DAGON_ALLOW_TEST_KEY=1 cargo +stable test --lib --target x86_64-pc-windows-msvc
```

Features: `conform` (the node answers CONF_RUN), `no-confirm` (refuses
CONFIRM, so its trial always rolls back), `pmic-download` (ENTER_ROM method
2, unconfirmed values).

## Conformance run over USB-C (direct)

Prerequisites: the rollback-enabled 2nd stage
(`targets/esp32s3-2ndstage/build/bootloader/bootloader.bin`), the factory
(`targets/esp32s3-agent`) and the two app builds above, all made with
`DAGON_PUBKEY` set to the signing key's public key, and the `dagon` CLI with
that key. Run from the repository root.

1. Raw app images (what goes into the OTA partitions):

   ```sh
   R=targets/esp32s3-agent/target/xtensa-esp32s3-none-elf/release
   C=targets/esp32s3-app/target/conform/xtensa-esp32s3-none-elf/release
   N=targets/esp32s3-app/target/noconfirm/xtensa-esp32s3-none-elf/release
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
   a few seconds (4 s WDT without CONFIRM + two boots) with A state 5 and last
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
