//! The boot LED and halt pattern come from the board definition:
//! `DAGON_BOARD` names the board, else the MCU's default board
//! (`board.rs`).

use dagon_topology::defs::board_boot::HaltPattern;
use dagon_topology::defs::codegen::boot_build;

fn main() {
    let b = boot_build(None);
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
