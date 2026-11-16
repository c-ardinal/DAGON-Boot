# dagon-board-waveshare-rp2040-touch-lcd-1-28

DAGON board package for the Waveshare RP2040-Touch-LCD-1.28 (topology board
id `waveshare-rp2040-touch-lcd-1.28`; Cargo package names cannot hold the
`.`, so it reads `-1-28` there). It implements the RP2040 MCU package's
`BoardCore` and `BoardDef` and adds the board's extras. Every board fact is read from
`dagon-board.toml` when the package builds (`build.rs`): `def` holds the
constants (bus speeds, the boot LED), `def::pins` the pin and block types
and `take!`.

| Use | Definition | Pins |
|---|---|---|
| Display SPI1 (CLK, MOSI) at 20 MHz; CS, DC, RST, backlight | bus `lcd`; pins `lcd_cs`, `lcd_dc`, `lcd_rst`, `backlight` | GPIO10, 11; 9, 8, 13, 25 |
| Touch controller reset (held high) | pin `touch_rst` | GPIO22 |
| DAGON's I2C0 to sub nodes (SDA, SCL), 400 kHz | bus `child-i2c` (`dagon = true`) | GPIO16, 17 |
| DAGON's PIO0 UART to sub nodes (TX, RX), 8N1 at the topology link's `baud` (built outside dagon: the bus's `baud`) | bus `child-uart` (`dagon = true`) | GPIO26, 27 |
| Sub-node NRST (open drain), BOOT0 | pins `child_nrst`, `child_boot0` | GPIO18, 28 |
| Boot LED and diagnostic blink codes (1 Hz halt) | `[boot] led`, `halt` | GPIO25 (backlight) |

- `Board::split` gives DAGON the flash, the watchdog and USB, and the app
  `Io`: the touch reset, the I2C, UART and control lines to other nodes,
  and `Spare`, which holds the display's bus and pins.
- `Display::pins` sets up SPI1 at 20 MHz with DMA, write only, and the
  panel's CS, DC, reset and backlight lines; drawing on the panel is the
  app's.
- `SubI2c::link`, `SubUart::stream`, `SubCtl::lines` set up the links to
  other nodes.
- Flash: 4 MiB (`FLASH_SIZE`, checked against the MCU layout).

`start` runs DAGON's relay on this board: the relay of the MCU package
over the board's I2C0, PIO UART and STM32 control lines, with the touch
controller held out of reset; the app's tasks take the guard tasks from
`dagon_mcu_rp2040::relay::TASKS` on. This package binds I2C0_IRQ,
PIO0_IRQ_0 and DMA_IRQ_0 (`Irqs`). It returns `Spare`, everything DAGON
does not use, for the app: the display (`Spare::display`), GPIO 0-7, 12,
14, 15, 19-21, 23, 24 and 29, I2C1, SPI0, UART0/1, PIO1, the ADC, DMA
channels 1-11, the eight PWM slices and CORE1 (DAGON runs on core 0),
and the node's events, which an app draws a display from. Some of those
pins are wired on the board itself:

| Pin | On the board |
|---|---|
| GPIO6, GPIO7 | I2C1 SDA, SCL of the touch controller (CST816S) and the IMU (QMI8658) |
| GPIO21 | touch controller interrupt |
| GPIO23, GPIO24 | IMU interrupts INT1, INT2 |
| GPIO12 | LCD MISO (SPI1 RX; `Display::pins` sets the bus up to write only) |
| GPIO29 | battery voltage (ADC3, through a divider) |

Features: `relay` (default), `slot-b`, `diag-panic` (forwarded to the MCU
package). `template/` is the project `dagon new` generates for a 親機 on
this board. See `../../mcu/rp2040/README.md` for a complete app.
