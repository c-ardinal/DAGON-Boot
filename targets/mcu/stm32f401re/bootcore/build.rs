//! The halt LED and pattern come from the board of the node whose boot
//! folder `DAGON_BOOT_CRATE` names (`board.rs`, `pins.rs`), and the clock
//! from the MCU's `[[bootcore.clock]]` setup for that board's `[clock]`
//! (`clock.rs`; the internal one for a node without a board); the build
//! fails without the folder, and without the stamp `dagon gen` writes
//! there once the boot crate's dependency graph passed its check for the
//! current inputs.

use dagon_topology::defs::codegen::{boot_build, boot_verify_stamp, bootcore_clock_build, Hal};

fn main() {
    boot_verify_stamp();
    let name = |s: &str| s.to_uppercase();
    let b = boot_build(Some(&Hal {
        peripherals: "embassy_stm32::peripherals",
        name: &name,
        lifetime: false,
    }));
    bootcore_clock_build(b.as_ref());
    println!("cargo::rustc-check-cfg=cfg(boot_led)");
    let Some(b) = b else {
        return;
    };
    if b.boot.as_ref().is_some_and(|x| x.led.is_some()) {
        println!("cargo::rustc-cfg=boot_led");
    }
    let code = b.boot.and_then(|x| x.halt).map(|h| h.pattern);
    assert!(
        code != Some(dagon_topology::defs::board_boot::HaltPattern::Code),
        "board {}: this boot core shows a halt as blink or off, not code",
        b.id
    );
}
