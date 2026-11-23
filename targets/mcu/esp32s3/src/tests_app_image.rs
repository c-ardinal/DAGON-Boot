//! The load plan stage0 makes of an ESP image: the real app the image tool
//! wrote for the M5StopWatch (its first 64 KiB and the header of its last
//! segment: the plan reads no segment data), and images changed one field
//! at a time.

use crate::app_image::{plan, Bus, Map, RamCopy, Refused, DRAM, IRAM, MAGIC};
use dagon_port::{Flash, FlashError, FlashRegion};

/// `tests/fixtures/m5stopwatch-app-head.bin`: the first 0x10028 bytes of the
/// sample app's image as the image tool wrote it (366,032 bytes; SHA-256
/// bce2de37...0c2820d): header, five segments, the last one's data cut.
const APP_HEAD: &[u8] = include_bytes!("../tests/fixtures/m5stopwatch-app-head.bin");
/// Where the app slot starts in these tests, and its end.
const AT: u32 = 0x1_0000;
const END: u32 = AT + 0x10_0000;

struct Mem {
    base: u32,
    bytes: Vec<u8>,
}

const REGIONS: [FlashRegion; 0] = [];

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let at = addr.checked_sub(self.base).ok_or(FlashError::Range)? as usize;
        let src = self
            .bytes
            .get(at..at + buf.len())
            .ok_or(FlashError::Range)?;
        buf.copy_from_slice(src);
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
    fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
}

/// The slot holding `image` from [`AT`], the rest erased.
fn slot(image: &[u8]) -> Mem {
    let mut bytes = vec![0xFF; (END - AT) as usize];
    bytes[..image.len()].copy_from_slice(image);
    Mem { base: AT, bytes }
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn the_real_app_is_copied_and_mapped_with_its_padding_skipped() {
    let p = plan(&mut slot(APP_HEAD), AT, END).unwrap();
    assert_eq!(p.entry, 0x4037_8498);
    // Segments as the image tool laid them out: DROM, DRAM, IRAM, padding,
    // IROM; data offsets from the slot start.
    assert_eq!(
        p.maps(),
        [
            Map {
                vaddr: 0x3C00_0020,
                paddr: AT + 0x20,
                len: 0x42E8,
                bus: Bus::Data
            },
            Map {
                vaddr: 0x4201_0020,
                paddr: AT + 0x1_0020,
                len: 0x4_9580,
                bus: Bus::Instruction
            },
        ]
    );
    assert_eq!(
        p.copies(),
        [
            RamCopy {
                from: AT + 0x4310,
                to: 0x3FC8_A700,
                len: 0x12B0
            },
            RamCopy {
                from: AT + 0x55C8,
                to: 0x4037_8000,
                len: 0x2700
            },
        ]
    );
    assert_eq!(p.maps()[1].pages(), (0x4201_0000, AT + 0x1_0000, 5));
}

/// The image with segment `i`'s load address set to `addr`.
fn with_segment_addr(i: usize, addr: u32) -> Vec<u8> {
    let mut b = APP_HEAD.to_vec();
    let mut off = 24usize;
    for _ in 0..i {
        let len = u32::from_le_bytes(b[off + 4..off + 8].try_into().unwrap()) as usize;
        off += 8 + len;
    }
    put32(&mut b, off, addr);
    b
}

#[test]
fn a_segment_outside_the_ranges_an_app_may_use_is_refused() {
    for addr in [
        // Stage0's own window, IRAM and DRAM side.
        0x403C_B700,
        0x3FCD_B700,
        // Peripherals, ROM, the other core's cache.
        0x6000_0000,
        0x4000_0000,
        0x3FF0_0000,
    ] {
        let r = plan(&mut slot(&with_segment_addr(1, addr)), AT, END);
        assert_eq!(r, Err(Refused::Range(addr)), "{addr:#x}");
    }
    // Ending past the IRAM it starts in.
    let near_end = IRAM.1 - 0x100;
    assert_eq!(
        plan(&mut slot(&with_segment_addr(2, near_end)), AT, END),
        Err(Refused::Range(near_end))
    );
}

#[test]
fn overlapping_ram_segments_are_refused() {
    // The IRAM segment moved onto the DRAM one's addresses (same bus).
    let r = plan(&mut slot(&with_segment_addr(2, 0x3FC8_A800)), AT, END);
    assert_eq!(r, Err(Refused::Range(0x3FC8_A800)));
    assert!(DRAM.0 < 0x3FC8_A800);
}

#[test]
fn a_mapped_segment_off_its_page_offset_is_refused() {
    let r = plan(&mut slot(&with_segment_addr(4, 0x4201_0040)), AT, END);
    assert_eq!(r, Err(Refused::Unaligned(0x4201_0040)));
}

#[test]
fn mapped_segments_on_one_mmu_entry_are_refused() {
    // IROM moved to page 0, the entry the DROM segment takes (one table for
    // both buses).
    let r = plan(&mut slot(&with_segment_addr(4, 0x4200_0020)), AT, END);
    assert_eq!(r, Err(Refused::Range(0x4200_0020)));
}

#[test]
fn an_entry_outside_the_loaded_code_is_refused() {
    for entry in [0x3FC8_A700, 0x4037_0000, 0x4300_0000, 0] {
        let mut b = APP_HEAD.to_vec();
        put32(&mut b, 4, entry);
        assert_eq!(
            plan(&mut slot(&b), AT, END),
            Err(Refused::Entry),
            "{entry:#x}"
        );
    }
    // Inside the IROM map is code too.
    let mut b = APP_HEAD.to_vec();
    put32(&mut b, 4, 0x4201_0100);
    assert!(plan(&mut slot(&b), AT, END).is_ok());
}

#[test]
fn a_segment_leaving_the_slot_is_refused() {
    // The slot ends inside the last segment's data.
    assert_eq!(
        plan(&mut slot(APP_HEAD), AT, AT + 0x2_0000),
        Err(Refused::Bounds)
    );
    // A length that wraps.
    let mut b = APP_HEAD.to_vec();
    put32(&mut b, 24 + 4, u32::MAX - 8);
    assert_eq!(plan(&mut slot(&b), AT, END), Err(Refused::Bounds));
}

#[test]
fn not_an_esp32s3_image_is_refused() {
    let mut b = APP_HEAD.to_vec();
    b[0] = !MAGIC;
    assert_eq!(plan(&mut slot(&b), AT, END), Err(Refused::Magic));
    let mut b = APP_HEAD.to_vec();
    b[12] = 2;
    assert_eq!(plan(&mut slot(&b), AT, END), Err(Refused::Chip));
    for n in [0u8, 17] {
        let mut b = APP_HEAD.to_vec();
        b[1] = n;
        assert_eq!(plan(&mut slot(&b), AT, END), Err(Refused::Segments));
    }
    // An erased slot.
    assert_eq!(plan(&mut slot(&[]), AT, END), Err(Refused::Magic));
    // A read past the flash.
    let mut short = Mem {
        base: AT,
        bytes: APP_HEAD[..100].to_vec(),
    };
    assert_eq!(plan(&mut short, AT, END), Err(Refused::Read));
}
