//! Stage0's diagnostic output and clock come from the definitions of the
//! node whose boot folder `DAGON_BOOT_CRATE` names (`diag.rs`: `BoardDiag`
//! on its board's `[boot]` LED, or none; `clock.rs`: the MCU's
//! `[[stage0.clock]]` setup for its board's `[clock]`; `dagon gen` writes
//! the folder in `boot/<id>/`); the build fails without it.

use dagon_topology::defs::codegen::{stage0_build, DiagHal};

fn main() {
    let gpio = |pin: &str| {
        pin.strip_prefix("GPIO")
            .and_then(|n| n.parse::<u8>().ok())
            .map(|n| n.to_string())
    };
    let c = stage0_build(&DiagHal {
        report: "crate::report",
        pin: &gpio,
    })
    .clock;
    dagon_mcu_build::stage0_clock(c.source.name(), c.in_hz, c.out_hz, &c.pll_fields());
}
