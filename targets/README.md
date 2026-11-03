# MCU and board definitions

Every MCU package (`targets/mcu/<id>/`) carries `dagon-mcu.toml` and every
board package (`targets/boards/<id>/`) carries `dagon-board.toml`. They hold
what DAGON knows about the MCU or board; DAGON's code keeps only the closed
sets of kinds and the engines that use them. `dagon-topology::defs` loads and
validates them into a `Registry`:

- `Registry::builtin()`: the six files of this tree, embedded at build time
  (no filesystem; the browser build uses this).
- `Registry::load(&[Source::Tree(root), Source::Project(dagon_toml)])`: a
  DAGON source tree's definitions replace the builtin set; a project adds its
  own. A project definition may not reuse an existing MCU or board id.

Numbers are TOML integers (`0x0800_8000`, `400_000`). Unknown fields are
errors; kind fields accept only the values listed below.

## dagon-mcu.toml

| Key | Meaning |
|---|---|
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
| `[rom_i2c]` | ROM I2C bootloader (`stm32-i2c-rom`): `addr`, `max_hz`, `erase_sectors`, `pairs = [{ controller, sda, scl }]` it listens on |
| `[connection]` | How the user connects it for a write: `label`, `hint` |
| `[[doctor]]` | Tool check: `what`, `probe` (command), `expect` (`"runs"`, `{ line = "..." }`, `{ prefix = "..." }`), `fix` (command), `hint` |

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
  tool reports; it never filters them.

## dagon-board.toml

| Key | Meaning |
|---|---|
| `id` | Board id used in `nodes[].board`; the first board of an MCU is its default |
| `product`, `mcu`, `flash_bytes`, `drawing` | Product name, MCU id (must be defined), flash size (must hold one of the MCU's flash maps), UI drawing |
| `i2c_target` | Whether the board can be an I2C target |
| `header` | `[{ pin, label }]`; `label` left out until confirmed on the board |
| `i2c` | `[{ controller, sda, scl }]` pairs usable from the header |
| `[default_link]` | How a node board is wired unless the topology says otherwise: `kind` (`i2c`, `uart`, `can`, `ethernet`), `iface` (`hw-uart0`, `pio-uart`), `addr`, `baud`, `control` (`full`, `cooperative`), `rom_entry` (`force-download-boot`, `m5pm1`, `stm32-i2c-rom`). Left out for the coordinator |
| `[[part]]` | Parts built from the board package itself (`name`, `sub`, `bin`) |

## Project definitions

A project's `dagon.toml` may define boards inline as `[board.<id>]` tables
(the board schema without `id`) and register MCU files with
`mcu_files = ["defs/my-mcu/dagon-mcu.toml"]`; files a definition references
resolve inside its own folder.

## Validation

On load: unknown fields; unique MCU ids, wire ids, flash map names and board
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
