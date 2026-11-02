# ESP-IDF 2nd stage bootloader (prebuilt, unmodified)

`bootloader.bin` here is the ESP-IDF second-stage bootloader for the
ESP32-S3, built from the official ESP-IDF v6.1 sources without any change to
them. DAGON supplies only the sdkconfig options in `sdkconfig.defaults` here
and the minimal project files in `targets/esp32s3-2ndstage`. It is licensed
under the Apache License 2.0 (`LICENSE-ESP-IDF`, the `LICENSE` file of the
same ESP-IDF commit; copyright Espressif Systems (Shanghai) Co., Ltd. and the
ESP-IDF contributors).

| File | Content |
|---|---|
| `bootloader.bin` | `build/bootloader/bootloader.bin` of the build below, copied unchanged |
| `sdkconfig.defaults` | the only sdkconfig options set (read by `targets/esp32s3-2ndstage`); every other option is the ESP-IDF v6.1 default |
| `SHA256SUMS` | `sha256sum bootloader.bin` |
| `LICENSE-ESP-IDF` | ESP-IDF `LICENSE` (Apache-2.0) |

| Item | Value |
|---|---|
| ESP-IDF version | `v6.1` (`git describe --tags --exact-match`) |
| ESP-IDF commit | `fff9895c82d744c7237be8847347bdd1b07c6643` (tree and submodules clean) |
| Source | `https://github.com/espressif/esp-idf.git`; tools at the versions ESP-IDF v6.1 pins (`tools/tools.json`) |
| Toolchain | `xtensa-esp-elf` 15.2.0 (`esp-15.2.0_20251204`) |
| Build host and date | Windows 11, 2026-11-03 |
| SHA-256 | `75e529456be566edb74e7e1436a45b614133d8d41896e747e9d6204329e8968f` |

## sdkconfig options (`sdkconfig.defaults`)

| Option | Why |
|---|---|
| `CONFIG_IDF_TARGET="esp32s3"` | chip |
| `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE=y` | otadata trial states: a slot left PendingVerify is aborted at the next boot |
| `CONFIG_ESPTOOLPY_FLASHMODE_DIO=y`, `CONFIG_ESPTOOLPY_FLASHFREQ_80M=y`, `CONFIG_ESPTOOLPY_FLASHSIZE_16MB=y` | the M5StopWatch flash (16 MB); written into the image header |
| `CONFIG_PARTITION_TABLE_OFFSET=0x8000` | where the 2nd stage reads the partition table |
| `CONFIG_PARTITION_TABLE_CUSTOM=y`, `..._FILENAME="../mcu/esp32s3/partitions.csv"` | builds the DAGON table alongside, as a check of `partitions.csv` (otadata 0xD000, factory 0x10000, ota_0 0x110000, ota_1 0x410000, dagon 0x710000); the 2nd stage itself reads the table from flash |
| `CONFIG_APP_REPRODUCIBLE_BUILD=y` | no build date, time or host paths in the binary |

## Build (reproducible)

PowerShell, ESP-IDF v6.1 checked out at `$env:IDF_PATH` and its tools
installed (`install.ps1 esp32s3`). Run outside MSYS / Git Bash (the export
script refuses an `MSYSTEM` environment). The project's `CMakeLists.txt`
takes its options from this directory's `sdkconfig.defaults` (the single
source, target included), so a plain build there is enough. Its `build/`,
`sdkconfig` and `dependencies.lock` are ignored by git; delete `sdkconfig`
first if one is left from an earlier build.

```powershell
. "$env:IDF_PATH\export.ps1"
cd <repo>\targets\esp32s3-2ndstage
idf.py build
git -C $env:IDF_PATH describe --tags --exact-match   # v6.1
git -C $env:IDF_PATH rev-parse HEAD                  # commit above
Copy-Item build\bootloader\bootloader.bin ..\mcu\esp32s3\vendor\
```

Three clean builds gave the SHA-256 above: two out of tree (`idf.py -C
<project> -B <dir> -D SDKCONFIG=<dir>\sdkconfig ... bootloader`, in different
directories) and one plain `idf.py build` in the project. Rebuilding
with the same ESP-IDF commit, toolchain and `sdkconfig.defaults` must give the
same SHA-256; a different one means a different toolchain or source.

Check (`esptool` 5.6.0 from the same ESP-IDF environment):

```sh
python -m esptool --chip esp32s3 image-info bootloader.bin
# Chip ID: 9 (ESP32-S3), Flash size: 16MB, Flash freq: 80m, Flash mode: DIO,
# checksum and validation hash valid, ESP-IDF: v6.1
python $IDF_PATH/components/partition_table/gen_esp32part.py <build>/partition_table/partition-table.bin
# otadata 0xd000, factory 0x10000, ota_0 0x110000, ota_1 0x410000, dagon 0x710000
```

## Flashing

espflash writes it at 0x0 with the partition table and an app:

```sh
espflash flash --chip esp32s3 --flash-size 16mb --partition-table-offset 0x8000 \
  --bootloader targets/mcu/esp32s3/vendor/bootloader.bin \
  --partition-table targets/mcu/esp32s3/partitions.csv <app ELF>
```

`--flash-size 16mb` is required every time: espflash writes the size into
the bootloader header, and the 2nd stage reads no further than that.
