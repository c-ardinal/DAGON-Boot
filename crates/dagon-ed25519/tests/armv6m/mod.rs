//! A small ARMv6-M (Cortex-M0+) interpreter for cycle estimates: it runs
//! one function of a Thumb ELF until the function returns and counts its
//! instructions and cycles by the Cortex-M0+ Technical Reference Manual's
//! instruction timings (ALU and MULS 1 cycle, loads and stores 2, LDM/STM
//! and PUSH 1 + N, POP with PC 3 + N, a taken branch 2, BL 3, BX and BLX
//! 2). Flash wait states and the XIP cache are not modelled: the count is
//! the core's alone, a lower bound of the time on a part.
//!
//! It implements the 16-bit Thumb instructions and BL, the subset a
//! compiler emits for code without exceptions; anything else stops the run.

/// Where the program's flash and RAM sit, as m0.ld places them.
const FLASH: u32 = 0x1000_0000;
const FLASH_LEN: usize = 1 << 20;
const RAM: u32 = 0x2000_0000;
const RAM_LEN: usize = 256 * 1024;
/// The return address the run starts with: reaching it ends the run.
const STOP: u32 = 0xFFFF_FFFE;

/// What a run counted.
#[derive(Debug, Clone, Copy)]
pub struct Run {
    /// R0 when the function returned.
    pub r0: u32,
    pub instructions: u64,
    pub cycles: u64,
    /// The deepest the stack went, bytes.
    pub stack: u32,
}

struct Cpu {
    flash: Vec<u8>,
    ram: Vec<u8>,
    r: [u32; 16],
    n: bool,
    z: bool,
    c: bool,
    v: bool,
    cycles: u64,
    instructions: u64,
    sp_min: u32,
}

/// `w` bits of `x` from bit `lo`.
fn bits(x: u32, lo: u32, w: u32) -> u32 {
    (x >> lo) & ((1 << w) - 1)
}

impl Cpu {
    fn mem(&mut self, a: u32, len: u32) -> &mut [u8] {
        let (mem, base) = if a >= RAM {
            (&mut self.ram, RAM)
        } else {
            (&mut self.flash, FLASH)
        };
        let o = a
            .checked_sub(base)
            .unwrap_or_else(|| panic!("access at {a:#x}")) as usize;
        &mut mem[o..o + len as usize]
    }

    fn load(&mut self, a: u32, len: u32) -> u32 {
        self.mem(a, len)
            .iter()
            .enumerate()
            .fold(0, |v, (i, b)| v | u32::from(*b) << (8 * i))
    }

    fn store(&mut self, a: u32, len: u32, v: u32) {
        assert!(a >= RAM, "store to flash at {a:#x}");
        for (i, b) in self.mem(a, len).iter_mut().enumerate() {
            *b = (v >> (8 * i)) as u8;
        }
    }

    fn nz(&mut self, r: u32) {
        self.n = r >> 31 == 1;
        self.z = r == 0;
    }

    /// `x + y + carry`, setting all four flags.
    fn add(&mut self, x: u32, y: u32, carry: bool) -> u32 {
        let s = u64::from(x) + u64::from(y) + u64::from(carry);
        let r = s as u32;
        self.c = s >> 32 == 1;
        self.v = ((x ^ r) & (y ^ r)) >> 31 == 1;
        self.nz(r);
        r
    }

    /// `x - y`, setting all four flags.
    fn sub(&mut self, x: u32, y: u32) -> u32 {
        self.add(x, !y, true)
    }

    fn cond(&self, c: u32) -> bool {
        let r = match c >> 1 {
            0 => self.z,
            1 => self.c,
            2 => self.n,
            3 => self.v,
            4 => self.c && !self.z,
            5 => self.n == self.v,
            6 => !self.z && self.n == self.v,
            _ => true,
        };
        if c & 1 == 1 && c != 15 {
            !r
        } else {
            r
        }
    }

    /// Shift `v` by `s` (LSL 0, LSR 1, ASR 2, ROR 3), setting the carry.
    fn shift(&mut self, op: u32, v: u32, s: u32) -> u32 {
        if s == 0 {
            return v;
        }
        match op {
            0 if s < 32 => {
                self.c = (v >> (32 - s)) & 1 == 1;
                v << s
            }
            0 => {
                self.c = s == 32 && v & 1 == 1;
                0
            }
            1 if s < 32 => {
                self.c = (v >> (s - 1)) & 1 == 1;
                v >> s
            }
            1 => {
                self.c = s == 32 && v >> 31 == 1;
                0
            }
            2 if s < 32 => {
                self.c = (v >> (s - 1)) & 1 == 1;
                ((v as i32) >> s) as u32
            }
            2 => {
                self.c = v >> 31 == 1;
                ((v as i32) >> 31) as u32
            }
            _ => {
                let r = v.rotate_right(s & 31);
                self.c = r >> 31 == 1;
                r
            }
        }
    }

    /// Executes one instruction.
    fn step(&mut self) {
        let pc = self.r[15];
        let hw = self.load(pc, 2);
        let (next, cycles) = self.execute(pc, hw);
        self.r[15] = next;
        self.cycles += cycles;
        self.instructions += 1;
        self.sp_min = self.sp_min.min(self.r[13]);
    }

    /// Runs `hw` at `pc`: the next PC and the cycles it took.
    fn execute(&mut self, pc: u32, hw: u32) -> (u32, u64) {
        let pcv = pc + 4;
        let (lo, mid) = (bits(hw, 0, 3) as usize, bits(hw, 3, 3) as usize);
        let top = bits(hw, 11, 5);
        let mut next = pc + 2;
        let mut cycles = 1;
        match top {
            // LSL, LSR, ASR (immediate)
            0..=2 => {
                let imm = bits(hw, 6, 5);
                let s = if imm == 0 && top != 0 { 32 } else { imm };
                let r = self.shift(top, self.r[mid], s);
                self.r[lo] = r;
                self.nz(r);
            }
            // ADD, SUB (register or 3-bit immediate)
            3 => {
                let x = bits(hw, 6, 3);
                let y = if hw & 0x400 != 0 {
                    x
                } else {
                    self.r[x as usize]
                };
                let a = self.r[mid];
                self.r[lo] = if hw & 0x200 != 0 {
                    self.sub(a, y)
                } else {
                    self.add(a, y, false)
                };
            }
            // MOV, CMP, ADD, SUB (8-bit immediate)
            4..=7 => {
                let d = bits(hw, 8, 3) as usize;
                let i = hw & 0xFF;
                match top & 3 {
                    0 => {
                        self.r[d] = i;
                        self.nz(i);
                    }
                    1 => {
                        self.sub(self.r[d], i);
                    }
                    2 => self.r[d] = self.add(self.r[d], i, false),
                    _ => self.r[d] = self.sub(self.r[d], i),
                }
            }
            8 if hw & 0x400 == 0 => self.data_processing(hw),
            8 => (next, cycles) = self.hi_register(hw, pc, pcv, next),
            // LDR (literal)
            9 => {
                let a = (pcv & !3) + (hw & 0xFF) * 4;
                self.r[bits(hw, 8, 3) as usize] = self.load(a, 4);
                cycles = 2;
            }
            // Loads and stores (register offset)
            10 | 11 => {
                let a = self.r[mid].wrapping_add(self.r[bits(hw, 6, 3) as usize]);
                cycles = 2;
                let t = self.r[lo];
                match bits(hw, 9, 3) {
                    0 => self.store(a, 4, t),
                    1 => self.store(a, 2, t),
                    2 => self.store(a, 1, t),
                    3 => self.r[lo] = self.load(a, 1) as u8 as i8 as u32,
                    4 => self.r[lo] = self.load(a, 4),
                    5 => self.r[lo] = self.load(a, 2),
                    6 => self.r[lo] = self.load(a, 1),
                    _ => self.r[lo] = self.load(a, 2) as u16 as i16 as u32,
                }
            }
            // Loads and stores (immediate offset): word, byte, halfword
            12..=17 => {
                let imm = bits(hw, 6, 5);
                let (len, off) = match top {
                    12 | 13 => (4, imm * 4),
                    14 | 15 => (1, imm),
                    _ => (2, imm * 2),
                };
                let a = self.r[mid].wrapping_add(off);
                cycles = 2;
                if hw & 0x800 != 0 {
                    self.r[lo] = self.load(a, len);
                } else {
                    self.store(a, len, self.r[lo]);
                }
            }
            // LDR, STR (SP-relative)
            18 | 19 => {
                let a = self.r[13] + (hw & 0xFF) * 4;
                let t = bits(hw, 8, 3) as usize;
                cycles = 2;
                if hw & 0x800 != 0 {
                    self.r[t] = self.load(a, 4);
                } else {
                    self.store(a, 4, self.r[t]);
                }
            }
            // ADR, ADD (SP plus immediate)
            20 => self.r[bits(hw, 8, 3) as usize] = (pcv & !3) + (hw & 0xFF) * 4,
            21 => self.r[bits(hw, 8, 3) as usize] = self.r[13] + (hw & 0xFF) * 4,
            22 | 23 => (next, cycles) = self.misc(hw, next),
            // LDM, STM
            24 | 25 => {
                let n = bits(hw, 8, 3) as usize;
                let list = hw & 0xFF;
                let mut a = self.r[n];
                let load = hw & 0x800 != 0;
                cycles = 1 + u64::from(list.count_ones());
                for i in (0..8).filter(|i| list >> i & 1 == 1) {
                    if load {
                        self.r[i] = self.load(a, 4);
                    } else {
                        self.store(a, 4, self.r[i]);
                    }
                    a += 4;
                }
                if !(load && list >> n & 1 == 1) {
                    self.r[n] = a;
                }
            }
            // B<cond>; 14 is UDF, 15 SVC
            26 | 27 => {
                let c = bits(hw, 8, 4);
                assert!(c < 14, "UDF or SVC at {pc:#x}");
                if self.cond(c) {
                    next = pcv.wrapping_add((hw as u8 as i8 as i32 * 2) as u32);
                    cycles = 2;
                }
            }
            // B
            28 => {
                let off = ((hw & 0x7FF) << 21) as i32 >> 20;
                next = pcv.wrapping_add(off as u32);
                cycles = 2;
            }
            // 32-bit: BL, or the barriers DMB/DSB/ISB (no effect here)
            _ => {
                let hw2 = self.load(pc + 2, 2);
                next = pc + 4;
                cycles = 3;
                if top == 30 && hw2 & 0xD000 == 0xD000 {
                    let s = bits(hw, 10, 1);
                    let i1 = 1 ^ bits(hw2, 13, 1) ^ s;
                    let i2 = 1 ^ bits(hw2, 11, 1) ^ s;
                    let imm =
                        s << 24 | i1 << 23 | i2 << 22 | (hw & 0x3FF) << 12 | (hw2 & 0x7FF) << 1;
                    let off = ((imm << 7) as i32) >> 7;
                    self.r[14] = (pc + 4) | 1;
                    next = pcv.wrapping_add(off as u32);
                } else {
                    assert!(
                        hw == 0xF3BF && hw2 & 0xFF00 == 0x8F00,
                        "unsupported instruction {hw:#06x} {hw2:#06x} at {pc:#x}"
                    );
                }
            }
        }
        (next, cycles)
    }

    /// AND, EOR, LSL, LSR, ASR, ADC, SBC, ROR, TST, RSB, CMP, CMN, ORR,
    /// MUL, BIC, MVN (registers).
    fn data_processing(&mut self, hw: u32) {
        let (d, m) = (bits(hw, 0, 3) as usize, bits(hw, 3, 3) as usize);
        let (a, b) = (self.r[d], self.r[m]);
        let r = match bits(hw, 6, 4) {
            0 => a & b,
            1 => a ^ b,
            2 => self.shift(0, a, b & 0xFF),
            3 => self.shift(1, a, b & 0xFF),
            4 => self.shift(2, a, b & 0xFF),
            5 => self.add(a, b, self.c),
            6 => self.add(a, !b, self.c),
            7 => self.shift(3, a, b & 0xFF),
            8 => return self.nz(a & b),
            9 => self.sub(0, b),
            10 => {
                self.sub(a, b);
                return;
            }
            11 => {
                self.add(a, b, false);
                return;
            }
            12 => a | b,
            13 => a.wrapping_mul(b),
            14 => a & !b,
            _ => !b,
        };
        self.r[d] = r;
        self.nz(r);
    }

    /// ADD, CMP, MOV with high registers; BX, BLX.
    fn hi_register(&mut self, hw: u32, pc: u32, pcv: u32, next: u32) -> (u32, u64) {
        let m = bits(hw, 3, 4) as usize;
        let d = bits(hw, 0, 3) as usize + 8 * bits(hw, 7, 1) as usize;
        let val = |cpu: &Cpu, i: usize| if i == 15 { pcv } else { cpu.r[i] };
        match bits(hw, 8, 2) {
            0 => {
                let r = val(self, d).wrapping_add(val(self, m));
                if d == 15 {
                    return (r & !1, 2);
                }
                self.r[d] = r;
            }
            1 => {
                self.sub(val(self, d), val(self, m));
            }
            2 => {
                let r = val(self, m);
                if d == 15 {
                    return (r & !1, 2);
                }
                self.r[d] = r;
            }
            _ => {
                let target = val(self, m);
                if hw & 0x80 != 0 {
                    self.r[14] = (pc + 2) | 1;
                }
                return (target & !1, 2);
            }
        }
        (next, 1)
    }

    /// ADD/SUB SP, SXTH, SXTB, UXTH, UXTB, PUSH, POP, REV, REV16, REVSH and
    /// the hints (no effect here).
    fn misc(&mut self, hw: u32, next: u32) -> (u32, u64) {
        let (d, m) = (bits(hw, 0, 3) as usize, bits(hw, 3, 3) as usize);
        let list = hw & 0x1FF;
        let n = list.count_ones();
        match bits(hw, 8, 4) {
            0 => {
                let k = (hw & 0x7F) * 4;
                self.r[13] = if hw & 0x80 != 0 {
                    self.r[13] - k
                } else {
                    self.r[13] + k
                };
            }
            2 => {
                let v = self.r[m];
                self.r[d] = match bits(hw, 6, 2) {
                    0 => v as u16 as i16 as u32,
                    1 => v as u8 as i8 as u32,
                    2 => v & 0xFFFF,
                    _ => v & 0xFF,
                };
            }
            4 | 5 => {
                let mut a = self.r[13] - 4 * n;
                self.r[13] = a;
                for i in (0..8).filter(|i| list >> i & 1 == 1) {
                    self.store(a, 4, self.r[i]);
                    a += 4;
                }
                if list & 0x100 != 0 {
                    self.store(a, 4, self.r[14]);
                }
                return (next, 1 + u64::from(n));
            }
            12 | 13 => {
                let mut a = self.r[13];
                for i in (0..8).filter(|i| list >> i & 1 == 1) {
                    self.r[i] = self.load(a, 4);
                    a += 4;
                }
                let mut to = next;
                let mut cycles = 1 + u64::from(n);
                if list & 0x100 != 0 {
                    to = self.load(a, 4) & !1;
                    a += 4;
                    cycles += 2;
                }
                self.r[13] = a;
                return (to, cycles);
            }
            10 => {
                let v = self.r[m];
                self.r[d] = match bits(hw, 6, 2) {
                    0 => v.swap_bytes(),
                    1 => v.swap_bytes().rotate_right(16),
                    _ => (v as u16).swap_bytes() as i16 as u32,
                };
            }
            // CPS, BKPT, NOP, YIELD, WFE, WFI, SEV
            _ => {}
        }
        (next, 1)
    }
}

/// Runs the ELF's entry function with `r0` as its argument until it
/// returns.
pub fn run(elf: &[u8], r0: u32) -> Run {
    let u32_at = |o: usize| u32::from_le_bytes(elf[o..o + 4].try_into().unwrap());
    let u16_at = |o: usize| usize::from(u16::from_le_bytes(elf[o..o + 2].try_into().unwrap()));
    let mut cpu = Cpu {
        flash: vec![0; FLASH_LEN],
        ram: vec![0; RAM_LEN],
        r: [0; 16],
        n: false,
        z: false,
        c: false,
        v: false,
        cycles: 0,
        instructions: 0,
        sp_min: u32::MAX,
    };
    let (phoff, phentsize, phnum) = (u32_at(0x1C) as usize, u16_at(0x2A), u16_at(0x2C));
    for i in 0..phnum {
        let p = phoff + i * phentsize;
        // PT_LOAD: copy the file bytes where the segment links.
        if u32_at(p) == 1 {
            let (off, vaddr, len) = (u32_at(p + 4) as usize, u32_at(p + 8), u32_at(p + 16));
            cpu.mem(vaddr, len)
                .copy_from_slice(&elf[off..off + len as usize]);
        }
    }
    let top = RAM + RAM_LEN as u32;
    cpu.r[0] = r0;
    cpu.r[13] = top;
    cpu.r[14] = STOP | 1;
    cpu.r[15] = u32_at(0x18) & !1;
    while cpu.r[15] != STOP {
        cpu.step();
    }
    Run {
        r0: cpu.r[0],
        instructions: cpu.instructions,
        cycles: cpu.cycles,
        stack: top - cpu.sp_min,
    }
}
