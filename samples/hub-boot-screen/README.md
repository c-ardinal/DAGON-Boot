# hub-boot-screen (sample trusted boot extension)

User code that runs inside the RP2040 hub's boot core, before the app: a
copy of `targets/boot-extension-template` that shows the boot core's steps
on the hub's round LCD, as a gauge of four steps and a line of Japanese
text. It is a sample, not part of DAGON. The boot events
(`dagon_boot::sequence`) tell it what to show.

| Boot event | Gauge | Text |
|---|---|---|
| `boot.watchdog_armed` | 1 of 4 | 起動しています |
| `boot.log_read` | 2 of 4 | 起動しています |
| `boot.before_decision` | 3 of 4 | ファームウェアを確かめています |
| `boot.slot_chosen`, confirmed image | 4 of 4 | 確定した版で起動します |
| `boot.slot_chosen`, image on trial | 4 of 4 | 新しい版を試します |
| `boot.no_image`, `boot.halt` | nothing drawn: the boot core's halt blink on the backlight | |

The app's display replaces the screen when the app starts. Without the
extension the LCD stays dark until then.

## What you see

Usually only the last step's text, briefly, just before the app's screen
appears. The boot is not held up for the screen, and the panel needs time
before it takes commands:

- The panel's reset line is low from the chip's reset on (the pad's
  pull-down), so the panel restarts on every boot. The extension releases
  it at the first event, and the panel takes commands 120 ms later. The
  first three events come within a few milliseconds of each other, so they
  pass before the panel is ready. The screen is drawn at the first event
  where the rest of that wait is at most 20 ms, waited inside the call.
- After sleep out the panel is given another 120 ms before the next command,
  so a step that comes sooner is not drawn.
- The app resets the panel when it starts, so the screen lasts from the
  slot being chosen to the jump to the app.
- Only the text line and the gauge are drawn, never the whole screen.
  Around them the panel shows what its memory held: the app's last screen
  after a reset, or noise after power-on.

The 120 ms figures are those of the hub board package's driver
(`targets/boards/waveshare-rp2040-touch-lcd-1.28/src/gc9a01.rs`). They are
not checked against the GC9A01 datasheet. Neither are the order used here
(the screen written and display on sent before sleep out, so no command
follows sleep out in the same call) and the panel's memory after a reset.

## Time per call

Each call stays within the manifest's `max_run_us` (45 ms; an observer's
handler is bounded by 50 ms). A call stops sending rows once 40 ms have
passed since it started (`DEADLINE_US`), and it waits for the panel at most
20 ms. The longest call is the one that sets the panel up: about 9,000
bytes (the init sequence, the text line of 210 x 14 pixels and the gauge
of 184 x 8 pixels). At an estimated 1 µs per byte (8 bits at 15.625 MHz,
the board's 20 MHz bus as the HAL divides the 125 MHz peripheral clock, plus
the HAL's byte-by-byte blocking write), that is about 9 ms, or about 29 ms
with the longest wait. This estimate is not measured on the board.

## What it takes from the chip

A trusted extension runs privileged in the boot core, and keeping it safe
is its author's job. This one takes its parts straight from the HAL the
boot core runs on (`embassy-rp`, the version the MCU package uses):

- SPI1 with SCK GPIO10 and MOSI GPIO11 at 20 MHz, and the pins CS GPIO9,
  DC GPIO8 and reset GPIO13. These numbers are from the hub's board
  definition (`targets/boards/waveshare-rp2040-touch-lcd-1.28/dagon-board.toml`:
  `[[pin]] lcd_cs`, `lcd_dc`, `lcd_rst` and `[[bus]] lcd`) and are written
  in `src/lib.rs` with those lines. On another board, change them there.
- The TIMER's microsecond counter, read only.
- The backlight through the MCU package's boot LED (`arm::led`), which the
  boot core selects from the board's `[boot] led`.

The boot core uses none of these. The extension changes nothing the boot
core decides.

## Japanese text

`src/glyphs.rs` holds 14 x 14 bitmaps of exactly the characters of
`src/texts.rs`, and nothing more. It is rendered from Noto Sans JP Regular
(SIL Open Font License 1.1, `fonts/OFL-NotoSansJP.txt`) by
`tools/gen_glyphs.py`, which needs Pillow:

```sh
python tools/gen_glyphs.py <NotoSansJP-Regular.ttf>
```

After you change a word, run it again. The host tests fail while the table
and the words differ.

## Enable it for the hub

`dagon gen` takes extension packages from inside the project only. Copy
this folder to `extensions/hub-boot-screen/` of the project. Then point its
`path = "../../..."` dependencies at the DAGON checkout that the project's
`[dagon] path` names, as the template says for a copy. Then, in the
project's `dagon.toml`:

```toml
[extensions]
packages = ["extensions/*"]
[extensions.nodes.hub]
boot = ["boot-screen"]
trusted = [{ id = "boot-screen", sha256 = "<the hash dagon gen names>" }]
```

1. `dagon gen hub`: the first run refuses and names the folder's source
   hash; put it in `sha256` (it depends on the copy's paths, so each
   project has its own).
2. If `boot/hub/Cargo.lock` exists from an earlier build, gen then stops
   at the lock (`--locked`): run `cargo metadata --offline
   --format-version 1` in `boot/hub/` once (it adds the extension to the
   lock and changes nothing else), then `dagon gen hub` again.
3. Build the boot core in `boot/hub/` (`cargo build --release`,
   `DAGON_PUBKEY` as for every image). stage0 does not change.

The hash covers every file in the copied folder. Run its tests with a
target directory outside the folder, or `dagon gen` names a new hash:

```sh
CARGO_TARGET_DIR=/tmp/hub-boot-screen cargo test --target <host triple>
```

To build the boot core without it, remove `"boot-screen"` from the hub's
`boot` list and its entry from the hub's `trusted` list. Remove the
`extensions/*` package glob too, unless another extension uses it. Then
run `dagon gen hub` again.

Size: the hub boot core grows by 8,380 bytes (67,616 to 75,996 bytes of
its 131,072-byte (128 KiB) slot).
