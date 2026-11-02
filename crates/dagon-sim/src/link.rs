//! SimLink: a frame link with injected faults (loss, bit flips, delay, a
//! silent peer), driven by its own seed-fixed PRNG.

use crate::rng::SimRng;

pub struct SimLink {
    /// Probability that a frame is lost.
    pub drop_p: f64,
    /// Probability that one random bit of a frame is flipped.
    pub flip_p: f64,
    /// One-way delay added to the simulated clock per frame.
    pub delay_ms: u64,
    /// The peer never answers.
    pub silent: bool,
    pub carried: u64,
    pub dropped: u64,
    pub flipped: u64,
    rng: SimRng,
}

impl SimLink {
    /// A clean link.
    pub fn new(seed: u64) -> Self {
        SimLink {
            drop_p: 0.0,
            flip_p: 0.0,
            delay_ms: 0,
            silent: false,
            carried: 0,
            dropped: 0,
            flipped: 0,
            rng: SimRng::new(seed),
        }
    }

    /// Passes one frame through; `None` = lost.
    pub fn carry(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        if self.rng.chance(self.drop_p) || frame.is_empty() {
            self.dropped += 1;
            return None;
        }
        let mut f = frame.to_vec();
        if self.rng.chance(self.flip_p) {
            let bit = self.rng.below(f.len() as u64 * 8);
            f[(bit / 8) as usize] ^= 1 << (bit % 8);
            self.flipped += 1;
        }
        self.carried += 1;
        Some(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_are_counted_and_reproducible() {
        let run = |seed| {
            let mut l = SimLink::new(seed);
            l.drop_p = 0.3;
            l.flip_p = 0.3;
            let out: Vec<_> = (0..50).map(|_| l.carry(&[0u8; 8])).collect();
            (out, l.dropped, l.flipped)
        };
        let (a, dropped, flipped) = run(5);
        assert_eq!(a, run(5).0);
        assert!(dropped > 0 && flipped > 0);
        assert!(SimLink::new(1).carry(b"ok").is_some());
    }
}
