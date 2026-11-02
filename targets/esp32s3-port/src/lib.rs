//! M5StopWatch (ESP32-S3) port: DAGON kind V node behind the stock ESP-IDF
//! 2nd stage with app rollback enabled.
//!
//! - [`flash::EspFlash`]: `Flash` over the OTA slots and the dagon partition
//! - [`link::UartLink`]: `Link` on the rear UART0 (G43 TX / G44 RX, 115200)
//! - [`boot::EspBootSelect`]: `BootSelectV` over otadata
//! - [`sys`]: `Clock`, `Watchdog` (RTC WDT), `Reset`
//! - [`board`]: rear-pin routing, test-app console, reset-on-panic
//! - [`otadata`]: otadata selection to STATUS slot state (rollback = Reverted)
//!
//! `layout`, `framer` and `otadata` are pure and tested on the host.
#![cfg_attr(not(test), no_std)]

pub mod framer;
pub mod layout;
pub mod otadata;

#[cfg(target_arch = "xtensa")]
pub mod board;
#[cfg(target_arch = "xtensa")]
pub mod boot;
#[cfg(target_arch = "xtensa")]
pub mod flash;
#[cfg(target_arch = "xtensa")]
pub mod link;
#[cfg(target_arch = "xtensa")]
pub mod sys;

/// Trial watchdog: an unconfirmed app is reset after this long without a feed,
/// and the 2nd stage then rolls back to the previous slot.
pub const TRIAL_WDT_MS: u32 = 4000;
