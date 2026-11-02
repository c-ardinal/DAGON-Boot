# dagon-board-nucleo-f401re

DAGON board package for the NUCLEO-F401RE (topology board id
`nucleo-f401re`). It implements the STM32F401RE MCU package's `BoardDef`.

| Use | Pins |
|---|---|
| DAGON link: I2C3 target (SCL, SDA), address 0x42 | PA8 (D7), PB4 (D5) |
| User LED LD2 | PA5 |
| 7-segment a..g, dp (direct drive, high = on) | PA10, PB5, PA9, PC7, PB6, PA7, PA6, PA0 |
| 7-segment digit selects (low = on) | PA1, PA4, PB0 |

- `Board::split` gives DAGON the flash, the IWDG and the I2C3 link (with
  DMA1 streams 4 and 2; this package binds their interrupts), and the app
  `Io`: LD2 and the 7-segment display pins.
- `seg`: text for the 3-digit 7-segment display (host-testable:
  `cargo test --no-default-features --target <host triple>`).
- The 7-segment board's pin map is not confirmed yet (Arduino header pins
  clear of I2C3, I2C2, LD2 and the ST-LINK UART).
- Flash: 512 KiB (`FLASH_SIZE`, checked against the MCU layout).

Features: `slot-b`, `conform` (forwarded to the MCU package). See
`../../mcu/stm32f401re/README.md` for a complete app and
`../../../samples/f401-app` for the demo node.
