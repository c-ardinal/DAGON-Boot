//! Stage0's diagnostic output comes from the board of the node whose boot
//! folder `DAGON_BOOT_CRATE` names (`diag.rs`: `BoardDiag` on its `[boot]`
//! LED, or none; `dagon gen` writes the folder in `boot/<id>/`); the build
//! fails without it.

use dagon_topology::defs::codegen::{stage0_build, DiagHal};

fn main() {
    let gpio = |pin: &str| {
        pin.strip_prefix("GPIO")
            .and_then(|n| n.parse::<u8>().ok())
            .map(|n| n.to_string())
    };
    stage0_build(&DiagHal {
        report: "dagon_mcu_rp2040::arm::led::report",
        pin: &gpio,
    });
}
