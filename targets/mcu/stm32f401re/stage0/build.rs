//! Stage0's diagnostic output and clock come from the definitions of the
//! node whose boot folder `DAGON_BOOT_CRATE` names (`diag.rs`: `BoardDiag`
//! on its board's `[boot]` LED, or none; `clock.rs`: the MCU's
//! `[[stage0.clock]]` setup for its board's `[clock]`; `dagon gen` writes
//! the folder in `boot/<id>/`); the build fails without it.

use dagon_topology::defs::codegen::{stage0_build, DiagHal};

/// `PA5` → `(0, 5)`: GPIO port A to E or H, pin 0 to 15.
fn port_pin(pin: &str) -> Option<String> {
    let rest = pin.strip_prefix('P')?;
    let port = rest
        .chars()
        .next()
        .filter(|c| matches!(c, 'A'..='E' | 'H'))?;
    let n: u32 = rest[1..].parse().ok().filter(|&n| n < 16)?;
    Some(format!("({}, {n})", port as u32 - 'A' as u32))
}

fn main() {
    let c = stage0_build(&DiagHal {
        report: "crate::indicator::report",
        pin: &port_pin,
    })
    .clock;
    dagon_mcu_build::stage0_clock(c.source.name(), c.in_hz, c.out_hz, &c.pll_fields());
}
