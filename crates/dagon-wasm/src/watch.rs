//! Monitoring rule: one HELLO per target every 5 s; a target counts as not
//! answering after two misses in a row, and as answering again on the next
//! reply.

use alloc::vec::Vec;
use dagon_host::Target;

pub const INTERVAL_MS: u32 = 5_000;
const MISSES_TO_SILENT: u8 = 2;

#[derive(Default)]
pub struct Watch {
    misses: Vec<(Target, u8)>,
}

impl Watch {
    /// Records one round for `to`; returns whether it still counts as answering.
    pub fn record(&mut self, to: Target, answered: bool) -> bool {
        let i = match self.misses.iter().position(|m| m.0 == to) {
            Some(i) => i,
            None => {
                self.misses.push((to, 0));
                self.misses.len() - 1
            }
        };
        let m = &mut self.misses[i].1;
        *m = if answered { 0 } else { m.saturating_add(1) };
        *m < MISSES_TO_SILENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_misses_in_a_row_then_back_on_reply() {
        let mut w = Watch::default();
        let seg = Target::I2c(0x42);
        assert!(w.record(seg, false));
        assert!(w.record(Target::Uart, false), "targets count separately");
        assert!(!w.record(seg, false));
        assert!(!w.record(seg, false));
        assert!(w.record(seg, true));
        assert!(w.record(seg, false), "a reply resets the count");
    }
}
