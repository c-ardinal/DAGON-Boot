//! The board's facts come from dagon-board.toml: `def.rs` (sizes, devices)
//! and `pins.rs` (pins and blocks by name, `take!`).

use dagon_topology::defs::codegen::{package_build, Hal};

fn main() {
    let name = |s: &str| s.to_uppercase();
    package_build(&Hal {
        peripherals: "esp_hal::peripherals",
        name: &name,
        lifetime: true,
    });
}
