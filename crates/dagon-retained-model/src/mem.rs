//! Byte-granular memory with fault injection. Every stored byte and every
//! erased byte is one step; the harness can stop a run after any step
//! (a torn write), then apply a reset (bytes kept) or a power loss (bytes
//! kept, cleared or randomised as the region's `survives` declares).
//!
//! A store outside the range the running code may write is applied (a real
//! bug would corrupt memory) and logged in [`Mem::stray`] as evidence.

use crate::rng::Rng;
use std::ops::Range;

/// What a power loss does to the region (`[boot.record] survives`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Survives {
    /// Survives a reset; power loss leaves random bytes (SRAM).
    ResetRandom,
    /// Survives a reset; power loss clears every byte to zero (registers).
    ResetCleared,
    /// Survives reset and power loss (battery-backed).
    Power,
}

/// The run stopped at an injected fault point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut;

pub struct Mem {
    bytes: Vec<u8>,
    left: Option<u64>,
    steps: u64,
    allowed: Range<usize>,
    stray: Vec<usize>,
    rng: Rng,
}

impl Mem {
    pub fn new(len: usize, fill: u8, seed: u64) -> Self {
        Mem {
            bytes: vec![fill; len],
            left: None,
            steps: 0,
            allowed: 0..len,
            stray: Vec::new(),
            rng: Rng::new(seed),
        }
    }

    /// Random content, as SRAM after power-up.
    pub fn random(len: usize, seed: u64) -> Self {
        let mut m = Mem::new(len, 0, seed);
        m.randomise(0..len);
        m
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Steps taken since creation.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Stop the run after `after` more steps (`None`: never).
    pub fn cut_after(&mut self, after: Option<u64>) {
        self.left = after;
    }

    /// The range the code about to run may write.
    pub fn allow(&mut self, r: Range<usize>) {
        self.allowed = r;
    }

    /// Addresses written outside the allowed range.
    pub fn stray(&self) -> &[usize] {
        &self.stray
    }

    fn step(&mut self) -> Result<(), Cut> {
        if let Some(n) = self.left.as_mut() {
            if *n == 0 {
                return Err(Cut);
            }
            *n -= 1;
        }
        self.steps += 1;
        Ok(())
    }

    fn check(&mut self, addr: usize) {
        if !self.allowed.contains(&addr) {
            self.stray.push(addr);
        }
    }

    /// RAM or register store of one byte.
    pub fn store(&mut self, addr: usize, v: u8) -> Result<(), Cut> {
        self.step()?;
        self.check(addr);
        self.bytes[addr] = v;
        Ok(())
    }

    /// NOR program of one byte: bits only go from 1 to 0.
    pub fn program(&mut self, addr: usize, v: u8) -> Result<(), Cut> {
        self.step()?;
        self.check(addr);
        self.bytes[addr] &= v;
        Ok(())
    }

    /// NOR erase, byte by byte from the start. Interrupted, it leaves the
    /// prefix erased and the rest either old or random.
    pub fn erase(&mut self, r: Range<usize>) -> Result<(), Cut> {
        for a in r.clone() {
            if self.step().is_err() {
                if self.rng.chance(0.5) {
                    self.randomise(a..r.end);
                }
                return Err(Cut);
            }
            self.check(a);
            self.bytes[a] = 0xFF;
        }
        Ok(())
    }

    /// Harness-only write (definition of a starting state), not a step.
    pub fn poke(&mut self, addr: usize, data: &[u8]) {
        self.bytes[addr..addr + data.len()].copy_from_slice(data);
    }

    pub fn randomise(&mut self, r: Range<usize>) {
        for a in r {
            self.bytes[a] = self.rng.byte();
        }
    }

    pub fn power_loss(&mut self, survives: Survives) {
        let all = 0..self.bytes.len();
        match survives {
            Survives::ResetRandom => self.randomise(all),
            Survives::ResetCleared => self.bytes.fill(0),
            Survives::Power => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_stops_before_the_step() {
        let mut m = Mem::new(4, 0, 1);
        m.cut_after(Some(2));
        assert!(m.store(0, 1).is_ok() && m.store(1, 1).is_ok());
        assert_eq!(m.store(2, 1), Err(Cut));
        assert_eq!(m.bytes(), &[1, 1, 0, 0]);
    }

    #[test]
    fn stray_stores_are_logged() {
        let mut m = Mem::new(4, 0, 1);
        m.allow(0..2);
        m.store(3, 9).unwrap();
        assert_eq!(m.stray(), &[3]);
    }

    #[test]
    fn nor_program_only_clears_bits() {
        let mut m = Mem::new(2, 0xFF, 1);
        m.program(0, 0xF0).unwrap();
        m.program(0, 0x0F).unwrap();
        assert_eq!(m.bytes()[0], 0);
    }
}
