# Extension manifest and generated protection files

An extension is a Rust crate with a `dagon-extension.toml` next to its
`Cargo.toml`. The manifest says what the extension replaces, observes and
answers, and what it needs. `dagon gen` checks it against the port catalog
([ports.md](ports.md)), the event catalog ([events.md](events.md)) and the
node's MCU and board definitions ([targets/README.md](../targets/README.md)),
then writes the node's protection files.

## dagon-extension.toml (`extension-manifest` format 1)

```toml
format = 1
id = "uds-transport"                 # a-z 0-9 -, 1 to 32
layers = ["boot", "app"]             # boot, app, relay, host
provides = ["UpdateTransport", "UpdateProtocol"]
observes = ["boot.*", "app.update_*", "app.transfer_*"]
decides = ["accept_update"]
trusted = false
[capabilities]
peripherals = ["can0"]               # the board's [[peripheral]] ids
pins = []                            # the board's [[pin]] names
buses = []                           # the board's [[bus]] ids, never a dagon = true one
timer = true
[limits]
max_run_us = 2000                    # per handler call
stack_bytes = 1024
ram_bytes = 4096                     # .ext.data + .ext.bss
text_bytes = 16384                   # .ext.text + .ext.rodata + the data load image
```

| Key | Rule |
|---|---|
| `format` | Read first; a number this dagon does not read is refused with both numbers |
| `provides` | Ports that exist in every listed layer; a trusted-only port needs `trusted = true` |
| `observes` | `layer.kind`, `layer.prefix*` (matching at least one kind of the catalog) or `layer.*` |
| `decides` | Decision points asked in one of the listed layers; a trusted extension answers none |
| `trusted` | The extension is part of DAGON for the project: it runs privileged, in DAGON's own memory, and nothing of the sandbox holds against it. A trusted-only port needs it; an extension that only observes may say it too (code of the project's own that runs in the boot core) |
| `limits` | Device layers: `max_run_us` 1 to the decision bound (50 ms) for an untrusted extension and for every one that observes events (each handler call is timed against it); untrusted only: `stack_bytes` at least 256, `text_bytes` above 0 |

The crate exports its extension as `pub type DagonExtension` and
`pub const DAGON_EXTENSION: DagonExtension`; the generated chain names them.

## dagon.toml

```toml
[extensions]
packages = ["extensions/*"]          # folders, or folder/* for each subfolder
[extensions.nodes.<id>]
boot = ["uds-transport"]             # the boot core's chain, in order
app = ["uds-transport"]              # the app's chain (a hub's relay too)
trusted = [{ id = "hsm-verify", sha256 = "<source SHA-256, 64 hex digits>" }]
```

`sha256` is the source hash of the extension's package folder: SHA-256
over every file in it, sorted by path (`/`-separated), each as its path, a
zero byte, its length as a little-endian u64 and its bytes with CRLF read
as LF; `target/`, `Cargo.lock` and hidden entries are left out. `dagon
gen` checks it and names the hash it found when the two differ; the boot
core's build checks it again, so only the source dagon.toml declares is
linked. The crates the extension itself depends on are pinned by the
boot crate's `Cargo.lock`, not by this hash.

For each node `dagon gen` refuses, naming the extension and the reason: an
extension listed for an image whose layers it does not have, a port
provided twice in one chain, a trusted extension without its hash or
whose source no longer hashes to it, a boot extension that is not
trusted (the boot core runs trusted extensions only), boot extensions on
an MCU without a DAGON boot stage (the ESP32-S3 starts its app from the
vendor's second stage, so no boot extension runs there),
capabilities the board does not declare, a peripheral that is not
window-eligible, stack and RAM over the MCU's `ext_ram_max`, boot calls
over a quarter of the trial watchdog period, more windows than the
protection unit has regions after code, data and stack, regions that do
not fit the image, a window that would also open a block the
extension did not declare, and an MCU whose data cache (`cache =
"id-cache"`) or tightly coupled memory (`tcm`) the kind's generator does
not place bounce buffers or regions for. Windows are device memory (never
cached); code, data and stacks are normal memory.

## Generated files (`nodes/<id>/dagon/`)

| File | Content |
|---|---|
| `boot_wiring.rs` | MCUs with a DAGON boot stage: the board's LED, halt pattern, power hold and boot action program; the watchdog periods; the boot record and fuse placement; the isolation level, kind, bounds and the registers read back at boot; the extensions in chain order (`BootChain`, `BOOT_CHAIN`) and each boot core slot's protection map |
| `memory-<image>-<slot>.x` | MCUs that define their RAM: the image's FLASH and RAM after the extension regions, each extension's regions and sections, the boot record and fuse cells |
| `protection.toml` | Every image's extensions, memory and protection map, readable |

`nodes/<id>/dagon/` holds only what the last `dagon gen` wrote: every
other file in it that carries gen's mark (its first line says it was
generated by dagon gen) is
removed (others are kept and reported), and a node gen refused or could not
generate keeps none of gen's files. When `nodes`, `nodes/<id>` or
`nodes/<id>/dagon` is a symbolic link or a junction, gen writes and removes
nothing there and reports it.

At level `none` no map is made and `dagon gen` notes that the node's
extensions run unguarded.

## Boot extensions

On an MCU with a DAGON boot stage (RP2040, STM32F401RE), `dagon gen` also
writes the node's boot crate into `boot/<id>/` of the project, from the
boot core template its MCU package ships (`bootcore/template/`, found by
`[package.metadata.dagon] bootcore = "<mcu id>"`): `Cargo.toml` (the boot
core library, `dagon-events` and each boot extension's package, and the
node, its MCU, its board and its trusted extensions under
`[package.metadata.dagon.boot]`), `boot.rs` (the boot core's `main`, from
`nodes/<id>/dagon/boot_wiring.rs`) and `.cargo/config.toml` (the target,
`DAGON_BOOT_CRATE` and the node's `DAGON_TARGET_TAG`). It is a folder of
its own because cargo merges the `.cargo/config.toml` of every folder
above a crate, and the node's app crate has one made for the app.
`boot/<id>/` holds only what the last `dagon gen` wrote, as
`nodes/<id>/dagon/` does; cargo's `target/` and `Cargo.lock` in it are
left alone. Build it in that folder with `cargo build --release`
(`DAGON_PUBKEY` as for every image).

The boot core runs its extensions privileged, so it runs only trusted
ones (manifest `trusted = true`, listed with their source hash under
`[extensions.nodes.<id>] trusted`). From `boot.watchdog_armed` to
`boot.before_jump` or `boot.halt`, each boot event (docs/events.md, layer
`boot`) reaches, in chain order, the extensions whose `observes` names
it, each call timed against its `max_run_us`: a call that takes longer
is not stopped, but the boot core then delivers `boot.extension_overran`
(the extension, its budget, the time taken) to the extensions observing
that kind, and goes on. `dagon gen` refuses a set whose calls could take
more than a quarter of the trial watchdog period: one call per observed
boot event, and for an extension observing `boot.extension_overran` one
more per call of any boot extension. The boot core's build checks the
whole resolved dependency graph of the boot crate: every crate is
DAGON's, inside a declared trusted extension's folder, or a registry
crate with a checksum in the boot crate's `Cargo.lock`. There is no decision point at boot: the slot
the boot core starts is decided by DAGON alone, so an extension cannot
change it. The calls share the boot core's flash budget (the
STM32F401RE's is 32,256 bytes) and its watchdog: one that does not
return makes the watchdog reset the chip.

A minimal trusted boot extension to copy is
`targets/boot-extension-template/` (it counts the boot events and keeps
the slot the boot core chose):

```toml
# dagon-extension.toml
format = 1
id = "boot-count"
layers = ["boot"]
observes = ["boot.*"]
trusted = true
[limits]
max_run_us = 1000
```

```rust
#![no_std]
use dagon_boot::sequence;
use dagon_events::{Ctx, Event, Extension};

pub type DagonExtension = BootCount;
pub const DAGON_EXTENSION: DagonExtension = BootCount { events: 0, chosen: None };

pub struct BootCount { events: u32, chosen: Option<(u8, bool)> }

impl Extension for BootCount {
    fn id(&self) -> &'static str { "boot-count" }
    fn on_event(&mut self, e: &Event<'_>, _ctx: &Ctx<'_>) {
        self.events += 1;
        if let Some(c) = e.get::<sequence::SlotChosen>() {
            self.chosen = Some((c.slot, c.trial));
        }
    }
}
```

```toml
# dagon.toml
[extensions]
packages = ["extensions/*"]
[extensions.nodes.hub]
boot = ["boot-count"]
trusted = [{ id = "boot-count", sha256 = "<the hash dagon gen names>" }]
```

An extension that shows the boot on a display reaches the display through
the MCU's HAL in `on_event` (it runs privileged) and keeps each call
within its `max_run_us`. The ESP32-S3 has no DAGON boot stage (its
vendor's second stage starts the app), so boot extensions do not exist
there and `dagon gen` refuses them with that reason.
