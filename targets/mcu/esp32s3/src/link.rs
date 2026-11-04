//! The DAGON UART link's settings. The node's links themselves are
//! [`crate::links::Uart`] and [`crate::links::UsbCdc`].

use esp_hal::uart::Config as UartConfig;

/// UART settings for the DAGON link, 8N1 at `baud` (the board's
/// definition gives it); the board crate applies them to the UART on its
/// link pins.
pub fn uart_config(baud: u32) -> UartConfig {
    UartConfig::default().with_baudrate(baud)
}
