# dagon-board-nucleo-f401re

DAGON board package for the NUCLEO-F401RE (topology board id
`nucleo-f401re`) with an X-NUCLEO-53L0A1 stacked on its Arduino headers. It
implements the STM32F401RE MCU package's `BoardDef`.

| Use | Pins |
|---|---|
| DAGON link: I2C3 target (SCL, SDA), address 0x42 (NC on the shield) | PA8 (D7), PB4 (D5) |
| User LED LD2 | PA5 |
| Shield I2C1 (SCL, SDA), the display's bus | PB8 (D15), PB9 (D14) |
| Shield centre VL53L0X GPIO1 (fitted; input only, not handed out) | PA4 (A2) |

The shield's 4-digit display (common anode, static drive, a segment lights
while its expander pin is low) and the VL53L0X XSHUT
lines hang off two STMPE1600 on I2C1 (ST UM2047 Figure 4):

| Expander (7-bit) | GPIO 0..6 | GPIO 7..13 | GPIO 14 | GPIO 15 |
|---|---|---|---|---|
| U21 at 0x43 | digit 1 (left) | digit 2 | XSHUT left | XSHUT right |
| U19 at 0x42 | digit 3 | digit 4 (right) | unused (input) | XSHUT centre |

Within a digit, GPIO n+0..n+6 = segments e, f, g, a, d, b, c. The
shield's decimal-point wiring is not confirmed yet; the driver currently
does not light it. Start-up
writes (`shield::INIT`, levels before directions so nothing flashes):
U19 GPSR (0x12) = 0x3FFF, GPDR (0x14) = 0xBFFF; U21 GPSR = 0x3FFF,
GPDR = 0xFFFF: all segments off, all three sensors held in XSHUT. Each
`show` then writes both GPSRs (U21, then U19). U19's 0x42 is on I2C1, not
on DAGON's I2C3 bus.

- `Board::split` gives DAGON the flash, the IWDG and the I2C3 link (with
  DMA1 streams 4 and 2; this package binds their interrupts), and the app
  `Io`: LD2, the display (`Display`: I2C1 with DMA1 streams 6 and 0) and
  `Spare` (USART2 on PA2/PA3, B1 on PC13, ADC1 with PC0/PC1). I2C1 and
  PA4 are not in `Spare`.
- `Display::open` gives a `SegDisplay`; `show(codes).await` sets the
  expanders up on first use and writes the digits, asynchronously in the
  calling app task (its interrupts at P7, below DAGON's executor), each
  write bounded by a 20 ms timeout. `running_version()` reads the running
  image's version from its header.
- `seg`: text for the 4 characters, one code per digit (segments a..g,
  bit 7 the point). `seg::render` takes up to 4 of `0`-`9`, space, `-`,
  `t`, `S`, `E`, right-aligned; a `.` sets the point of the character
  before it and takes no digit (not lit, see above); anything else is
  `None`. Host-testable with `shield`'s register writes:
  `cargo test --no-default-features --target <host triple>`.
- Flash: 512 KiB (`FLASH_SIZE`, checked against the MCU layout).

Features: `slot-b`, `conform` (forwarded to the MCU package). See
`../../mcu/stm32f401re/README.md` for a complete app and
`../../../samples/f401-app` for the demo node (what it shows is in its
README).
