# esp32s3-port

DAGON port for the M5StopWatch (ESP32-S3, 16 MB flash), kind V: the stock
ESP-IDF 2nd stage built with app rollback (`targets/esp32s3-2ndstage`) picks
the slot, this crate drives otadata.

| Module | Provides |
|---|---|
| `flash` | `Flash` over ota_0, ota_1 and the dagon partition (4 KiB erase, 4 B write, read-back) |
| `link` | `Link` on the rear UART0, G43 TX / G44 RX, 115200 8N1 |
| `boot` | `BootSelectV` over otadata (0 = ota_0, 1 = ota_1, 2 = factory), `confirm` |
| `sys` | `Clock`, `Watchdog` (RTC WDT, system reset), `Reset` |
| `otadata` | otadata selection to STATUS slot state; an aborted trial is Reverted / TRIAL_EXHAUSTED |
| `board` | PYG1 routing of the rear pins to UART0, console, reset on panic |

Test apps:

- `testapp-confirm`: marks its slot Valid right after boot, keeps feeding the
  4 s trial watchdog. Keys on either console: `0` / `1` try ota_0 / ota_1,
  `f` factory, `r` reset, `s` status.
- `testapp-noconfirm`: never confirms; the watchdog resets it after 4 s and
  the 2nd stage rolls back to the previous slot.

## Build and test

```sh
# from this directory (espup toolchain)
cargo build --release
# pure modules on the host
cargo +stable test --lib --target x86_64-pc-windows-msvc
```

## Rollback check on the watch (USB-C)

From the repository root, with `targets/esp32s3-2ndstage` built (`idf.py bootloader`).
The 16 MB flash size must be given every time: the 2nd stage reads no further
than the size in its header, and ota_1 starts at 0x410000.

```sh
E=targets/esp32s3-port/target/xtensa-esp32s3-none-elf/release
F="--chip esp32s3 --flash-size 16mb --partition-table-offset 0x8000 \
   --bootloader targets/esp32s3-2ndstage/build/bootloader/bootloader.bin \
   --partition-table targets/esp32s3-agent/partitions.csv"
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
4. Key `r`: still ota_0. Key `f`: back to factory.
