//! The board's facts come from dagon-board.toml: `def.rs` (ids, sizes, the
//! link address), `pins.rs` (pins and blocks by name, `take!`) and `dma.rs`
//! (the DMA streams of its `dma = true` buses, from the MCU definition's
//! request mapping).

use dagon_topology::defs::codegen::{dma_build, package_build, DmaHal, Hal};

fn main() {
    let name = |s: &str| s.to_uppercase();
    package_build(&Hal {
        peripherals: "embassy_stm32::peripherals",
        name: &name,
        lifetime: false,
    });
    let stream = |c: &str, n: u8| format!("{}_CH{n}", c.to_uppercase());
    let interrupt = |c: &str, n: u8| format!("{}_STREAM{n}", c.to_uppercase());
    dma_build(&DmaHal {
        peripherals: "::embassy_stm32::peripherals",
        stream: &stream,
        interrupt: &interrupt,
        handler: "::embassy_stm32::dma::InterruptHandler",
        bind: "::embassy_stm32::bind_interrupts",
    });
}
