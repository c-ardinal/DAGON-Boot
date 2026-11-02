# ESP-IDF 2nd stage bootloader (prebuilt, unmodified)

`bootloader.bin` here is the ESP-IDF second-stage bootloader for the
ESP32-S3, built from the official ESP-IDF sources without any change to
them. DAGON only supplies the project files and `sdkconfig.defaults` in
`targets/esp32s3-2ndstage` (app rollback on, 16 MB flash). It is licensed
under the Apache License 2.0 (`LICENSE-ESP-IDF`, copyright Espressif
Systems (Shanghai) Co., Ltd. and the ESP-IDF contributors).

## Status

Not added yet: ESP-IDF is not installed on the machine this package was
prepared on, and binaries are not taken from elsewhere. Until the files
below exist, build the 2nd stage yourself with the steps that follow.

| File | Content |
|---|---|
| `bootloader.bin` | `targets/esp32s3-2ndstage/build/bootloader/bootloader.bin`, copied unchanged |
| `sdkconfig` | `targets/esp32s3-2ndstage/sdkconfig` written by the same build |
| `SHA256SUMS` | `sha256sum bootloader.bin sdkconfig` |
| this file | the ESP-IDF version and commit below filled in |

| Item | Value |
|---|---|
| ESP-IDF version | v6.1 (fill in the exact tag) |
| ESP-IDF commit | (fill in: `git -C $IDF_PATH rev-parse HEAD`) |
| Build host and date | (fill in) |
| `sdkconfig.defaults` | `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE=y`, `CONFIG_ESPTOOLPY_FLASHSIZE_16MB=y` |

## Build steps (reproducible)

```sh
# ESP-IDF v6.1 installed and exported (IDF_PATH set, export script run)
cd targets/esp32s3-2ndstage
idf.py set-target esp32s3
idf.py bootloader
git -C "$IDF_PATH" describe --tags --exact-match   # version
git -C "$IDF_PATH" rev-parse HEAD                  # commit
cp build/bootloader/bootloader.bin ../mcu/esp32s3/vendor/bootloader.bin
cp sdkconfig ../mcu/esp32s3/vendor/sdkconfig
cd ../mcu/esp32s3/vendor && sha256sum bootloader.bin sdkconfig > SHA256SUMS
```

Rebuilding with the same ESP-IDF commit and `sdkconfig` must give the same
SHA-256; a different one means a different toolchain or source, not a
DAGON change.

## Flashing

espflash writes it at 0x0 with the partition table and an app:

```sh
espflash flash --chip esp32s3 --flash-size 16mb --partition-table-offset 0x8000 \
  --bootloader targets/mcu/esp32s3/vendor/bootloader.bin \
  --partition-table targets/mcu/esp32s3/partitions.csv <app ELF>
```

`--flash-size 16mb` is required every time: espflash writes the size into
the bootloader header, and the 2nd stage reads no further than that.
