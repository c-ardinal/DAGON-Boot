//! Microseconds from the CPU's cycle counter (CCOUNT) at the frequency the
//! ROM runs the CPU at (`ets_get_cpu_frequency`), the 32-bit counter
//! widened by reading it often (the boot core reads it at every poll, far
//! inside one wrap).

use core::cell::Cell;
use dagon_events::Clock;

extern "C" {
    fn ets_get_cpu_frequency() -> u32;
}

fn ccount() -> u32 {
    let c: u32;
    // SAFETY: reads a special register, no side effect.
    unsafe { core::arch::asm!("rsr.ccount {0}", out(reg) c) };
    c
}

#[derive(Clone)]
pub struct Cycles {
    mhz: u32,
    last: Cell<u32>,
    high: Cell<u64>,
}

impl Cycles {
    pub fn new() -> Self {
        // SAFETY: a ROM function without arguments.
        let mhz = unsafe { ets_get_cpu_frequency() }.max(1);
        Cycles {
            mhz,
            last: Cell::new(ccount()),
            high: Cell::new(0),
        }
    }

    fn cycles(&self) -> u64 {
        let c = ccount();
        if c < self.last.get() {
            self.high.set(self.high.get() + (1 << 32));
        }
        self.last.set(c);
        self.high.get() | u64::from(c)
    }
}

impl Clock for Cycles {
    fn now_us(&mut self) -> u64 {
        self.cycles() / u64::from(self.mhz)
    }
}
