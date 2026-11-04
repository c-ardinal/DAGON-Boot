# dagon-board-nucleo-f401re

DAGON board package for the NUCLEO-F401RE (topology board id
`nucleo-f401re`). It implements the STM32F401RE MCU package's `BoardDef`.
Every board fact is read from `dagon-board.toml` when the package builds
(`build.rs`): `def` holds the constants, `def::pins` the pin and block types
and `take!`.

| Use | Pins |
|---|---|
| DAGON link (bus `link`): I2C3 target (SCL, SDA), address `default_link.addr` (0x42) | PA8 (D7), PB4 (D5) |
| User LED LD2; the boot core's halt LED (`[boot] led`, 10 Hz) | PA5 |
| Arduino I2C1 (bus `arduino-i2c`, SDA, SCL) | PB9 (D14), PB8 (D15) |
| ST-LINK virtual COM port (bus `vcp`, USART2 TX, RX) | PA2, PA3 |
| User button B1 | PC13 |
| A4, A5 (ADC1) | PC1, PC0 |

- `Board::split` gives DAGON the flash, the IWDG and the I2C3 link (with
  DMA1 streams 4 and 2; this package binds their interrupts), and the app
  `Io`: LD2, `arduino_i2c` (I2C1 with its pins and DMA1 streams 6 and 0,
  as plain peripherals) and `Spare` (USART2 on PA2/PA3, B1 on PC13, ADC1
  with PC0/PC1).
- What is stacked on the Arduino headers is the app's: its driver opens
  `arduino_i2c` and binds the bus's interrupts.
- Flash: 512 KiB (`FLASH_SIZE`, from `flash_bytes`, checked against the MCU
  layout).

Features: `slot-b`, `conform` (forwarded to the MCU package). See
`../../mcu/stm32f401re/README.md` for a complete app.
