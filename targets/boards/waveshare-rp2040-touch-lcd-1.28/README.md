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

`start` runs the DAGON 親機 on this board: the relay of the MCU package
over the board's I2C0, PIO UART and STM32 control lines, the touch
controller held out of reset and, with `display`, the round display's home
screen (guard task `DISPLAY_TASK`; the app's tasks start at `APP_TASKS`).
This package binds I2C0_IRQ, PIO0_IRQ_0 and DMA_IRQ_0 (`Irqs`). It
returns `Spare`, everything DAGON does not use, for the app: GPIO 0-7, 12,
14, 15, 19-21, 23, 24 and 29, I2C1, SPI0, UART0/1, PIO1, the ADC, DMA
channels 1-11, the eight PWM slices and CORE1 (DAGON runs on core 0).
Some of those pins are wired on the board itself:

| Pin | On the board |
|---|---|
| GPIO6, GPIO7 | I2C1 SDA, SCL of the touch controller (CST816S) and the IMU (QMI8658) |
| GPIO21 | touch controller interrupt |
| GPIO23, GPIO24 | IMU interrupts INT1, INT2 |
| GPIO12 | LCD MISO (SPI1 RX; DAGON's display only writes) |
| GPIO29 | battery voltage (ADC3, through a divider) |

The home screen (`screen`, host-tested without `hal`) shows the state
words in Japanese (確定, 試行中, 無応答, 巻き戻し; 検証, 再起動, 署名不正,
鍵不一致, 非互換, 破損, 拒否, 失敗) with an 11x11 bitmap of only those characters
(`src/glyphs.rs`), rasterised from Noto Sans JP Regular by
`tools/gen_glyphs.py`; SIL Open Font License 1.1, see
`fonts/OFL-NotoSansJP.txt`. A rolled-back node's ring is yellow. "TEST KEY"
sits below the lowest node ring for 0 to 3 sub nodes.

Features: `relay` and `display` (default), `diag` (display test pattern),
`slot-b`, `diag-panic` (forwarded to the MCU package). `template/` is the
project `dagon new` generates for a 親機 on this board. See `../../mcu/rp2040/README.md` for a
complete app and `../../rp2040-hub` for DAGON's hub firmware on this board.
