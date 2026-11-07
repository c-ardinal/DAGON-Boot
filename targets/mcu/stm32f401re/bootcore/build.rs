//! The halt LED and pattern come from the board of the node whose boot
//! folder `DAGON_BOOT_CRATE` names (`board.rs`, `pins.rs`); the build
//! fails without it, and without the stamp `dagon gen` writes there once
//! the boot crate's dependency graph passed its check for the current
//! inputs.

use dagon_topology::defs::codegen::{boot_build, boot_verify_stamp, Hal};

fn main() {
    boot_verify_stamp();
    let name = |s: &str| s.to_uppercase();
    let b = boot_build(Some(&Hal {
        peripherals: "embassy_stm32::peripherals",
        name: &name,
        lifetime: false,
    }));
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
