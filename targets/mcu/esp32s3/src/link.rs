//! The DAGON UART link's settings. The node's links themselves are
//! [`crate::links::Uart`] and [`crate::links::UsbCdc`].

use esp_hal::uart::Config as UartConfig;

/// Link speed: 115200 8N1 on every DAGON UART link.
pub const LINK_BAUD: u32 = 115_200;

/// UART settings for the DAGON link; the board crate applies them to the
/// UART on its link pins.
pub fn uart_config() -> UartConfig {
    UartConfig::default().with_baudrate(LINK_BAUD)
}
