//! The board's facts come from dagon-board.toml: `def.rs` (ids, sizes, bus
//! speeds, the boot LED) and `pins.rs` (pins and blocks by name, `take!`).

use dagon_topology::defs::codegen::{package_build, Hal};

/// embassy-rp names pin `GPIOn` `PIN_n`.
fn name(s: &str) -> String {
    match s.strip_prefix("GPIO") {
        Some(n) => format!("PIN_{n}"),
        None => s.to_uppercase(),
    }
}

fn main() {
    package_build(&Hal {
        peripherals: "embassy_rp::peripherals",
        name: &name,
        lifetime: false,
    });
}
