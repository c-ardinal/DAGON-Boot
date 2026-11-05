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
as LF. Nothing in the folder is left out (the compiler can read any of
it through `include!` or `#[path]`), so running cargo inside the
extension's own folder, which writes `target/` and `Cargo.lock` there,
changes the hash. `dagon gen` checks it and names the hash it found when
the two differ. The registry crates the extension itself depends on are
pinned by the boot crate's `Cargo.lock`, not by this hash.

`packages` are folders inside the project; `dagon gen` refuses one
outside it. To use an extension from DAGON's source tree (the template
below, or a sample under `samples/`), copy its folder into the project
and point its `path = "../../..."` dependencies at the DAGON checkout the
project's `[dagon] path` names. The copy's hash depends on those paths, so
each project has its own: the first `dagon gen` refuses and names it.

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
`nodes/<id>/dagon/boot_wiring.rs`), `graph.stamp` (below) and `.cargo/config.toml` (the target,
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
more per call of any boot extension.

`dagon gen` also checks the boot crate's resolved dependency graph with
cargo (every feature, every target): every crate built into it must be
DAGON's, inside a declared trusted extension's folder, or a registry
crate with a checksum in the boot crate's `Cargo.lock`. It then writes
`boot/<id>/graph.stamp` for the boot crate's `Cargo.toml`, `Cargo.lock`
and trusted sources as they are; the boot core's build refuses to build
without that stamp or when any of them changed since (run `dagon gen`
again). There is no decision point at boot: the slot
the boot core starts is decided by DAGON alone, so an extension cannot
change it. The calls share the boot core's flash budget (the
STM32F401RE's is 32,256 bytes) and its watchdog: one that does not
return makes the watchdog reset the chip.

Once `boot/<id>/Cargo.lock` exists, `dagon gen` resolves the graph with
`--locked`, so after an extension is added it stops at the lock. Run
`cargo metadata --offline --format-version 1` in `boot/<id>/` once (it
adds the new package to the lock), then `dagon gen` again. To build the
boot core without an extension, remove it from the node's `boot` and
`trusted` lists and run `dagon gen` again.

The boot events come as fast as the boot core works:
`boot.watchdog_armed`, `boot.log_read` and `boot.before_decision` follow
each other with only the state log's flash reads between them,
`boot.slot_chosen` or `boot.no_image` follows the image check, and
`boot.before_jump` comes right before the app starts and takes the chip
over. Whatever an extension shows lasts only until then, and every wait
in a handler adds to the boot.

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

DAGON hands a boot extension no peripheral or pin (`Ctx` carries the
layer only). A trusted extension that drives hardware (a display, a pin)
takes it from the MCU's HAL itself in `on_event`, after the boot core's
HAL init (it runs privileged). What it takes, and that this is safe, is
the extension's author's responsibility: DAGON checks none of it. It must
leave alone what the boot core uses (on the RP2040: the flash, the
watchdog, the clocks, and the board's `[boot] led` once the boot core
halts) and keep each call within
its `max_run_us`. The ESP32-S3 has no DAGON boot stage (its
vendor's second stage starts the app), so boot extensions do not exist
there and `dagon gen` refuses them with that reason.
