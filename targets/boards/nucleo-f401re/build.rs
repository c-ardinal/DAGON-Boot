//! The board's facts come from dagon-board.toml: `def.rs` (ids, sizes, the
//! link address) and `pins.rs` (pins and blocks by name, `take!`).

use dagon_topology::defs::codegen::{package_build, Hal};

fn main() {
    let name = |s: &str| s.to_uppercase();
    package_build(&Hal {
        peripherals: "embassy_stm32::peripherals",
        name: &name,
        lifetime: false,
    });
}
