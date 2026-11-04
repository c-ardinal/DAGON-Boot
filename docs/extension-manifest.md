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
buses = []                           # the board's [[bus]] ids
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
| `trusted` | The extension is part of DAGON for the project: it runs privileged and must provide a trusted-only port |
| `limits` | Device layers, untrusted: `max_run_us` 1 to the decision bound (50 ms), `stack_bytes` at least 256, `text_bytes` above 0 |

The crate exports its extension as `pub type DagonExtension` and
`pub const DAGON_EXTENSION: DagonExtension`; the generated chain names them.

## dagon.toml

```toml
[extensions]
packages = ["extensions/*"]          # folders, or folder/* for each subfolder
[extensions.nodes.<id>]
boot = ["uds-transport"]             # the boot core's chain, in order
app = ["uds-transport"]              # the app's chain (a hub's relay too)
trusted = [{ id = "hsm-verify", sha256 = "<artifact SHA-256, 64 hex digits>" }]
```

For each node `dagon gen` refuses, naming the extension and the reason: an
extension listed for an image whose layers it does not have, a port
provided twice in one chain, a trusted extension without its hash,
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
other file in it is removed, and a node gen refused or could not generate
keeps none.

At level `none` no map is made and `dagon gen` notes that the node's
extensions run unguarded.
