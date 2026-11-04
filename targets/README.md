# MCU and board definitions

Every MCU package (`targets/mcu/<id>/`) carries `dagon-mcu.toml` and every
board package (`targets/boards/<id>/`) carries `dagon-board.toml`. They hold
what DAGON knows about the MCU or board; DAGON's code keeps only the closed
sets of kinds and the engines that use them. `dagon-topology::defs` loads and
validates them into a `Registry`:

- `Registry::builtin()`: every `targets/mcu/*/dagon-mcu.toml` and
  `targets/boards/*/dagon-board.toml` of this tree, found by folder and
  embedded at build time in folder-name order (no filesystem; the browser
  build uses this). Adding a package folder adds its definition.
- `Registry::load(&[Source::Tree(root), Source::Project(dagon_toml)])`: a
  DAGON source tree's definitions replace the builtin set; a project adds its
  own. A project definition may not reuse an existing MCU or board id.

Numbers are TOML integers (`0x0800_8000`, `400_000`). Unknown fields are
errors; kind fields accept only the values listed below.

## dagon-mcu.toml

| Key | Meaning |
|---|---|
| `format` | Definition format number (`1`; left out means 1). A dagon that does not read the number refuses the file (docs/versioning.md) |
| `id` | MCU id used in topologies (`stm32f401re`) |
| `name` | Display name (`STM32F401RE`) |
| `wire_id` | Image header `mcu_id`; unique |
| `port` | `self-boot` or `vendor-stage2` |
| `gpio_count` | GPIO0..n-1 checked for nodes without a board; 0 when pins are named by port |
| `[[flash_map]]` | `name` (unique), `base` (address of flash byte 0), `bytes`. The first is the default |
| `partitions` | ESP-IDF partition table CSV in the package (rows need explicit offset and size) |
| `[layout]` | Optional fixed regions: `stage0 = { at, size }`, `bootcore_log` / `app_log = { faces = [a, b], face_size }` |
| `[[slot]]` | `image` (`app`, `bootcore`, `agent`, `stage0`), `binding` (`a`, `b`, `any`), `header` (`inline`, `trailer`, `detached`); the region is `at` + `size` or `partition`; a detached header is at `header_at` or `header_partition` + `header_offset` |
| `[writer]` | First-write tool: `kind` and its parameters (below) |
| `[[part]]` | DAGON parts built from this package for a first write: `name`, `sub` (folder in the package), `bin` |
| `[coordinator]` | Only for an MCU that can be the coordinator: `pio_state_machines`, `pio_uart_state_machines` (per link), `uart0_tx`, `uart0_rx` |
| `[rom_i2c]` | ROM I2C bootloader (`stm32-i2c-rom`): `name` (as check messages call it), `addr`, `max_hz`, `erase_sectors`, `pairs = [{ controller, sda, scl }]` it listens on |
| (coordinator) | A topology may make the MCU its coordinator only when `[coordinator]` is defined (V-25) |
| `[connection]` | How the user connects it for a write: `label`, `hint` |
| `[[doctor]]` | Tool check: `what`, `probe` (command), `expect` (`"runs"`, `{ line = "..." }`, `{ prefix = "..." }`), `fix` (command), `hint`, `slow` (`true`: `dagon doctor --fix` says the install takes a while before it starts) |
| `ram` | `{ at, size }`: all of the MCU's RAM. Images get the RAM below the boot record, the fuse cells and the extension regions |
| `[watchdog]` | `kind` (`iwdg`, `rp-watchdog`, `rtc-wdt`), `trial_ms` (armed on a trial boot), `confirmed_ms` (armed otherwise). An app may delay a reboot by at most half the shorter one |
| `[isolation]` | How DAGON keeps extensions away from itself (below). Left out: `kind = "none"`, `level = "none"` |
| `[[core]]` | Every processor core, `id`; DAGON runs on the first |
| `[[peripheral]]` | Every register block: `id`, `base`, `size`, `window_eligible` (an extension may get it as a window), `bus_master` (it can write memory), `privileged_why` (one or a list of `dma`, `flash`, `watchdog`, `clock`, `reset`, `irq-routing`, `pinmux`, `power`, `rom`, `core-control`, `protection`, `shared`) |
| `[boot.record]` | MCUs with a DAGON boot stage: where the boot record and the lockup fuse live. `kind` (`noinit-ram` inside `ram`, `retained-registers` inside a `[[peripheral]]`), `at`, `size` (at most 4096), `survives` (`reset-random`, `reset-cleared`, `power`: what a power loss leaves), `rings = [{ writer, at, size }]` (one per boot stage, in boot order, inside the record), `fuse = { cells = [{ image, at, size }] }` (`bootcore`, `app`; outside the record), `reserved` (addresses the record never stores to). The byte layout inside is the `boot-record` contract's |
| `boot_capabilities` | What a board's boot action program may use besides pins: `["i2c"]` allows `bus_write` |

Slot geometry: `inline` puts the 512-byte header at the slot start and the
payload after it; `trailer` puts the payload at the slot start and the
header in the last 512 bytes; `detached` puts the header elsewhere and the
payload fills the slot. `any` slots answer for slot A.

Writer kinds:

- `uf2-drive`: `family`, `sector`, `info_file`, `info_header`, `board_id`
  (the boot ROM drive is the one whose info file starts with `info_header`
  and has the line `Board-ID: <board_id>`).
- `probe-rs`: `chip`.
- `espflash`: `chip`, `flash_size`, `table_at`, `bootloader`, `checksums`,
  `usb_hint = { vid, pid }`. The hint only labels candidates the writer
  tool reports; it never filters them. Matching candidates are listed first and
  pre-selected.

### `[isolation]`

| Key | Meaning |
|---|---|
| `level` | What DAGON enforces: `none`, `mpu`, `pms`, `tz`. A kind gives `none` or one level (`mpu` for the MPU and PMP kinds, `pms` for `esp-pms`, `tz` for `armv8m-tz`) |
| `kind` | The protection hardware: `armv6m-mpu`, `armv7m-mpu`, `armv8m-mpu`, `armv8m-tz`, `riscv-pmp`, `riscv-epmp`, `esp-pms`, `none` |
| `boot_extensions` | The MCU has a DAGON boot stage that runs extensions (`port = "self-boot"` only) |
| `ext_ram_max` | Most RAM the sandboxed extensions of one image take, regions rounded up (required when `kind` is not `none`) |
| `root_protection` | Root-image protection `dagon provision` may set: `rdp1` (reversible), `rdp2` (permanent), `wrp` (reversible), `esp-secure-boot`, `esp-flash-encryption` (permanent) |
| `cache` | `none`, `i-cache`, `id-cache` (instruction and data) |
| `tcm` | Tightly coupled memories, `[{ at, size }]` |
| `[isolation.capabilities]` | The kind's capability record, read back from the hardware at boot (a mismatch turns the level to `none`). `armv6m-mpu`: `mpu_regions` 0 or 8, `min_region` at least 256, `subregions_from`; `armv7m-mpu`: `mpu_regions` 8 or 16, `min_region` at least 32, `subregions_from`; `armv8m-mpu`: `mpu_regions` 0, 4, 8, 12 or 16; `armv8m-tz`: `sau_regions` (0, 4, 8), `s_mpu_regions`, `ns_mpu_regions`, `idau = [{ base, size, attribution }]`; `riscv-pmp`, `riscv-epmp`: `pmp_entries` (0, 16, 64), `grain`, `na4`, `tor`; `esp-pms`: `worlds`, `gdma_channels`, `memory_regions`, `peripheral_slots`. Every kind: `critical_section` (`primask` on armv6m, `basepri` on armv7m and armv8m, `mstatus-mie` on RISC-V, `xtensa-level` on esp-pms), `unwind` (`cortex-m-exception`, `armv8m-tz`, `riscv-trap`, `xtensa-world`, by kind), `timer` (`systick`, `riscv-mtime`, `timer-group`), `reset_cleanup` (`peripheral-reset`, `reinit`), `background_privileged`, `fault_address`; optionally `np_max_us` and `dma_stop_max_us`, which may only lower DAGON's bounds (50 and 20) |

A definition whose `kind` is not `none` lists every core and at least one
block of each class `flash`, `watchdog`, `clock`, `reset` and
`irq-routing` (also `core-control` with more than one core, and
`protection` for `esp-pms` and `riscv-epmp`); a bus master is never
window-eligible, and every block that is not window-eligible says why. A
definition that leaves a class out is refused with the class named.

## dagon-board.toml

| Key | Meaning |
|---|---|
| `format` | As in `dagon-mcu.toml` |
| `id` | Board id used in `nodes[].board`; the first board of an MCU is its default |
| `product`, `mcu`, `flash_bytes`, `drawing` | Product name, MCU id (must be defined), flash size (must hold one of the MCU's flash maps), UI drawing |
| `i2c_target` | Whether the board can be an I2C target |
| `header` | `[{ pin, label }]`; `label` left out until confirmed on the board |
| `i2c` | `[{ controller, sda, scl }]` pairs usable from the header |
| `[default_link]` | How a node board is wired unless the topology says otherwise: `kind` (`i2c`, `uart`, `can`, `ethernet`), `iface` (`hw-uart0`, `pio-uart`), `addr`, `baud`, `control` (`full`, `cooperative`), `rom_entry` (`force-download-boot`, `m5pm1`, `stm32-i2c-rom`). Left out for the coordinator |
| `[coordinator]` | Pins the page wires new links to when this board is the 親機: `i2c = { controller, sda, scl }` (a new I2C bus; `controller` is one of the board's `i2c`) and `pio_uart = { tx, rx }` (a new PIO UART link), numbered GPIOs below the MCU's `gpio_count`. Only on boards whose MCU has `[coordinator]` |
| `[[part]]` | Parts built from the board package itself (`name`, `sub`, `bin`) |
| `[boot]` | The boot stage's board values: `led` (a `[[pin]]`), `led_active` (`high`, `low`), `halt = { pattern, period_ms }` (`blink`, `code`, `off`), `power_hold = { pin, level }` |
| `[[boot.action]]` | A boot action program (`boot-program` format 1): `on` (a boot event the event catalog marks action-capable) and `ops`: `{ op = "led", state }` (`on`, `off`, `toggle`), `{ op = "gpio", pin, level }` (a `[[pin]]` with `boot = true`; the power hold pin takes only its hold level), `{ op = "delay", ms }` (1 to 20), `{ op = "bus_write", device, reg, bytes }` (a `[[device]]` register in its `writable` list, 1 to 8 bytes, only on an MCU with `boot_capabilities = ["i2c"]`). At most 32 ops and 50 ms per event, a bus write counting 5 ms |
| `[[pin]]` | Pins extensions and boot programs may name: `pin`, `boot` (a boot program may drive it), `levels` (allowed levels; all when left out) |
| `[[bus]]` | Buses extensions reach through the bus service: `id`, `kind` (`i2c`, `spi`, `uart`, `can`), `controller` (an MCU `[[peripheral]]`) |
| `[[peripheral]]` | Board names for MCU blocks extensions may declare: `id`, `mcu` (the MCU `[[peripheral]]` id) |
| `[[device]]` | A device on a board bus: `id`, `bus`, `addr`, `writable` (registers anything outside DAGON may write) |

## What leaves this machine

Definitions are not a place for secrets. Names in them (ids, product
names, pin labels, partition names) appear in DAGON Console and are sent
to Claude when you ask it; commands, hints, comments and file paths are
never sent. A project's own names reach the page and Claude only when they are
plain (at most 40 of `A-Z a-z 0-9`, space, `.`, `_`, `-`); anything else is
shown as `project-text`.

定義ファイルは秘密を書く場所ではありません。定義の中の名前（id、製品名、ピンのラベル、パーティション名）は DAGON Console に表示され、Claude に頼んだときは Claude にも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。プロジェクト自身の名前は、40 文字以内の `A-Z a-z 0-9`・空白・`.`・`_`・`-` だけのときにそのまま出し、それ以外は `project-text` と表示します。

## Project definitions

A project's `dagon.toml` may define boards inline as `[board.<id>]` tables
(the board schema without `id`) and register MCU files with
`mcu_files = ["defs/my-mcu/dagon-mcu.toml"]`; files a definition references
resolve inside its own folder.

## Validation

Every number a definition holds is checked against what the engine that
reads it needs; the engines declare those requirements in
`dagon-topology/src/defs/requirements.rs` (UF2 `sector`: a power of two of
at least 256; log `face_size`: at least 96 bytes; slots larger than a
header; every region inside every flash map, none overlapping; and so on).
A numeric field without a declared requirement fails the tests.

Also on load: unknown fields; unique MCU ids, wire ids, flash map names and board
ids; a board's MCU exists; every slot, detached header and layout region
lies inside every flash map of the MCU and none overlap; one slot per image
and binding; detached headers inside their partition; referenced files are
relative, stay inside the package and (when loaded from disk) exist; so do
part folders (`sub`), and part `name` / `bin` are plain names. Addresses
that run past 32 bits are errors. Inline `[board.<id>]` boards have no
package, so they may not list parts.
Doctor `fix` and writer commands from a project definition never run
without the user's explicit confirmation.

Importing Arduino, Zephyr or probe-rs data is a future item.
