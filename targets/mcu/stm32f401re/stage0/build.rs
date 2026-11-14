//! Stage0's diagnostic output comes from the board of the node whose boot
//! folder `DAGON_BOOT_CRATE` names (`diag.rs`: `BoardDiag` on its `[boot]`
//! LED, or none; `dagon gen` writes the folder in `boot/<id>/`); the build
//! fails without it.

use dagon_topology::defs::codegen::{stage0_build, DiagHal};

/// `PA5` → `(0, 5)`: GPIO port A to E or H, pin 0 to 15.
fn port_pin(pin: &str) -> Option<String> {
    let rest = pin.strip_prefix('P')?;
    let port = rest.chars().next().filter(|c| matches!(c, 'A'..='E' | 'H'))?;
    let n: u32 = rest[1..].parse().ok().filter(|&n| n < 16)?;
    Some(format!("({}, {n})", port as u32 - 'A' as u32))
}

fn main() {
    stage0_build(&DiagHal {
        report: "crate::indicator::report",
        pin: &port_pin,
    });
}
