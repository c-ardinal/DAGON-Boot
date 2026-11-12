//! What a board crate supplies to the MCU layer.
//!
//! The MCU layer knows the ESP32-S3; a board crate knows the board: which
//! pins carry the link UART, IO expanders, power managers. It
//! hands the MCU layer a configured link UART (see `link::uart_config`) and
//! an implementation of [`BoardDef`] for the rest. A new ESP32-S3 board needs a
//! board crate (or just these two things in the app), nothing else.

use dagon_events::BoardBringUp;

/// Board extras the link task drives. Every method but
/// [`BoardDef::erase_regions`] has a default; that one returns the erase
/// regions the board's build generated, so a board cannot leave them out:
///
/// ```compile_fail
/// struct NoRegions;
/// impl dagon_mcu_esp32s3::board::BoardDef for NoRegions {}
/// ```
pub trait BoardDef: Send {
    /// The board definition's id (`dagon-board.toml` `id`), for
    /// `app.bring_up`; `None` for a board without one (no bring-up runs).
    fn id(&self) -> Option<&'static str> {
        None
    }

    /// The board's `BoardBringUp` port, run once in the link task before
    /// it serves anything (`app.bring_up`), e.g. pin routing through an IO
    /// expander for the link. Keep it short (milliseconds). Default: none,
    /// so no bring-up runs and no `app.bring_up` is emitted.
    fn bring_up_port(&mut self) -> Option<&mut dyn BoardBringUp> {
        None
    }

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

    /// The flash this board's nodes erase in: the erase regions its build
    /// generated (`ERASE_REGIONS` rows in a board package's def.rs, or in a
    /// node's wiring.rs), which are the board's `[flash] erase_regions`
    /// when it carries its own flash part, else the MCU's. Required, so no
    /// board leaves them out. The slot store plans every erase over these
    /// and the MCU's slot layouts.
    fn erase_regions(&self) -> &'static [dagon_port::timeout::EraseRegion];
}

/// A board with no extras: only the link UART and USB Serial/JTAG, and the
/// erase regions the node's build generated (wiring.rs `ERASE_REGIONS`).
#[derive(Clone, Copy, Debug)]
pub struct PlainBoard(pub &'static [dagon_port::timeout::EraseRegion]);

impl BoardDef for PlainBoard {
    fn erase_regions(&self) -> &'static [dagon_port::timeout::EraseRegion] {
        self.0
    }
}
