//! The ESP image stage0 starts: read from flash, checked and turned into a
//! load plan before anything is copied or mapped. The image is the one the
//! ROM format describes (header, segments; esptool `bin_image.py`), as the
//! image tool writes an esp-hal app: RAM segments are copied with 32-bit
//! stores, IROM and DROM segments are mapped through the MMU in 64 KiB
//! pages, and segments with load address 0 are the tool's padding between
//! them. A segment anywhere else, one that leaves the image's slot, a RAM
//! segment that overlaps another (through either bus's address of the same
//! SRAM) or stage0's own window, two mapped segments
//! on one MMU entry (both buses share the table), and an entry point
//! outside the loaded code are refused: stage0 never starts such an image.

use dagon_port::Flash;

/// Header byte 0 of an ESP image.
pub const MAGIC: u8 = 0xE9;
/// Header chip id of the ESP32-S3.
pub const CHIP_ID: u16 = 9;
/// Image header (common 8 bytes and the 16-byte extended header).
pub const HEADER_LEN: u32 = 24;
/// Each segment's load address and length before its data.
pub const SEGMENT_HEADER_LEN: u32 = 8;
/// The MMU page: a mapped segment's flash offset and address agree below it.
pub const MMU_PAGE: u32 = 0x1_0000;
/// The ROM loader's limit, which the image tool keeps.
pub const MAX_SEGMENTS: usize = 16;
/// IROM and DROM segments an app has (the tool writes one of each; a few
/// more are accepted).
pub const MAX_MAPS: usize = 4;
/// RAM segments an app may have.
pub const MAX_COPIES: usize = 8;

/// Internal SRAM on the instruction bus an app may load into: below stage0's
/// window (0x403CB700, as IDF's 2nd stage keeps it), where esp-hal apps
/// link their IRAM.
pub const IRAM: (u32, u32) = (0x4037_0000, 0x403C_B700);
/// Internal SRAM on the data bus, below stage0's window (0x3FCDB700).
pub const DRAM: (u32, u32) = (0x3FC8_8000, 0x3FCD_B700);
/// RTC fast memory (instruction-bus alias) and RTC slow memory.
pub const RTC_FAST: (u32, u32) = (0x600F_E000, 0x6010_0000);
pub const RTC_SLOW: (u32, u32) = (0x5000_0000, 0x5000_2000);
/// External flash on the instruction bus (IROM) and the data bus (DROM).
pub const IROM: (u32, u32) = (0x4200_0000, 0x4400_0000);
pub const DROM: (u32, u32) = (0x3C00_0000, 0x3E00_0000);

const RAM: [(u32, u32); 4] = [IRAM, DRAM, RTC_FAST, RTC_SLOW];

/// SRAM1 seen on the instruction bus from 0x40378000 is the same memory
/// the data bus sees from 0x3FC88000 (SRAM0 below it is instruction
/// only), so copies are compared by the memory they take ([`memory`]).
const SRAM1_IBUS: (u32, u32) = (0x4037_8000, 0x403E_0000);
const SRAM1_DBUS: u32 = 0x3FC8_8000;

/// `len` bytes at `a` as the memory they take: a part in the SRAM1
/// instruction alias at its data-bus address, the rest where it is (a
/// range may cross from SRAM0 into SRAM1). Empty parts are `(0, 0)`.
fn memory(a: u32, len: u32) -> [(u32, u32); 2] {
    let end = a + len;
    let (lo, hi) = (a.max(SRAM1_IBUS.0), end.min(SRAM1_IBUS.1));
    if lo >= hi {
        return [(a, end), (0, 0)];
    }
    let alias = (
        lo - SRAM1_IBUS.0 + SRAM1_DBUS,
        hi - SRAM1_IBUS.0 + SRAM1_DBUS,
    );
    // Below the alias (SRAM0) or above it, never both: the RAM ranges
    // end inside SRAM1.
    let rest = if a < lo { (a, lo) } else { (hi, end) };
    [alias, rest]
}

/// Whether two ranges take some of the same memory.
fn share(a: (u32, u32), b: (u32, u32)) -> bool {
    memory(a.0, a.1).iter().any(|&(s, e)| {
        memory(b.0, b.1)
            .iter()
            .any(|&(t, f)| s < e && t < f && s < f && t < e)
    })
}

/// A RAM segment: `len` bytes of flash at `from` copied to `to`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RamCopy {
    pub from: u32,
    pub to: u32,
    pub len: u32,
}

/// Which cache bus a mapped segment is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bus {
    /// IROM: code.
    #[default]
    Instruction,
    /// DROM: read-only data.
    Data,
}

/// A flash segment: `len` bytes at flash `paddr` seen at `vaddr` on `bus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Map {
    pub vaddr: u32,
    pub paddr: u32,
    pub len: u32,
    pub bus: Bus,
}

impl Map {
    /// Its MMU table entries: the ESP32-S3 has one table for both buses,
    /// indexed by the address's offset in its bus region in pages.
    fn entries(&self) -> (u32, u32) {
        let first = (self.vaddr & 0x01FF_FFFF) / MMU_PAGE;
        (first, first + self.pages().2)
    }

    /// The MMU pages it takes: (first virtual page, first flash page, count).
    pub fn pages(&self) -> (u32, u32, u32) {
        let v = self.vaddr & !(MMU_PAGE - 1);
        let p = self.paddr & !(MMU_PAGE - 1);
        let end = self.vaddr + self.len;
        (v, p, (end - v).div_ceil(MMU_PAGE))
    }
}

/// What stage0 does to start the image: its copies and maps, then a jump
/// to `entry`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub entry: u32,
    copies: [RamCopy; MAX_COPIES],
    ncopies: usize,
    maps: [Map; MAX_MAPS],
    nmaps: usize,
}

impl Plan {
    pub fn copies(&self) -> &[RamCopy] {
        &self.copies[..self.ncopies]
    }

    pub fn maps(&self) -> &[Map] {
        &self.maps[..self.nmaps]
    }
}

/// Why an image is not started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The flash read failed.
    Read,
    /// Not an ESP image (magic).
    Magic,
    /// An image for another chip.
    Chip,
    /// No segment, or more than the ROM format allows.
    Segments,
    /// A segment's data leaves the image's slot.
    Bounds,
    /// A segment loads at `addr`, outside every range an app may use, or
    /// overlapping another RAM segment, or on MMU entries another mapped
    /// segment takes.
    Range(u32),
    /// A mapped segment at `addr` whose flash offset does not agree with
    /// its address below the MMU page, or a RAM segment at `addr` whose
    /// address or length is not a multiple of 4 (copies are 32-bit stores).
    Unaligned(u32),
    /// More RAM or mapped segments than the plan holds.
    TooMany,
    /// The entry point lies in no loaded code.
    Entry,
}

fn within(r: (u32, u32), a: u32, len: u32) -> bool {
    a.checked_add(len).is_some_and(|end| a >= r.0 && end <= r.1)
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The load plan of the ESP image at flash `at` in a slot ending at `end`
/// (exclusive): every segment's data inside the slot, RAM segments inside
/// [`IRAM`], [`DRAM`] or RTC memory without overlapping each other, IROM
/// and DROM segments page-aligned with their flash offset and on MMU entries
/// of their own, RAM segments word-aligned in address and length, padding
/// skipped,
/// and the entry point inside an IRAM copy or an IROM map. Nothing is
/// written.
pub fn plan<F: Flash>(flash: &mut F, at: u32, end: u32) -> Result<Plan, Refused> {
    let mut h = [0u8; HEADER_LEN as usize];
    flash.read(at, &mut h).map_err(|_| Refused::Read)?;
    if h[0] != MAGIC {
        return Err(Refused::Magic);
    }
    if u16::from_le_bytes([h[12], h[13]]) != CHIP_ID {
        return Err(Refused::Chip);
    }
    let count = usize::from(h[1]);
    if count == 0 || count > MAX_SEGMENTS {
        return Err(Refused::Segments);
    }
    let mut plan = Plan {
        entry: le32(&h, 4),
        ..Plan::default()
    };
    let mut off = at.checked_add(HEADER_LEN).ok_or(Refused::Bounds)?;
    for _ in 0..count {
        let mut s = [0u8; SEGMENT_HEADER_LEN as usize];
        let data = off.checked_add(SEGMENT_HEADER_LEN).ok_or(Refused::Bounds)?;
        if data > end {
            return Err(Refused::Bounds);
        }
        flash.read(off, &mut s).map_err(|_| Refused::Read)?;
        let (addr, len) = (le32(&s, 0), le32(&s, 4));
        let next = data.checked_add(len).ok_or(Refused::Bounds)?;
        if next > end {
            return Err(Refused::Bounds);
        }
        if addr == 0 {
            // The image tool's padding before a page-aligned segment.
        } else if within(IROM, addr, len) || within(DROM, addr, len) {
            if (data ^ addr) & (MMU_PAGE - 1) != 0 {
                return Err(Refused::Unaligned(addr));
            }
            let bus = if within(IROM, addr, len) {
                Bus::Instruction
            } else {
                Bus::Data
            };
            let map = Map {
                vaddr: addr,
                paddr: data,
                len,
                bus,
            };
            let (a, b) = map.entries();
            let shares = plan.maps().iter().any(|m| {
                let (c, d) = m.entries();
                a < d && c < b
            });
            if shares {
                return Err(Refused::Range(addr));
            }
            let slot = plan.maps.get_mut(plan.nmaps).ok_or(Refused::TooMany)?;
            *slot = map;
            plan.nmaps += 1;
        } else if RAM.iter().any(|&r| within(r, addr, len)) {
            // Copied by 32-bit stores: a word at a time, ending on a word.
            if addr % 4 != 0 || len % 4 != 0 {
                return Err(Refused::Unaligned(addr));
            }
            // Through either bus: SRAM1 has an address on each.
            let overlaps = plan
                .copies()
                .iter()
                .any(|c| share((addr, len), (c.to, c.len)));
            if overlaps {
                return Err(Refused::Range(addr));
            }
            let slot = plan.copies.get_mut(plan.ncopies).ok_or(Refused::TooMany)?;
            *slot = RamCopy {
                from: data,
                to: addr,
                len,
            };
            plan.ncopies += 1;
        } else {
            return Err(Refused::Range(addr));
        }
        off = next;
    }
    let e = plan.entry;
    let in_code = plan
        .copies()
        .iter()
        .any(|c| within(IRAM, c.to, c.len) && e >= c.to && e - c.to < c.len)
        || plan
            .maps()
            .iter()
            .any(|m| m.bus == Bus::Instruction && e >= m.vaddr && e - m.vaddr < m.len);
    if !in_code {
        return Err(Refused::Entry);
    }
    Ok(plan)
}
