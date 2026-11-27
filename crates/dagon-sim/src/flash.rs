//! SimFlash: NOR flash with a geometry, an operation log, power-cut points,
//! an operation that fails with the power on, and bits that flip at rest.

use crate::rng::SimRng;
use dagon_port::{Flash, FlashError, FlashRegion};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Erase,
    Write,
}

/// One erase or write, in call order (the log is the list of power-cut points).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashOp {
    pub kind: OpKind,
    pub addr: u32,
    pub len: u32,
}

/// Where in operation `index` (0-based over [`SimFlash::ops`]) the power fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutPoint {
    /// The operation completes, then power is lost.
    After(usize),
    /// Power is lost part-way: a write lands only its first whole write
    /// units, an erase leaves a prefix erased and the rest old or random.
    During(usize),
}

#[derive(Clone)]
pub struct SimFlash {
    regions: Vec<FlashRegion>,
    mem: Vec<Vec<u8>>,
    ops: Vec<FlashOp>,
    cut: Option<CutPoint>,
    /// The operation that fails with the power on, and whether part of it
    /// lands ([`SimFlash::arm_failure`]).
    fail: Option<(usize, bool)>,
    powered: bool,
    rng: SimRng,
}

impl SimFlash {
    /// All regions start erased.
    pub fn new(regions: &[FlashRegion], seed: u64) -> Self {
        SimFlash {
            mem: regions
                .iter()
                .map(|r| vec![r.erased_value; r.len as usize])
                .collect(),
            regions: regions.to_vec(),
            ops: Vec::new(),
            cut: None,
            fail: None,
            powered: true,
            rng: SimRng::new(seed),
        }
    }

    /// The flash built-in MCU definition `mcu` gives in `[execution]`:
    /// its erase regions and erased value, programmed `write_size` bytes
    /// at a time. That is the chip's own unit, which a ROM loader writes
    /// in, finer than the definition's `write_unit` (DAGON's firmware's).
    pub fn of(mcu: &str, write_size: u32, seed: u64) -> Self {
        SimFlash::of_definition(crate::mcu::simulated_mcu(mcu), write_size, seed)
            .unwrap_or_else(|| panic!("MCU definition {mcu} has no [execution]"))
    }

    /// [`SimFlash::of`] for the given definition `def` (a project's own,
    /// its overrides included); `None` when it has no `[execution]`.
    pub fn of_definition(
        def: &dagon_topology::defs::McuDef,
        write_size: u32,
        seed: u64,
    ) -> Option<Self> {
        let e = def.execution.as_ref()?;
        let regions: Vec<_> = e
            .erase_regions
            .iter()
            .map(|r| FlashRegion {
                start: r.start,
                len: r.len,
                erase_size: r.unit,
                write_size,
                erased_value: e.erased_value,
            })
            .collect();
        Some(SimFlash::new(&regions, seed))
    }

    /// STM32F401xE as its definition gives it, byte programming.
    pub fn stm32f401xe(seed: u64) -> Self {
        SimFlash::of("stm32f401re", 1, seed)
    }

    /// RP2040 with 4 MiB flash as its definition gives it, 256-byte pages.
    pub fn rp2040_4m(seed: u64) -> Self {
        SimFlash::of("rp2040", 256, seed)
    }

    /// ESP32-S3 SPI flash as its definition gives it, 4-byte writes.
    pub fn esp32s3(seed: u64) -> Self {
        SimFlash::of("esp32s3", 4, seed)
    }

    pub fn ops(&self) -> &[FlashOp] {
        &self.ops
    }

    pub fn clear_ops(&mut self) {
        self.ops.clear();
    }

    /// Arms one power cut; indices count from the current op log.
    pub fn arm_cut(&mut self, cut: CutPoint) {
        self.cut = Some(cut);
    }

    /// Arms one failed operation with the power left on: operation
    /// `index` (counted as [`SimFlash::arm_cut`] counts) answers
    /// [`FlashError::Hw`], as a worn or disturbed sector does. With
    /// `partly`, a part of it lands first (an erase leaves a prefix
    /// erased, a write its first whole write units); otherwise none. The
    /// operations after it land.
    pub fn arm_failure(&mut self, index: usize, partly: bool) {
        self.fail = Some((index, partly));
    }

    /// Flips bit `bit` of the byte at `addr`, as a cell that loses or
    /// gains charge at rest does: no operation, nothing in the log.
    pub fn flip_bit(&mut self, addr: u32, bit: u8) -> Result<(), FlashError> {
        let (i, off) = self.locate(addr, 1)?;
        self.mem[i][off] ^= 1 << (bit % 8);
        Ok(())
    }

    /// Takes the armed failure when it is for the operation just logged.
    fn failure(&mut self) -> Option<bool> {
        let (index, partly) = self.fail?;
        (index + 1 == self.ops.len()).then(|| {
            self.fail = None;
            partly
        })
    }

    pub fn is_powered(&self) -> bool {
        self.powered
    }

    /// Restores power (contents persist; the armed cut, if spent, is gone).
    pub fn power_on(&mut self) {
        self.powered = true;
    }

    /// Start of erase unit `index`, counting units across all regions (the
    /// STM32 "sector number").
    pub fn sector_addr(&self, index: u32) -> Option<(u32, u32)> {
        let mut left = index;
        for r in &self.regions {
            let n = r.len / r.erase_size;
            if left < n {
                return Some((r.start + left * r.erase_size, r.erase_size));
            }
            left -= n;
        }
        None
    }

    /// Erases every unit, as a chip erase does (each one an erase op).
    pub fn erase_all(&mut self) -> Result<(), FlashError> {
        let mut i = 0;
        while let Some((a, l)) = self.sector_addr(i) {
            self.erase(a, l)?;
            i += 1;
        }
        Ok(())
    }

    /// Erase unit containing `addr`.
    pub fn unit_at(&self, addr: u32) -> Option<FlashRegion> {
        self.locate(addr, 1).ok().map(|(i, _)| self.regions[i])
    }

    /// Direct view for inspection (no power or op-log effects).
    pub fn peek(&self, addr: u32, len: u32) -> Option<&[u8]> {
        let (i, off) = self.locate(addr, len).ok()?;
        Some(&self.mem[i][off..off + len as usize])
    }

    fn locate(&self, addr: u32, len: u32) -> Result<(usize, usize), FlashError> {
        let end = addr as u64 + len as u64;
        self.regions
            .iter()
            .position(|r| addr >= r.start && end <= r.start as u64 + r.len as u64)
            .map(|i| (i, (addr - self.regions[i].start) as usize))
            .ok_or(FlashError::Range)
    }

    /// Records the op and returns the cut that applies to it, if any.
    fn begin(&mut self, kind: OpKind, addr: u32, len: u32) -> Result<Option<CutPoint>, FlashError> {
        if !self.powered {
            return Err(FlashError::Hw);
        }
        let index = self.ops.len();
        self.ops.push(FlashOp { kind, addr, len });
        let hit = match self.cut {
            Some(c @ (CutPoint::After(k) | CutPoint::During(k))) if k == index => Some(c),
            _ => None,
        };
        if hit.is_some() {
            self.cut = None;
        }
        Ok(hit)
    }
}

impl Flash for SimFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &self.regions
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        if !self.powered {
            return Err(FlashError::Hw);
        }
        let (i, off) = self.locate(addr, buf.len() as u32)?;
        buf.copy_from_slice(&self.mem[i][off..off + buf.len()]);
        Ok(())
    }

    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let (i, off) = self.locate(addr, len)?;
        let r = self.regions[i];
        if !(addr - r.start).is_multiple_of(r.erase_size) || !len.is_multiple_of(r.erase_size) {
            return Err(FlashError::Align);
        }
        if len == 0 {
            return Ok(());
        }
        let cut = self.begin(OpKind::Erase, addr, len)?;
        let n = len as usize;
        if let Some(partly) = self.failure() {
            if partly {
                let done = self.rng.below(n as u64) as usize;
                self.mem[i][off..off + done].fill(r.erased_value);
            }
            return Err(FlashError::Hw);
        }
        let done = match cut {
            Some(CutPoint::During(_)) => self.rng.below(n as u64) as usize,
            _ => n,
        };
        self.mem[i][off..off + done].fill(r.erased_value);
        if done < n {
            if self.rng.chance(0.5) {
                for b in &mut self.mem[i][off + done..off + n] {
                    *b = self.rng.byte();
                }
            }
            self.powered = false;
            return Err(FlashError::Hw);
        }
        if cut.is_some() {
            self.powered = false;
        }
        Ok(())
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let (i, off) = self.locate(addr, data.len() as u32)?;
        let r = self.regions[i];
        let ws = r.write_size as usize;
        if !((addr - r.start) as usize).is_multiple_of(ws) || !data.len().is_multiple_of(ws) {
            return Err(FlashError::Align);
        }
        if data.is_empty() {
            return Ok(());
        }
        // Programmed bytes may only be "rewritten" with themselves or with the
        // erased value (padding), which leaves them as they are on NOR.
        let dst = &self.mem[i][off..off + data.len()];
        let clash = dst
            .iter()
            .zip(data)
            .any(|(&old, &new)| old != r.erased_value && new != r.erased_value && new != old);
        if clash {
            return Err(FlashError::Hw);
        }
        let cut = self.begin(OpKind::Write, addr, data.len() as u32)?;
        let failed = self.failure();
        let done = match (cut, failed) {
            (_, Some(false)) => 0,
            (Some(CutPoint::During(_)), _) | (_, Some(true)) => {
                self.rng.below((data.len() / ws) as u64) as usize * ws
            }
            _ => data.len(),
        };
        for (old, &new) in self.mem[i][off..off + done].iter_mut().zip(data) {
            if new != r.erased_value {
                *old = new;
            }
        }
        if cut.is_some() {
            self.powered = false;
        }
        if done < data.len() || failed.is_some() {
            return Err(FlashError::Hw);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_and_op_log() {
        let mut f = SimFlash::stm32f401xe(1);
        assert_eq!(f.erase(0x0800_0001, 0x4000), Err(FlashError::Align));
        f.write(0x0800_0000, &[1, 2]).unwrap();
        assert_eq!(
            f.write(0x0800_0000, &[3]),
            Err(FlashError::Hw),
            "not erased"
        );
        f.erase(0x0800_0000, 0x4000).unwrap();
        assert_eq!(f.peek(0x0800_0000, 2), Some(&[0xFF, 0xFF][..]));
        assert_eq!(f.ops().len(), 2);
        assert_eq!(f.sector_addr(5), Some((0x0802_0000, 0x2_0000)));
        assert_eq!(f.sector_addr(8), None);
        let mut b = [0u8; 1];
        assert_eq!(f.read(0x0808_0000, &mut b), Err(FlashError::Range));
    }

    #[test]
    fn power_cut_during_write_keeps_whole_units() {
        let mut f = SimFlash::rp2040_4m(3);
        f.arm_cut(CutPoint::During(0));
        let data = vec![0u8; 1024];
        assert_eq!(f.write(0x1000_0000, &data), Err(FlashError::Hw));
        assert!(!f.is_powered());
        let landed = f.peek(0x1000_0000, 1024).unwrap();
        let n = landed.iter().take_while(|&&b| b == 0).count();
        assert_eq!(n % 256, 0);
        assert!(n < 1024);
        assert_eq!(
            f.erase(0x1000_0000, 0x1000),
            Err(FlashError::Hw),
            "unpowered"
        );
        f.power_on();
        f.arm_cut(CutPoint::After(1));
        f.erase(0x1000_0000, 0x1000).unwrap();
        assert!(!f.is_powered());
        assert_eq!(f.peek(0x1000_0000, 4), Some(&[0xFF; 4][..]));
    }

    #[test]
    fn a_failed_operation_leaves_the_power_on_and_the_next_ones_land() {
        for partly in [false, true] {
            let mut f = SimFlash::rp2040_4m(7);
            let base = f.geometry()[0].start;
            f.write(base, &[0x11; 256]).unwrap();
            // Operation 1 (the erase) fails; the flash stays powered.
            f.arm_failure(1, partly);
            assert_eq!(f.erase(base, 0x1000), Err(FlashError::Hw));
            assert!(f.is_powered());
            let left = f.peek(base, 256).unwrap();
            if partly {
                assert!(left.iter().all(|&b| b == 0xFF || b == 0x11));
            } else {
                assert!(left.iter().all(|&b| b == 0x11));
            }
            // The retry lands whole, and so does a write after it.
            f.erase(base, 0x1000).unwrap();
            f.write(base, &[0x22; 256]).unwrap();
            assert_eq!(f.peek(base, 256).unwrap(), &[0x22; 256]);
            // A failed write lands at most a prefix of whole write units.
            f.arm_failure(f.ops().len(), partly);
            assert_eq!(f.write(base + 256, &[0x33; 256]), Err(FlashError::Hw));
            let got = f.peek(base + 256, 256).unwrap();
            let landed = got.iter().take_while(|&&b| b == 0x33).count();
            assert!(got[landed..].iter().all(|&b| b == 0xFF));
            assert!(partly || landed == 0);
            assert_eq!(f.ops().len(), 5);
        }
    }

    #[test]
    fn a_flipped_bit_is_no_operation() {
        let mut f = SimFlash::stm32f401xe(1);
        let ops = f.ops().len();
        f.flip_bit(0x0800_0003, 6).unwrap();
        assert_eq!(f.peek(0x0800_0000, 4).unwrap(), &[0xFF, 0xFF, 0xFF, 0xBF]);
        assert_eq!(f.ops().len(), ops);
        assert_eq!(f.flip_bit(0, 0), Err(FlashError::Range));
    }
}
