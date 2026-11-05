# hub-boot-light (sample trusted boot extension)

User code that runs inside the RP2040 hub's boot core, before the app: a
copy of `targets/boot-extension-template` that shows the boot core's steps
on the backlight of the hub's round LCD (the board's `[boot] led`,
GPIO25). It is a sample, not part of DAGON; it uses the boot events
(`dagon_boot::sequence`) and the MCU package's LED (`arm::led`) only.

| Boot event | Backlight |
|---|---|
| `boot.watchdog_armed` | one flash (30 ms on, 10 ms off) |
| `boot.log_read` | one flash |
| `boot.before_decision` | on while the boot core checks the image (signature, SHA-256) |
| `boot.slot_chosen`, confirmed image | off |
| `boot.slot_chosen`, image on trial | flicker (10 ms off, on, off), then on until the app takes the pin over |
| `boot.no_image`, `boot.halt` | nothing: the boot core's halt blink |

So every boot shows two quick flashes and a glow; a boot of a new version
on trial (after an update) ends lit instead of dark. Without the extension
the backlight stays dark until the app's display turns it on.

Each call keeps within the manifest's `max_run_us` (45 ms; an observer's
handler is bounded by 50 ms). The show adds about 80 ms to a boot (two
40 ms flashes) and about 110 ms to a boot on trial (30 ms more of
flicker). The extension changes nothing the boot core decides.

## Enable it for the hub

`dagon gen` takes extension packages from inside the project only. Copy
this folder to `extensions/hub-boot-light/` of the project and point its
three `path = "../../..."` dependencies at the DAGON checkout the
project's `[dagon] path` names (as the template says for a copy). Then, in
the project's `dagon.toml`:

```toml
[extensions]
packages = ["extensions/*"]
[extensions.nodes.hub]
boot = ["boot-light"]
trusted = [{ id = "boot-light", sha256 = "<the hash dagon gen names>" }]
```

1. `dagon gen hub`: the first run refuses and names the folder's source
   hash; put it in `sha256` (it depends on the copy's paths, so each
   project has its own).
2. If `boot/hub/Cargo.lock` exists from an earlier build, gen then stops
   at the lock (`--locked`): run `cargo metadata --offline
   --format-version 1` in `boot/hub/` once (it adds the extension to the
   lock and changes nothing else), then `dagon gen hub` again.
3. Build the boot core in `boot/hub/` (`cargo build --release`,
   `DAGON_PUBKEY` as for every image). stage0 does not change (a build with
   `DAGON_BOOT_CRATE` pointing at `boot/hub/` is byte-identical with and
   without the extension).

The hash covers every file in the copied folder: run its tests with a
target directory outside it, or `dagon gen` names a new hash:

```sh
CARGO_TARGET_DIR=/tmp/hub-boot-light cargo test --target <host triple>
```

To build the boot core without it, remove `"boot-light"` from the hub's
`boot` list and its entry from the hub's `trusted` list (and the
`extensions/*` package glob only if no other extension uses it), then run
`dagon gen hub` again.

Size: the hub boot core grows by 4,648 bytes (100,176 to 104,824 bytes of
its 128 KiB slot).
