//! The halt LED and pattern come from the board of the node whose boot
//! folder `DAGON_BOOT_CRATE` names (`board.rs`, `pins.rs`); the build
//! fails without it, and when the node's boot crate links anything but
//! DAGON's packages and the trusted extensions dagon.toml declares with
//! their current source hash.

use dagon_topology::defs::codegen::{boot_build, boot_check_linked, Hal};

fn main() {
    boot_check_linked();
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
