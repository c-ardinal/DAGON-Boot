# dagon-board-waveshare-rp2040-touch-lcd-1-28

DAGON board package for the Waveshare RP2040-Touch-LCD-1.28 (topology board
id `waveshare-rp2040-touch-lcd-1.28`; Cargo package names cannot hold the
`.`, so it reads `-1-28` there). It implements the RP2040 MCU package's
`BoardDef` and adds the board's extras.

| Use | Pins |
|---|---|
| Display SPI1 (CLK, MOSI), CS, DC, RST, backlight | GPIO10, 11, 9, 8, 13, 25 |
| Touch controller reset (held high) | GPIO22 |
| Sub-node I2C0 (SDA, SCL), 400 kHz | GPIO16, 17 |
| Sub-node PIO0 UART (TX, RX), 115200 8N1 | GPIO26, 27 |
| Sub-node NRST (open drain), BOOT0 | GPIO18, 28 |
| Diagnostic blink codes | GPIO25 (backlight) |

- `Board::split` gives DAGON the flash, the watchdog and USB, and the app
  `Io`: the display, the touch reset, and the sub-node I2C, UART and
  control lines.
- `Display::pins` sets up SPI1 at 20 MHz with DMA; `gc9a01` is the panel
  driver (RAM frame buffer, whole-frame DMA flush).
- `SubI2c::link`, `SubUart::stream`, `SubCtl::lines` set up the hub's links
  to sub nodes.
- Flash: 4 MiB (`FLASH_SIZE`, checked against the MCU layout).

Features: `usb` (DAGON over USB, `node::start`), `slot-b`, `diag-panic`
(forwarded to the MCU package). See `../../mcu/rp2040/README.md` for a
complete app and `../../rp2040-hub` for DAGON's hub firmware on this board.
