//! What a board crate supplies to the MCU layer.
//!
//! The MCU layer knows the ESP32-S3; a board crate knows the board: which
//! pins carry the link UART, IO expanders, power managers. It
//! hands the MCU layer a configured link UART (see `link::uart_config`) and
//! an implementation of [`BoardDef`] for the rest. A new ESP32-S3 board needs a
//! board crate (or just these two things in the app), nothing else.

/// Board extras the link task drives. Every method has a default, so a
/// plain board implements nothing: `impl BoardDef for MyBoard {}`.
pub trait BoardDef: Send {
    /// Runs once in the link task, before it serves anything: pin routing
    /// through an IO expander for the link. Keep it short (milliseconds).
    fn bring_up(&mut self) {}

    /// ENTER_ROM method 2 is available: the board's power manager can
    /// restart the chip in download mode. Default: no (method 2 answers
    /// E_UNSUPPORTED).
    fn pmic_download_supported(&self) -> bool {
        false
    }

    /// ENTER_ROM method 2. Called after the OK has left the node and the
    /// watchdogs are stopped; return if it did not take (the node then
    /// resets back into the app).
    fn pmic_download(&mut self) {}
}

/// A board with no extras: only the link UART and USB Serial/JTAG.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainBoard;

impl BoardDef for PlainBoard {}
