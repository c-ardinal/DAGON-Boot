//! The halt LED and pattern come from the board definition: `DAGON_BOARD`
//! names the board, else the MCU's default board (`board.rs`, `pins.rs`).

use dagon_topology::defs::codegen::{boot_build, Hal};

fn main() {
    let name = |s: &str| s.to_uppercase();
    let b = boot_build(Some(&Hal {
        peripherals: "embassy_stm32::peripherals",
        name: &name,
        lifetime: false,
    }));
    println!("cargo::rustc-check-cfg=cfg(boot_led)");
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
