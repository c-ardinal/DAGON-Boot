# dagon-board-m5stopwatch

Board layer for the M5StopWatch (ESP32-S3R8, 16 MB flash): the pins and
extras on top of `dagon-mcu-esp32s3` (`targets/mcu/esp32s3`).

| What | Where |
|---|---|
| DAGON link | rear bus MUX_IO_1/2 = UART0 TX G43 / RX G44 (`rear_uart`), plus USB Serial/JTAG on USB-C |
| Rear pins UART/USB switch | M5IOE1 IO expander (I2C 0x4F) PYG1 on the internal I2C (SDA G47, SCL G48), driven low at bring-up |
| AMOLED reset | M5IOE1 PYG5, pulsed once at bring-up (low 10 ms, then high); then `node::display_ready()` |
| AMOLED bus | CO5300, QSPI on SPI2 + DMA channel 0: CS G39, SCK G40, D0-D3 G41/G42/G46/G45, TE G38 unused (`DisplayPins`, `display_pins!`); 466 x 466, column offset 6 |
| Power manager | M5PM1 on the internal I2C; ENTER_ROM method 2 only with the `pmic-download` feature (address 0x6E and command 0xA3 are unconfirmed assumptions) |
| Flash | 16 MB (`FLASH_SIZE`, espflash `--flash-size 16mb`); partition table from the MCU crate |
| Node tag | `watch-s3` (`TARGET_TAG`, the `DAGON_TARGET_TAG` default) |

`M5StopWatch` implements the MCU crate's `BoardDef`. It owns the internal I2C
bus and is moved into the DAGON link task by `start`, so nothing else on the
watch may use that bus.

## Adding DAGON to an app

The DAGON-specific lines of an app (see `samples/esp32s3-app` for a complete
one):

```rust
use dagon_board_m5stopwatch::{self as board, dagon_pins};
use dagon_mcu_esp32s3::{liveness::{Lease, Liveness}, node::{self, NodeConfig}};

static LIVENESS: Liveness = Liveness::new();

fn self_check() -> bool { true } // your health check before CONFIRM

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    let mut p = esp_hal::init(esp_hal::Config::default());
    let armed = node::arm_watchdog(p.RTC_TIMER.reborrow());
    esp_rtos::start(esp_hal::timer::timg::TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    board::start(armed, dagon_pins!(p), NodeConfig::app(self_check), &LIVENESS);
    // your tasks: take a lease with LIVENESS.register(), then
    // lease.busy(node::now_ms(), budget_ms) before work, lease.idle() after
}
```

The app is built with `DAGON_PUBKEY` (release key) and linked with
`esp_bootloader_esp_idf::esp_app_desc!()`. Without CONFIRM within about 4 s
of starting, a new image is rolled back.

For another ESP32-S3 board: copy this crate and change the pins in
`rear_uart` / `dagon_pins!`, the `BoardDef` extras (or use the MCU crate's
`PlainBoard`), `FLASH_SIZE` and `TARGET_TAG`.

## Factory firmware

Binary `factory`: DAGON's recovery agent alone for the factory partition
(`NodeConfig::factory()`, HELLO mode 3). The 2nd stage boots it when otadata
selects nothing bootable; it receives an image into ota_0 or ota_1 and hands
over with SET_PENDING, REBOOT and CONFIRM.

```sh
# from this directory (espup toolchain; DAGON_PUBKEY set to the release key)
cargo build --release --bin factory
```

## Project template

`template/` is what `dagon gen` writes for a node on this board (the package
is found by `[package.metadata.dagon] board = "m5stopwatch"`): `Cargo.toml`
with the `{{node}}` / `{{dep:<package>}}` placeholders, `.cargo/config.toml`,
`rust-toolchain.toml`, `src/main.rs` (the start-up above) and `src/app.rs`
(the developer's file). `src/wiring.rs` comes from the topology.

## Test apps

- `testapp-confirm`: marks its slot Valid right after boot, keeps feeding the
  4 s trial watchdog. Keys on either console: `0` / `1` try ota_0 / ota_1,
  `f` factory, `r` reset, `s` status.
- `testapp-noconfirm`: never confirms; the watchdog resets it after 4 s and
  the 2nd stage rolls back to the previous slot.

```sh
# from this directory (espup toolchain; DAGON_PUBKEY or DAGON_ALLOW_TEST_KEY=1)
cargo build --release
```

## Rollback check on the watch (USB-C)

Prerequisites:

- The rollback-enabled 2nd stage: `targets/mcu/esp32s3/vendor/bootloader.bin`
  (prebuilt, unmodified ESP-IDF v6.1; see `vendor/PROVENANCE.md` there).
- The test apps are built (`cargo build --release` here); `E` below is that
  release output directory.

Run from the repository root. The 16 MB flash size must be given every time:
the 2nd stage reads no further than the size in its header, and ota_1 starts
at 0x410000.

```sh
# release output directory of this crate
E=targets/boards/m5stopwatch/target/xtensa-esp32s3-none-elf/release
F="--chip esp32s3 --flash-size 16mb --partition-table-offset 0x8000 \
   --bootloader targets/mcu/esp32s3/vendor/bootloader.bin \
   --partition-table targets/mcu/esp32s3/partitions.csv"
espflash flash $F --erase-parts otadata --target-app-partition factory $E/testapp-confirm
espflash flash $F --target-app-partition ota_0 $E/testapp-confirm
espflash flash $F --target-app-partition ota_1 $E/testapp-noconfirm --monitor
```

Expected (one line per boot, `status[slot]` is the STATUS state:
3 trying, 4 confirmed, 5 reverted; reason 3 = TRIAL_EXHAUSTED):

1. `running=factory selected=factory state=None` (otadata blank).
2. Key `0`: reboot into `running=ota_0 ... state=Some(PendingVerify) status[ota_0]=3`,
   then `confirm -> Ok(())` and `state=Some(Valid) status[ota_0]=4`.
3. Key `1`: `testapp-noconfirm` with `running=ota_1 ... status[ota_1]=3`, a count
   to 3000 ms, a reset, then `testapp-confirm` with
   `running=ota_0 selected=ota_1 state=Some(Aborted) reset=Watchdog status[ota_1]=5 reason=3 ROLLED BACK from ota_1`.
4. Key `r`: still ota_0.
5. Key `f`: back to `running=factory selected=factory state=None` (otadata blank
   again). Then key `1`: from blank otadata the entry for ota_1 gets seq 2
   (not 0), and `testapp-noconfirm` starts with `running=ota_1 ...
   status[ota_1]=3`. After its reset there is no confirmed slot to fall back
   to, so the 2nd stage boots factory:
   `running=factory selected=ota_1 state=Some(Aborted) reset=Watchdog status[ota_1]=5 reason=3 ROLLED BACK from ota_1`.

If the monitor drops on a watchdog reset, reopen it (`espflash monitor`) and
press `s` for the status line.
