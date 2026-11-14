//! The boot LED and halt pattern come from the board of the node whose
//! boot folder `DAGON_BOOT_CRATE` names (`board.rs`), and the clock from
//! the MCU's `[[bootcore.clock]]` setup for that board's `[clock]`
//! (`clock.rs`; the internal one for a node without a board); the build
//! fails without the folder, and without the stamp `dagon gen` writes
//! there once the boot crate's dependency graph passed its check for the
//! current inputs.

use dagon_topology::defs::board_boot::HaltPattern;
use dagon_topology::defs::codegen::{boot_build, boot_verify_stamp, bootcore_clock_build};

fn main() {
    boot_verify_stamp();
    let board = boot_build(None);
    bootcore_clock_build(board.as_ref());
    let Some(b) = board else {
        return;
    };
    let boot = b.boot.unwrap_or_default();
    assert!(
        boot.halt.map(|h| h.pattern) != Some(HaltPattern::Code),
        "board {}: this boot image shows a halt as blink or off, not code",
        b.id
    );
    assert!(
        boot.led.is_none()
            || boot
                .led
                .as_deref()
                .and_then(|p| p.strip_prefix("GPIO"))
                .is_some(),
        "board {}: [boot] led must be a GPIOn pin",
        b.id
    );
}
