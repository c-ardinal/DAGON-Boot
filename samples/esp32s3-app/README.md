# esp32s3-app (sample)

Demo app of the M5StopWatch (ESP32-S3), the image that goes into ota_0 and
ota_1. It is a sample, not part of DAGON: DAGON comes in through the
board package `targets/boards/m5stopwatch` (`board::start`, `dagon_pins!`)
and the public API of `targets/mcu/esp32s3` only:
`board::start` runs the agent's link task, and the app's own tasks register
in the `Liveness` registry.

| Task | Executor | Owns |
|---|---|---|
| DAGON link (`board::start`) | interrupt executor, priority 2 | UART0 (rear G43/G44), USB Serial/JTAG, flash, RTC/super watchdogs, internal I2C |
| `heartbeat` (demo) | thread mode (`#[esp_rtos::main]`), below every interrupt | nothing |
| `display_task` | thread mode | SPI2, AMOLED pins (CS G39, SCK G40, D0-D3 G41/G42/G46/G45, TE G38) |

`heartbeat` is a placeholder: it counts a beat per second, each beat
announced to the watchdog with a 500 ms budget. CONFIRM is accepted once at
least one beat has completed. Watchdog, trial and ENTER_ROM behaviour
are those of the MCU package (`targets/mcu/esp32s3/README.md`).

`display_task` shows the running version (`node::running_version()`, from
the image header) in large seven-segment digits on the 466×466 AMOLED
(CO5300 over QSPI): light digits on a black screen, brightness 0x60, drawn
once. `src/digits.rs` lays the digits out (host tested): kept inside the
round screen, as large as the line allows; `---` when the version is
unknown. The task runs below the DAGON link task with a liveness budget per
step; if the panel has no power or does not take the commands the screen
stays dark and nothing else changes.

### Panel power and initialisation

The display belongs to this app, not to DAGON. Its power and reset lines sit
on the internal I2C bus (I2C0, SDA G47, SCL G48), which DAGON's link task
owns once `board::start` has run, so `main` does the power part first
(`src/power.rs`, on `p.I2C0.reborrow()`). The steps follow M5Stack's
M5StopWatch-UserDemo V0.5 (`hal_pmic.cpp`, `hal_ioe.cpp`, M5GFX
`Panel_AMOLED`); this sequence lights the panel on the watch, from power-on
and after a chip reset:

1. M5PM1 (0x6E), 100 kHz: I2C sleep off (0x09 bits 3:0 = 0), LDO hold
   (0x07 bit 5), PMG2 (the charger's CHG_STAT) a plain input (0x16 bits 5:4,
   0x10 bit 2 and 0x14 bits 5:4 cleared), watchdog off (0x0A = 0). The
   official demo's charging and power-button settings are left out, and so
   is PMG3 (the charger's CHG_PROG, which the demo drives low).
2. M5IOE1 (0x4F): a wake access, then I2C_CFG 0x23 = 0x10 (400 kHz). Right
   after power-on (power button, battery) the expander does not answer yet:
   as the official `M5IOE1::begin`, the app then waits 800 ms, wakes it
   again and tries once more. The ESP side switches to 400 kHz, waits 50 ms
   and wakes it again. At 400 kHz the expander now and then answers garbage,
   so every write is read back and retried.
3. M5IOE1 IO1, IO3, IO4, IO5, IO8, IO9, IO10 as push-pull outputs (pull-ups
   and pull-downs off); IO3, IO4, IO5 (AMOLED reset, released without a
   pulse), IO8 (L3B_EN, the panel's LDO) high; IO1 (rear pins to UART) and
   IO10 low. IO8 is read back from the input register every 80 ms until it
   is high (10 tries), then 50 ms.
4. Panel, SPI2 mode 0 at 80 MHz: a command is opcode 0x02 plus the 24-bit
   address `cmd << 8` and its arguments on one line; pixels are opcode 0x32
   with `00 2C 00` (then `3C` for the next chunks) and RGB565 on four lines.
   `11`, 150 ms, `C4 80`, `35 80`, `44 01 D2`, `53 20`, `20`, `36 00`,
   `51 A0`, `29`, `3A 55`, `36 00`, `51 60`. Columns start at 6 in the
   controller's memory (the clear covers 468 columns). Reading the panel
   returns zeros and tells nothing.

After an update the chip only resets: the power manager and the expander
keep their state and nothing turns the panel off, so the app runs this whole
sequence again on every start rather than relying on what the previous image
left behind.

Each start logs this sequence on the USB console before DAGON takes it (ESP
reset reason, the M5PM1 and M5IOE1 register reads, every write result, the
check at which IO8 read high; `src/bootlog.rs`) and keeps a 32-byte snapshot
of it in the M5PM1's RTC RAM (0xA0), which the next start prints first. A
start from the power button enumerates USB anew, so its own lines are
usually lost; the snapshot printed by the next start (a reset from the PC)
shows how it went. The console never blocks: with no PC reading, the lines
are dropped.

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
(`targets/mcu/esp32s3/vendor/bootloader.bin`, prebuilt; see
`vendor/PROVENANCE.md` there), the factory firmware (binary `factory` of
`targets/boards/m5stopwatch`) and the two app builds above, all made with
`DAGON_PUBKEY` set to the signing key's public key, and the `dagon` CLI with
that key. Run from the repository root.

1. Raw app images (what goes into the OTA partitions):

   ```sh
   R=targets/boards/m5stopwatch/target/xtensa-esp32s3-none-elf/release
   C=samples/esp32s3-app/target/conform/xtensa-esp32s3-none-elf/release
   N=samples/esp32s3-app/target/noconfirm/xtensa-esp32s3-none-elf/release
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
      --bootloader targets/mcu/esp32s3/vendor/bootloader.bin \
      --partition-table targets/mcu/esp32s3/partitions.csv"
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
