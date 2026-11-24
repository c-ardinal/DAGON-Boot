---
title: "MCU and board definitions"
description: "dagon-mcu.toml and dagon-board.toml: what DAGON knows about an MCU or a board."
---

Every MCU package (`targets/mcu/<id>/`) carries `dagon-mcu.toml` and every
board package (`targets/boards/<id>/`) carries `dagon-board.toml`. They hold
what DAGON knows about the MCU or board; DAGON's code keeps only the closed
sets of kinds and the engines that use them. `dagon-topology::defs` loads and
validates them into a `Registry`:

- `Registry::builtin()`: every `targets/mcu/*/dagon-mcu.toml` and
  `targets/boards/*/dagon-board.toml` of this tree, found by folder and
  embedded at build time (no filesystem; the browser build uses this).
  Adding a package folder adds its definition; the order of folders carries
  no meaning.
- `Registry::load(&[Source::Tree(root), Source::Project(dagon_toml)])`: a
  DAGON source tree's definitions replace the builtin set; a project adds its
  own. A project definition may not reuse an existing MCU or board id. A
  project may override just the fields it changes of an existing
  definition (Project definitions, below).

Numbers are TOML integers (`0x0800_8000`, `400_000`). Unknown fields are
errors; kind fields accept only the values listed below.

## dagon-mcu.toml

<!-- generated from dagon-topology's McuDef: python site/scripts/gen.py -->
| Key | Type | Required | Meaning |
|---|---|---|---|
| `format` | integer | no | Definition format number (`3`; left out means 1). A dagon that does not read the number refuses the file (site/src/content/docs/explanation/versioning.md). |
| `id` | string | yes | MCU id used in topologies (`stm32f401re`). |
| `name` | string | yes | Display name (`STM32F401RE`). |
| `wire_id` | integer | yes | Image header `mcu_id`; unique. |
| `port` | string | yes | `self-boot` or `vendor-stage2`. |
| `gpio_count` | integer | no | GPIO0..n-1 checked for nodes without a board; 0 when pins are named by port. |
| `[[flash_map]]` | tables | yes | `name` (unique), `base` (address of flash byte 0), `bytes`, `xip` (required; `true`: the CPU reads flash at `base`, so flash shares the address space with RAM and register blocks; `false`: `base` and the slots are flash offsets an MMU maps elsewhere). The first is the default. |
| `partitions` | string | no | ESP-IDF partition table CSV in the package (rows need explicit offset and size). |
| `[layout]` | table | no | Optional fixed regions: `stage0 = { at, size }`, `bootcore_log` / `app_log = { faces = [a, b], face_size }`; a `bootcore_log` equal to `app_log` is one log holding both the boot core and the APP records. |
| `[stage0]` | table | no | The MCU's DAGON stage0, exactly when `[layout]` has `stage0`: `key` (where stage0 reads the key it checks a boot core's signature with: `"flash"`, the default and the only place, a constant in stage0's own flash written when stage0 is built; any other place is refused), `region_bytes` (the whole region, equal to `[layout] stage0` size; stage0's image leaves 1/16 of it free, which its link checks) and `head_bytes` (required: the bytes at the region start the boot ROM reads before stage0, such as the RP2040's boot2; 0 for none; less than `region_bytes`). `[[stage0.clock]]` lists the clock setups stage0 may run on: exactly one `{ source = "internal", out_hz }` (the MCU's own oscillator as it runs from reset) and any number of `{ source = "crystal" \| "external", in_hz, out_hz, pll = { ... } }`, where `pll` holds the MCU's own fields (RP2040: `refdiv`, `fbdiv`, `postdiv1`, `postdiv2`, from a crystal; STM32F401: `m`, `n`, `p`, `q`, `flash_wait_states`, from a crystal or an external clock), which name the PLL whose rules check them; a `pll` whose fields name no PLL DAGON knows is refused. For a node, stage0 runs on the setup of the board's `[clock]` source (same source and `in_hz`) with the highest `out_hz`, else on the internal one; two setups of one source and `in_hz` with the same `out_hz` are refused. |
| `[bootcore]` | table | no | The DAGON boot loader's clock, exactly when the MCU has a `bootcore` slot: `[[bootcore.clock]]` lists the setups the boot loader runs on, as `[[stage0.clock]]` does for stage0 and checked by the same PLL rules, and the boot loader's HAL sets the chosen one up. Its internal setup may run a PLL from the MCU's own oscillator: `{ source = "internal", in_hz, out_hz, pll = { ... } }`, `in_hz` that oscillator's frequency (the STM32F401's HSI); without a `pll` it takes only `out_hz`. For a node, the boot loader runs on the setup the board's `[clock]` picks, as stage0 does, else on the internal one. |
| `[app]` | table | no | The app's clock, exactly when the MCU has a `bootcore` slot: `[[app.clock]]`, as `[[bootcore.clock]]` for the app, which `dagon gen` writes into the node's `wiring.rs` and a board package into its definition module; the MCU package's app start-up sets it up. On the RP2040 a setup with the USB PLL (`usb_refdiv`, `usb_fbdiv`, `usb_postdiv1`, `usb_postdiv2`, giving 48 MHz) gives the app its USB clock; an app without one builds only without a USB uplink. The internal setup may state `tolerance_ppm`, how far the oscillator may run from `out_hz`; a node whose app runs on an internal setup without it runs no UART link (V-26) and drives no I2C bus (V-20) on that clock. |
| `[[slot]]` | tables | yes | `image` (`app`, `bootcore`, `agent`, `stage0`), `binding` (`a`, `b`, `any`), `header` (`inline`, `trailer`, `detached`); the region is `at` + `size` or `partition`; a detached header is at `header_at` or `header_partition` + `header_offset`. |
| `[writers]` | table | yes | What the MCU's firmware can write: each image kind a `[[slot]]` names, mapped to the kinds that write it (`app = ["app"]`, `bootcore = ["app"]`), each of them a kind with a slot here. A kind writes another kind's slots from its own; a kind that lists itself writes its own slot it is not running from, which counts only when it has two. A kind has one slot (`a`) or two (`a` and `b`); one slot loads only when a kind other than itself writes it. |
| `[writer]` | table | yes | First-write tool: `kind` and its parameters (below). |
| `[[part]]` | tables | no | DAGON parts built from this package for a first write: `name`, `sub` (folder in the package), `bin`. |
| `[coordinator]` | table | no | Only for an MCU that can be the coordinator: `pio_state_machines`, `pio_uart_state_machines` (per link), `uart0_tx`, `uart0_rx`. A topology may make the MCU its coordinator only when `[coordinator]` is defined (V-25). |
| `[link_uart]` | table | no | How the UART the MCU package runs DAGON's UART links on makes a bit rate, as its HAL computes it: its clock divided by `clocks_per_bit` times a divider with `frac_bits` fraction bits, rounded down. The clock is `clock_hz` (a frequency no clock setup changes) or `clock = "app"` (the core clock of the node's `[[app.clock]]` setup, which V-26 takes for each node; a node on an internal setup without `tolerance_ppm` runs no UART link), exactly one of them. `bauds` lists the bit rates the firmware runs its link at; `fast_ppm` and `slow_ppm` (required) bound how far its bit rate may run above and below a link's `baud`, its clock's tolerance included (`slow_ppm` at most 1000000). The topology check (V-26) refuses a link `baud` that either end's list does not name or that either end makes outside its bounds, and DAGON Console offers only the speeds both ends name. Required on an MCU whose `[links]` has `uart`: without it, every UART link that MCU's firmware runs is refused, since no speed on it is known to work. |
| `[link_i2c]` | table | no | The I2C bus speeds the MCU package's firmware runs DAGON's I2C link at, as controller or target: `hzs`. The topology check (V-20) refuses a bus `speed_hz` that any node's list does not name, and DAGON Console offers only the speeds every node on the bus names. Required on an MCU whose `[links]` has `i2c`: without it, every I2C bus that MCU's firmware is on is refused, since no speed on it is known to work. `[link_i2c.controller]` states how the firmware makes SCL when the node drives the bus, as its HAL computes it: its clock divided by the bus speed's period rounded to the nearest whole cycle, which the HAL refuses under `min_period` cycles. The clock is `clock_hz` or `clock = "app"`, as `[link_uart]`'s, exactly one of them; `fast_ppm` and `slow_ppm` bound how far SCL may run above and below `speed_hz`, its clock's tolerance included. V-20 refuses a bus whose controller (the end its target does not reach at an address) runs SCL outside those bounds on its node's clock, cannot make the speed, or runs on an internal setup without `tolerance_ppm`, and a bus driven by a node whose MCU states no `[link_i2c.controller]`. |
| `[rom_i2c]` | table | no | ROM I2C bootloader (`stm32-i2c-rom`): `name` (as check messages call it), `addr`, `max_hz`, `erase_sectors` (erase units of `execution.erase_regions`, counted from the first region; the host waits each one's `unit_max_ms` and a margin), `pairs = [{ controller, sda, scl }]` it listens on. |
| `[links]` | table | no | Link kinds the MCU package's firmware implements (its `LINK_KINDS`). |
| `[boot_receive]` | table | no | How the boot stage that writes the app (`[writers] app` names a kind other than `app`) receives when the app does not answer (left out: no link kind takes part in a listen window, and the boot stage receives only in update or recovery mode): `listen_window_ms` (0 to 200, 50 when left out; 0 opens no window), how long after its start it waits for STAY before it boots, and `kinds = { <link kind> = true \| false, ... }`, whether each link kind takes part in the listen window (a kind left out does not). A kind the other end enumerates first (`usb-cdc`) cannot. The boot stage receives on every link of a kind of `[links]`, none above another: on a kind that takes part from its start, on the others in update or recovery mode. A project changes the window with `[mcu_override.<id>.boot_receive]`. |
| `[[link_port]]` | tables | no | The MCU's link interfaces a topology link end names: `index` (0 to 255), `name` (`a-z`, `0-9`, `-`) and `kind` (a link kind: `i2c`, `uart`, `can`, `ethernet`, `usb-cdc`, `ble`, `wifi-tcp`); `index` and `name` each unique. A link end's `port` is one of these names, of the link's kind (topology check V-27), and the compiled topology carries its `index`. |
| `[[relay_link]]` | tables | no | The links a node of this MCU relays over, one per `[[link_port]]` at most: `id` (RELAY's link id, 1 to 254, unique), `port` (a `[[link_port]]` name), `addressing` (`"seven-bit"`: the reached end's address from the topology, on a kind with addresses; `{ fixed = N }`, N 0 to 255: the one peer of a kind without addresses) and `max_payload` (1 up to the protocol maximum of the port's kind). An MCU package's build generates them as its `RELAY_LINKS`, and the PC reads the same entries. The relay keeps, per link, a buffer of one frame of its `max_payload` (`RELAY_LINK_BUFFERS`) and no more; the build stops when that buffer cannot hold the largest frame of the link's kind. Data longer than the buffer (a RELAY's inner frame, the data I2C_RAW writes and the length it reads) is refused with `E_ARG`, never cut. The I2C_RAW limit is the buffer of the link the recovered node is on. |
| `[relay]` | table | no | The budget of the relaying app firmware: `ram_bytes` (static RAM, `.bss` plus `.data`, in bytes; 1 up to the MCU's RAM size). Only an MCU with `[[relay_link]]` gives it. CI builds the MCU package's `examples/relay.rs`, measures it and fails above this value (`scripts/relay_ram.py`). |
| `[codec]` | table | no | The wire codec's budgets: `bootcore` (every boot core of the MCU) and `relay` (the relay firmware), each `{ flash_bytes, stack_bytes, ram_bytes }`: the codec's (`dagon_proto`) functions and named read-only data in flash, the deepest stack from any of its functions, and its static data in RAM, in bytes. `bootcore` only with a `[[slot_layout]]` of target `bootcore`, `relay` only with `[[relay_link]]`; an MCU with such an image gives its budget. CI builds each image with rustc's stack frame records, measures it, and fails above a value, and when the call graph has an indirect call other than a handler row's dispatch, a call it cannot follow or recursion (`scripts/codec_budget.py`). |
| `[connection]` | table | yes | How the user connects it for a write: `label`, `hint`. |
| `[[doctor]]` | tables | no | Tool check: `what`, `probe` (command), `expect` (`"runs"`, `{ line = "..." }`, `{ prefix = "..." }`), `fix` (command), `hint`, `slow` (`true`: `dagon doctor --fix` says the install takes a while before it starts). |
| `[ram]` | table | no | `{ at, size }`: all of the MCU's RAM. Images get the RAM below the boot record, the fuse cells and the extension regions. |
| `[isolation]` | table | no | How DAGON keeps extensions away from itself (below). Left out: `kind = "none"`, `level = "none"`. |
| `[[peripheral]]` | tables | no | Every register block: `id`, `base`, `size`, `window_eligible` (an extension may get it as a window), `bus_master` (it can write memory), `privileged_why` (one or a list of `dma`, `flash`, `watchdog`, `clock`, `reset`, `irq-routing`, `pinmux`, `power`, `rom`, `core-control`, `protection`, `shared`), `dma = { streams, channels }` (a DMA controller, a bus master: its streams, each selecting one of `channels` requests), `dma_requests = [{ signal, controller, stream, channel }]` (the block's rows of the MCU's DMA request mapping table: `signal` `a-z 0-9` such as `tx`, `rx`, `ch1`, `up`; one cell serves one block). Board packages and MCU templates take DMA streams from these (first row of the request on a free stream). |
| `[[core]]` | tables | no | Every processor core, `id`; DAGON runs on the first. |
| `[watchdog]` | table | no | `kind` (`iwdg`, `rp-watchdog`, `rtc-wdt`), `trial_ms` (armed on a trial boot), `confirmed_ms` (armed otherwise), `trial_attempts` (boots the boot stage gives a pending image before it rolls back: the boot core's limit, or the vendor second stage's). An app may delay a reboot by at most half the shorter period. The MCU package's build turns `trial_attempts` into the constant its boot core and its agent use. `flag` (required): the watchdog-reset flag the lockup fuse needs (set by a watchdog reset, kept over every later reset but power-on, cleared when a start acknowledges it), a fact of `none`, `register` (a reset-flag register; name it and its source in a comment) or `package` (the MCU package keeps it): `"register"`, `{ unverified = "package" }`, `"unmeasured"`. Without a flag with a source the MCU has no lockup fuse. `range_ms` (required): `{ min, max }`, the periods the MCU's watchdog can run, ms, with its source in a comment; a definition whose `trial_ms` or `confirmed_ms` lies outside it is refused, when the definitions load and when the firmware layout is generated. |
| `[execution]` | table | no | Hardware facts of the execution model (site/src/content/docs/en/explanation/lifecycle-events.md, "Execution model"): `masked_max_us` (longest DAGON's own code masks interrupts, 1..=1000), `flash_op` (what an erase or program stops: `fetch-stall`, `xip-off-masked`, `cache-off-masked`), `write_unit` (required: the bytes one flash write covers, a power of two dividing every erase unit), `erased_value` (required: the byte erased flash reads as), `erase_regions = [{ start, len, unit, unit_max_ms }]` (the flash DAGON erases in: each region's erase unit and that unit's datasheet maximum erase time; whole units, not overlapping; 2 x `unit_max_ms` at most `watchdog.trial_ms` and `watchdog.confirmed_ms`). A fact is a number checked against its source (cite it in a comment), `{ unverified = N }` or `"unmeasured"` (no value; not for `unit_max_ms`). |
| `[[slot_layout]]` | tables | no | Every XFER target and slot the node receives into: `target` (`app`, `bootcore`, `agent`; the removed `topology` is refused), `slot` (`a`, `b`, `factory`), `start`, `capacity` (largest body, header excluded), `header` (`"inline"` at `start`, or `{ detached = <address> }`), `partition` (optional: the partition the body region is, checked against the table). One per target and slot, with `[execution]`; each `[[slot]]` image has its own, and an erase of the largest body lies in `erase_regions`. The node and the host plan every erase from these. |
| `[conformance]` | table | no | `scratch = { at, size }`: flash a conformance build erases and writes; apart from every slot, header, log face and stage0. |
| `[boot]` | table | no | `[boot.record]`, for MCUs with a DAGON boot stage: where the boot record and the lockup fuse live. `kind` (`noinit-ram` inside `ram`, `retained-registers` inside a `[[peripheral]]`), `at`, `size` (at most 4096), `survives` (`reset-random`, `reset-cleared`, `power`: what a power loss leaves), `entropy = { source, min_bits }` (required; the start-up word a new record sequence mixes in: `source` one of `rng`, `ring-osc`, `osc-capture`, `adc-noise`; `min_bits` its min-entropy per word, a fact, at least 16 when it has a value), `rings = [{ writer, at, size }]` (one per boot stage, in boot order, inside the record), `fuse = { cells = [{ image, at, size }] }` (`bootcore`, `app`; outside the record), `reserved` (addresses the record never stores to). The byte layout inside is the `boot-record` contract's. |
| `boot_capabilities` | array of strings | no | What a board's boot action program may use besides pins: `["i2c"]` allows `bus_write`. |
<!-- end of generated table -->

Slot geometry: `inline` puts the 512-byte header at the slot start and the
payload after it; `trailer` puts the payload at the slot start and the
header in the last 512 bytes; `detached` puts the header elsewhere and the
payload fills the slot. `any` slots answer for slot A.

Slot count: each image kind has the slots its `[[slot]]` entries list,
one (`a`) or two (`a` and `b`); a `b` without an `a` does not load. With
two, a new image goes to the slot the kind is not running from and the
previous version stays in the other. One slot loads only when
`[writers]` lists a kind other than itself that writes it, since an
image cannot rewrite the slot it runs from; otherwise the definition
does not load, and the message names the kind and says no other image
on this MCU can write it. Each writer kind has a slot of its own here.
What one slot cannot do (the previous version, a power cut during a
write, the image that recovers it) is in
[Limitations](/en/explanation/limitations/).

Writer kinds:

- `uf2-drive`: `family`, `sector`, `info_file`, `info_header`, `board_id`
  (the boot ROM drive is the one whose info file starts with `info_header`
  and has the line `Board-ID: <board_id>`).
- `probe-rs`: `chip`.
- `espflash`: `chip`, `flash_size`, `table_at`, `bootloader`, `checksums`,
  `usb_hint = { vid, pid }`. The hint only labels candidates the writer
  tool reports; it never filters them. Matching candidates are listed first and
  pre-selected.

### Erase and trial waits

`[execution] erase_regions` describes the flash DAGON erases in: each
region (`start`, `len`) is erased in whole units of `unit` bytes, one unit
per flash operation, and one unit takes at most `unit_max_ms`. An MCU
whose units differ by address (the STM32F401's 16, 64 and 128 KiB
sectors) lists one region per unit size. Where the flash is a part on
the board (the RP2040 and the ESP32-S3), the MCU's regions are a default
that every candidate part supports: 4 KiB sectors, each taking the
largest maximum sector erase time of those parts. A board that carries a
known part states it in `[flash]` (`part`, its datasheet cited in a
comment, and `erase_regions` in the same shape); its regions replace the
MCU's whole for nodes on that board, and they are checked with the MCU's
watchdog and slot layouts. `[[slot_layout]]` lists every
target and slot the node receives into, with where its body starts, its
largest body and where its header goes (`"inline"` at the start, or
`{ detached = <address> }`, as the ESP32-S3's headers in its dagon
partition).

From a node's effective regions (its board's `[flash]` when present,
else its MCU's) and its MCU's slot layouts, one planner
(`dagon_port::timeout`) gives the erase of a
transfer: the bytes it covers (an inline header and the body from the
slot start, or a detached header and the body apart), and each whole unit
of the region holding them. The node's agent hands exactly that plan to
the slot store, which takes the regions from its board
(`ERASE_REGIONS`, generated from the same definitions into the board
package or the node's wiring.rs and required of every `BoardDef`). The
host and the conformance runner plan the
same transfer from the same definitions and wait for the request the node
erases on the sum of the planned units' `unit_max_ms` plus 1000 ms. A
node's trial wait comes from `[watchdog] trial_ms` and `trial_attempts`:
the host polls the new version's HELLO and sends CONFIRM inside one
trial, and its wait for the new version covers `trial_attempts` trials
(each MCU's values are checked against the host's waits when its
timings are built).

For a node whose MCU has no definition, or a definition without
`[watchdog]`, the host and the conformance runner use the fallback: an
erase wait of 30000 ms, a trial of 8000 ms and 3 attempts. A node whose
definition has no `[execution]` gets the fallback erase wait of
30000 ms.

### `[isolation]`

<!-- generated from dagon-topology's Isolation: python site/scripts/gen.py -->
| Key | Type | Required | Meaning |
|---|---|---|---|
| `level` | string | no | What DAGON enforces: `none`, `mpu`, `pms`, `tz`. A kind gives `none` or one level (`mpu` for the MPU and PMP kinds, `pms` for `esp-pms`, `tz` for `armv8m-tz`). |
| `kind` | string | no | The protection hardware: `armv6m-mpu`, `armv7m-mpu`, `armv8m-mpu`, `armv8m-tz`, `riscv-pmp`, `riscv-epmp`, `esp-pms`, `none`. |
| `boot_extensions` | boolean | no | The MCU has a DAGON boot stage that runs extensions (`port = "self-boot"` only). |
| `ext_ram_max` | integer | no | Most RAM the sandboxed extensions of one image take, regions rounded up (required when `kind` is not `none`). |
| `root_protection` | array of strings | no | Root-image protection `dagon provision` may set: `rdp1` (reversible), `rdp2` (permanent), `wrp` (reversible), `esp-secure-boot`, `esp-flash-encryption` (permanent). |
| `cache` | string | no | `none`, `i-cache`, `id-cache` (instruction and data). |
| `[[isolation.tcm]]` | tables | no | Tightly coupled memories, `[{ at, size }]`. |
| `[isolation.capabilities]` | table | no | The kind's capability record, read back from the hardware at boot (a mismatch turns the level to `none`). `armv6m-mpu`: `mpu_regions` 0 or 8, `min_region` at least 256, `subregions_from`; `armv7m-mpu`: `mpu_regions` 8 or 16, `min_region` at least 32, `subregions_from`; `armv8m-mpu`: `mpu_regions` 0, 4, 8, 12 or 16; `armv8m-tz`: `sau_regions` (0, 4, 8), `s_mpu_regions`, `ns_mpu_regions`, `idau = [{ base, size, attribution }]`; `riscv-pmp`, `riscv-epmp`: `pmp_entries` (0, 16, 64), `grain`, `na4`, `tor`; `esp-pms`: `worlds`, `gdma_channels`, `memory_regions`, `peripheral_slots`. Every kind: `critical_section` (`primask` on armv6m, `basepri` on armv7m and armv8m, `mstatus-mie` on RISC-V, `xtensa-level` on esp-pms), `unwind` (`cortex-m-exception`, `armv8m-tz`, `riscv-trap`, `xtensa-world`, by kind), `timer` (`systick`, `riscv-mtime`, `timer-group`), `reset_cleanup` (`peripheral-reset`, `reinit`), `background_privileged`, `fault_address`; optionally `np_max_us` and `dma_stop_max_us`, which may only lower DAGON's bounds (50 and 20). |
<!-- end of generated table -->

A definition whose `kind` is not `none` lists every core and at least one
block of each class `flash`, `watchdog`, `clock`, `reset` and
`irq-routing` (also `core-control` with more than one core, and
`protection` for `esp-pms` and `riscv-epmp`); a bus master is never
window-eligible, and every block that is not window-eligible says why. A
definition that leaves a class out is refused with the class named.

## dagon-board.toml

<!-- generated from dagon-topology's BoardDef: python site/scripts/gen.py -->
| Key | Type | Required | Meaning |
|---|---|---|---|
| `format` | integer | no | Definition format number (`3`; left out means 1). A dagon that does not read the number refuses the file (site/src/content/docs/explanation/versioning.md). |
| `id` | string | yes | Board id used in `nodes[].board`. |
| `product` | string | yes | Product name. |
| `mcu` | string | yes | MCU id (must be defined). |
| `flash_bytes` | integer | yes | Flash size (must hold one of the MCU's flash maps). |
| `i2c_target` | boolean | no | Whether the board can be an I2C target. |
| `drawing` | string | yes | UI drawing. |
| `default` | boolean | no | `true`: the board a node naming only its MCU is checked against (a project cannot replace the default of a builtin MCU that already has one: two marked boards do not load). At most one board of an MCU says so; an MCU's only board is its default without it; an MCU with several boards and none marked has no default (V-06 rejects a node naming only that MCU). Definition order never decides it. |
| `[[header]]` | tables | no | `[{ pin, label }]`; `label` left out when not confirmed on the board. Every pin a topology may name on this board, as a link end on it (a hub's `pins`, `control`, bus `sda`/`scl`; a node's `node_pins` come from `i2c`): the check (V-07) refuses any other, and DAGON Console offers only these. |
| `[[i2c]]` | tables | no | `[{ controller, sda, scl }]` pairs usable from the header. |
| `[default_link]` | table | no | How a node board is wired unless the topology says otherwise: `kind` (`i2c`, `uart`, `can`, `ethernet`), `iface` (`hw-uart0`, `pio-uart`), `addr`, `baud`, `control` (`full`, `cooperative`), `rom_entry` (`force-download-boot`, `m5pm1`, `stm32-i2c-rom`). Left out for the coordinator. |
| `[[part]]` | tables | no | Parts built from the board package itself (`name`, `sub`, `bin`). |
| `[boot]` | table | no | The boot stage's board values: `led` (a `[[pin]]`), `led_active` (`high`, `low`), `halt = { pattern, period_ms }` (`blink`, `code`, `off`; with `led` on an MCU with a DAGON stage0, a `blink` period is at least 10 ms, since stage0 blinks its code in tenths of it), `power_hold = { pin, level }`. Its boot action programs, `[[boot.action]]`: A boot action program (`boot-program` format 1): `on` (a boot event the event catalog marks action-capable) and `ops`: `{ op = "led", state }` (`on`, `off`, `toggle`), `{ op = "gpio", pin, level }` (a `[[pin]]` with `boot = true`; the power hold pin takes only its hold level), `{ op = "delay", ms }` (1 to 20), `{ op = "bus_write", device, reg, bytes }` (a `[[device]]` register in its `writable` list, 1 to 8 bytes, only on an MCU with `boot_capabilities = ["i2c"]`). At most 32 ops and 50 ms per event, a bus write counting 5 ms. |
| `[[pin]]` | tables | no | The board's pins: `pin`, `name` (what the board package's code takes it by, `a-z 0-9 _`), `boot` (a boot program may drive it), `levels` (allowed levels; all when left out). |
| `[[bus]]` | tables | no | The board's buses: `id` (`a-z 0-9 - _`), `kind` (`i2c`, `spi`, `uart`, `can`), `controller` (an MCU `[[peripheral]]`), its pins by signal (`sda`, `scl` for i2c; `tx` and/or `rx` for uart; `tx`, `rx` for can; `sck` and `mosi` and/or `miso` for spi), `hz` (i2c, spi) or `baud` (uart, can; on a `dagon = true` UART bus the link speed of a build dagon does not give the topology's, see below), `dagon` (`true`: DAGON's own link runs on it, so neither the bus nor its controller is ever an extension's), `dma` (`true`: the board package opens or hands out the bus with DMA; i2c, spi, uart; its tx and rx streams come from the MCU's `dma_requests`, buses in order, and the board package's build writes them as `<Bus>TxDma`/`<Bus>RxDma`, `take_dma!` and `bind_<bus>_dma!`). |
| `[[peripheral]]` | tables | no | Board names for MCU blocks extensions may declare: `id`, `mcu` (the MCU `[[peripheral]]` id; never the controller of a `dagon = true` bus). |
| `[[device]]` | tables | no | A device on a board bus: `id` (`a-z 0-9 - _`), `bus`, `addr`, `max_transfer` (required: the most data bytes it takes in one transfer, from its datasheet; the bus port and boot programs keep to it), `regs = [{ name, reg }]` (its registers by name, `a-z 0-9 _`), `writable` (registers anything outside DAGON may write: numbers or names of `regs`). |
| `[flash]` | table | no | The flash part the board carries, for an MCU whose flash is a board part: `part` (cite its datasheet in a comment) and `erase_regions` in the shape of the MCU's `[execution]` `erase_regions` (`start`, `len`, `unit`, `unit_max_ms`). They replace the MCU's regions whole for nodes on this board; checked with the MCU's watchdog and slot layouts. Left out: the MCU's regions. The node and the host plan every erase from the same regions. |
| `[clock]` | table | no | What drives the MCU's clock input on this board: `source` is `"internal"`, `{ crystal_hz = N }` or `{ external_hz = N }` (cite the schematic or datasheet that gives N in a comment). Left out: internal. Stage0, the DAGON boot loader and the app each run on their MCU's setup for this source (`[[stage0.clock]]`, `[[bootcore.clock]]`, `[[app.clock]]`), or on the internal one when none matches; an MCU without those stages does not use it. |
<!-- end of generated table -->

A board package runs DAGON's UART link at the topology link's `baud`:
`dagon build`, `deploy` and `flash` give every build of a node (its app and
its recovery agent) the speed of that node's UART link as `DAGON_LINK_BAUD`,
so both ends are built with the same value. A build without it (plain
`cargo build`) takes `[default_link] baud`, or the `baud` of the board's
`dagon = true` UART bus.

A node's I2C target address is the `addr` of its end in the topology:
`dagon gen` writes it as `LINK_ADDR` into the app's `wiring.rs` and the
boot loader's `boot_wiring.rs`, so both answer at the address the hub
reaches. Where the topology gives none, both take `[default_link] addr`.

No pin appears twice across `[[pin]]` and the buses, and every name code is
generated from (pin names, bus ids and their signals, device ids, with `-`
read as `_`) appears once. When the board is the coordinator, the page wires
a new I2C bus to its `dagon = true` I2C bus and a new PIO UART link to its
`dagon = true` UART bus on a `pio` controller (numbered `GPIO<n>` pins, on
an MCU with `[coordinator]`). The board states those pins only once, in
the buses.

A board package's build script reads the board's definition
(`dagon_topology::defs::codegen::package_build`) and includes the result.
With `DAGON_BOOT_CRATE` set it reads `board.toml` in that folder (the
board definition with the project's overrides applied, which `dagon gen`
writes; below), else the source tree's board definition. It includes
the board id, flash size, default link, boot LED and halt pattern, bus
speeds and devices as constants (`def.rs`), and every named pin, bus and
bus pin as a HAL type with a `take!(p.<name>)` macro (`pins.rs`), and each
device as a `dagon_events::Device` bus port capability (`devices.rs`, the
only way one is made). The build reruns when any definition the validation
read changes. The MCU packages' boot images read the board's `[boot]` the
same way (`boot_build`), for the board of the node they are built for:
`DAGON_BOOT_CRATE` names the node's boot crate (`boot/<id>/` of the project,
written by `dagon gen`), whose `Cargo.toml` names the node, its MCU, its
board (a board of this tree or an inline `[board.<id>]` of the project)
and its trusted boot extensions. A build without it fails; a node
without a board gets no boot LED and no halt pattern. The code holds no
pin, address or pattern of its own.

An MCU package's build script generates its layout from the MCU
definition the node build is given (`dagon_mcu_build::definition`):
every slot's address and length per image kind, the stage0 region, the
logs, the erase regions and slot layouts, and the watchdog and trial
values. That definition is `boot/<id>/mcu.toml`, which `dagon gen` writes
for every node: the built-in definition with the project's overrides
applied. For a node with a board it also writes `board.toml`, the board
definition with the project's overrides applied the same way. `dagon gen`
then writes one line per written file to `defs.sha256`: the file's
SHA-256 (64 lowercase hex digits), two spaces and the file name, in UTF-8
with LF line endings. Every build that reads a node's definitions (the
MCU and board build scripts, `dagon build`, `deploy` and `flash`) refuses
a file that no longer matches its line and a folder without
`defs.sha256`. To change a node's definitions, change the overrides in
`dagon.toml` and run `dagon gen` again. The app's `.cargo/config.toml` and the boot crate's both set
`DAGON_BOOT_CRATE` to that folder, and `dagon build`, `deploy` and
`flash` give it to every image they build for the node, a board's own
parts (such as a recovery agent) included. So every image of one node
builds from one file, and a project that moves its slots, gives them
lengths of their own or sets other watchdog periods gets matching
firmware without a code edit. An `mcu.toml` of another MCU or
another definition format is refused; a build without
`DAGON_BOOT_CRATE` (a plain `cargo build` of the package) reads the
package's own `dagon-mcu.toml`.

## What leaves this machine

Definitions are not a place for secrets. Names in them (ids, product
names, pin labels, partition names) appear in DAGON Console and are sent
to Claude when you ask it; commands, hints, comments and file paths are
never sent. A project's own names reach the page and Claude only when they are
plain (at most 40 of `A-Z a-z 0-9`, space, `.`, `_`, `-`); anything else is
shown as `project-text`.

## Project definitions

A project's `dagon.toml` starts with `format = 1`, the number of the
`dagon.toml` format (`project` in
[Contracts and components](/en/reference/contracts/)). A file without
`format`, or with another number, is refused with the number this dagon
reads. These are the only top-level keys of `dagon.toml`; any other key
is refused:

<!-- generated from dagon-topology's project KEYS: python site/scripts/gen.py -->
`format`, `topology`, `release`, `provision_dir`, `pubkey`, `signing_key`, `provision`, `build`, `dagon`, `extensions`, `board`, `mcu_files`, `mcu_override`, `board_override`
<!-- end of generated table -->

`dagon.toml` may define boards inline as `[board.<id>]` tables
(the board schema without `id`) and register MCU files with
`mcu_files = ["defs/my-mcu/dagon-mcu.toml"]`; files a definition references
resolve inside its own folder.

The `<id>` of `[board.<id>]` is a board id, the id a node's `board:` names;
a `[board.<id>]` table of `dagon.toml` is always a board definition. A node
on such a board has no board package: `dagon gen` makes it from its MCU
package and writes the board's `flash_bytes` into its `wiring.rs`. A node
that names only an MCU needs no board: nothing asks for a board or a flash
size, and DAGON Console shows, read-only, the flash size the MCU definition's
default flash map lays out.

### Overriding a definition

To change part of an existing definition (a built-in one, or one of a
DAGON source tree), write only the fields that change in `dagon.toml`
instead of copying the definition. `[mcu_override.<id>]` overrides an MCU
definition and `[board_override.<id>]` a board definition; each is written
in the schema of that definition and holds only the fields that change.
An override is per id for the whole project: every node on that MCU or
board gets the merged definition. Fields not written stay as the original
definition has them, so they change when a new DAGON release changes the
original. Nodes that need a different variant use a definition
registered under a new id (`mcu_files`, `[board.<id>]`). A project's own
definition is not overridden but changed in place. An override of an id
that does not exist is refused.

Example: one boot loader slot on the RP2040, and the 128 KiB it frees
added to app slot A. Add to `dagon.toml`:

```toml
[[mcu_override.rp2040.slot]]
image = "bootcore"
binding = "b"
dagon-remove = true

[[mcu_override.rp2040.slot]]
image = "app"
binding = "a"
at = 0x1002_8000
size = 0x12_0000

[[mcu_override.rp2040.slot_layout]]
target = "bootcore"
slot = "b"
dagon-remove = true

[[mcu_override.rp2040.slot_layout]]
target = "app"
slot = "a"
start = 0x1002_8000
capacity = 0x11_FE00
```

Slots are found by their key (`image` and `binding`), slot layouts by
theirs (`target` and `slot`). App slot A changes only in the fields
written; the others, such as `header`, stay as the original definition
has them. One boot loader slot loads because `[writers]` has the app
write the boot loader. What one slot cannot do is in
[Limitations](/en/explanation/limitations/).

Merge rules:

- Tables merge key by key, nested ones included; a key written replaces
  the original value.
- A table whose shape follows its kind (`writer` by `kind`, a boot
  operation by `op`, `expect`, a slot layout's `header`) and a fact value
  are replaced whole when written.
- Arrays of plain values (strings, integers) are replaced whole when
  written.
- Arrays of tables follow the table below. In an array merged by key, an
  override entry changes only the fields it writes of the original entry
  with the same key; an entry with a new key is added after the original
  entries. In an array whose entries are added or removed whole, an
  existing entry cannot be changed in part: remove it and write the whole
  new entry (both in the same override).

An array whose order carries meaning (`flash_map`, whose first entry is
the default; `core`, whose first entry DAGON runs on;
`boot.record.rings`, in boot order) takes its order from
`dagon-order.<array> = [<key>, ...]` in the override's table, listing
every key of the merged array exactly once; a list that misses or repeats
a key, or names a key the array does not have, is refused. A nested
array is named by its path (`dagon-order.boot.record.rings`). For example,
a project whose RP2040 boards carry 16 MiB of flash adds a flash map and
makes it the default:

```toml
[mcu_override.rp2040]
dagon-order.flash_map = ["rp2040-16m", "rp2040-4m"]

[[mcu_override.rp2040.flash_map]]
name = "rp2040-16m"
base = 0x1000_0000
bytes = 0x100_0000
xip = true
```

Removing: an entry of an array merged by key, or of one whose entries are
added or removed whole, is removed by writing its key fields and
`dagon-remove = true` and nothing else; an optional table is removed by
writing `dagon-remove = true` alone. A whole definition cannot be
removed. An override that removes an entry or table the original
definition does not have is refused with the override named.

Unsetting: an entry or table may carry `dagon-unset = ["<field>", ...]`,
which removes those optional fields before the override's own fields
apply, to swap one optional field for another (a slot from `partition` to
`at` and `size`, a bus from `hz` to `baud`). Key fields, fields the
original definition does not have, and fields that may not be overridden
cannot be unset.

```toml
[[mcu_override.esp32s3.slot]]
image = "app"
binding = "any"
dagon-unset = ["partition"]
at = 0x11_0000
size = 0x30_0000
```

Removing a required table or unsetting a required field is refused by
the merged definition's normal loading and checks, the error naming the
override. `dagon-remove`, `dagon-unset` and `dagon-order` are read by the
merge only and never reach the merged definition.

Fields that may not change: `id`, `wire_id`, a board's `mcu`, `format`,
`port`, `partitions`, the write tool's files (`writer.bootloader`,
`writer.checksums`) and part folders (`part.sub`) are the definition's
identity or name files of its folder, so an override that changes one is
refused with the field named. A part an override adds may name only a
`sub` a part of the original definition names. `[[doctor]]` may not be
overridden. For something else, register a definition under a new id.

A board's `[flash]`: overrides are first merged into each definition on
its own (`[mcu_override]` into the MCU, `[board_override]` into the
board). Then, when the merged board has `[flash]` `erase_regions`, they
replace the merged MCU's erase regions whole for nodes on that board.
`[flash]` merges as a table, its `erase_regions` replaced whole when
written. An override that changes `flash.part` writes the whole `[flash]`
table, since erase times belong to the part and a new part must not
inherit the old part's; otherwise it is refused with the override named.

The merged definition is made from the override in `dagon.toml` before
the definition's format check and its loading, and then goes through the
same loading and checks as any definition; an error names `dagon.toml`
and the override's table (`[mcu_override.<id>]`). The merged definition
counts as project text (how its names are shown, and the confirmation
before its commands run: What leaves this machine, above, and
Validation, below). The original definition's folder stays its folder,
so firmware and parts are found where they were. The CLI, DAGON Console,
the host, signing and the firmware builds (`boot/<id>/`, above) all use
the merged definition.

The rules of the arrays and of the tables whose shape follows their kind,
and the fields that may not change:

<!-- generated from dagon-topology's merge ROWS: python site/scripts/gen.py -->
| Definition | Path | Rule |
|---|---|---|
| MCU | `flash_map` | merged entry by entry by key (`name`); `dagon-order` gives its order |
| MCU | `slot` | merged entry by entry by key (`image`, `binding`) |
| MCU | `part` | whole entries added or removed by key (`name`) |
| MCU | `doctor` | may not be overridden |
| MCU | `peripheral` | merged entry by entry by key (`id`) |
| MCU | `peripheral.dma_requests` | replaced whole when given |
| MCU | `peripheral.privileged_why` | replaced whole when given |
| MCU | `core` | whole entries added or removed by key (`id`); `dagon-order` gives its order |
| MCU | `rom_i2c.pairs` | merged entry by entry by key (`sda`, `scl`) |
| MCU | `link_port` | merged entry by entry by key (`name`) |
| MCU | `relay_link` | merged entry by entry by key (`port`) |
| MCU | `relay_link.addressing` | replaced whole when given |
| MCU | `boot.record.rings` | merged entry by entry by key (`writer`); `dagon-order` gives its order |
| MCU | `boot.record.fuse.cells` | merged entry by entry by key (`image`) |
| MCU | `boot.record.entropy.min_bits` | replaced whole when given |
| MCU | `isolation.capabilities.idau` | replaced whole when given |
| MCU | `isolation.tcm` | replaced whole when given |
| MCU | `slot_layout` | merged entry by entry by key (`target`, `slot`) |
| MCU | `slot_layout.header` | replaced whole when given |
| MCU | `execution.erase_regions` | merged entry by entry by key (`start`) |
| MCU | `execution.erase_regions.unit_max_ms` | replaced whole when given |
| MCU | `execution.masked_max_us` | replaced whole when given |
| MCU | `watchdog.flag` | replaced whole when given |
| MCU | `writer` | replaced whole when given |
| MCU | `stage0.clock` | replaced whole when given |
| MCU | `bootcore.clock` | replaced whole when given |
| MCU | `app.clock` | replaced whole when given |
| MCU | `id` | may not change (the definition's identity or a file of its package) |
| MCU | `wire_id` | may not change (the definition's identity or a file of its package) |
| MCU | `format` | may not change (the definition's identity or a file of its package) |
| MCU | `port` | may not change (the definition's identity or a file of its package) |
| MCU | `partitions` | may not change (the definition's identity or a file of its package) |
| MCU | `writer.bootloader` | may not change (the definition's identity or a file of its package) |
| MCU | `writer.checksums` | may not change (the definition's identity or a file of its package) |
| MCU | `part.sub` | may not change (the definition's identity or a file of its package) |
| board | `header` | merged entry by entry by key (`pin`) |
| board | `i2c` | merged entry by entry by key (`sda`, `scl`) |
| board | `part` | whole entries added or removed by key (`name`) |
| board | `pin` | merged entry by entry by key (`pin`) |
| board | `bus` | merged entry by entry by key (`id`) |
| board | `peripheral` | merged entry by entry by key (`id`) |
| board | `device` | merged entry by entry by key (`id`) |
| board | `device.regs` | merged entry by entry by key (`name`) |
| board | `device.writable` | replaced whole when given |
| board | `boot.action` | replaced whole when given |
| board | `boot.action.ops` | replaced whole when given |
| board | `flash` | merged key by key; a new `part` comes with the whole table |
| board | `flash.erase_regions` | replaced whole when given |
| board | `clock.source` | replaced whole when given |
| board | `id` | may not change (the definition's identity or a file of its package) |
| board | `mcu` | may not change (the definition's identity or a file of its package) |
| board | `format` | may not change (the definition's identity or a file of its package) |
| board | `part.sub` | may not change (the definition's identity or a file of its package) |
<!-- end of generated table -->

## Validation

Every number a definition holds is checked against what the engine that
reads it needs; the engines declare those requirements in
`dagon-topology/src/defs/requirements.rs` (UF2 `sector`: a power of two of
at least 256; log `face_size`: at least 96 bytes; slots larger than a
header; every region inside every flash map, none overlapping; and so on).
A numeric field without a declared requirement fails the tests.

Also on load: unknown fields; unique MCU ids, wire ids, flash map names and board
ids; unique keys in each array the override table gives a key; a board's MCU exists; every slot, detached header and layout region
lies inside every flash map of the MCU and none overlap; one slot per image
and binding; detached headers inside their partition; referenced files are
relative, stay inside the package and (when loaded from disk) exist; so do
part folders (`sub`), and part `name` / `bin` are plain names. Addresses
that run past 32 bits are errors. Inline `[board.<id>]` boards have no
package, so they may not list parts.
Doctor `fix` and writer commands from a project definition never run
without the user's explicit confirmation.
