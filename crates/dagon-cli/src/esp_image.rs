//! Builds and checks the ROM-loadable image of an ESP chip (the format the
//! ROM reads at flash offset 0: header, extended header, segments, checksum
//! byte, SHA-256) from an ELF. The image is signed and written exactly as
//! built; the flasher must not re-pack it (`espflash` rewrites header byte 3
//! and the trailing SHA-256 of an image it saves or writes).
//!
//! Format values come from esptool's `bin_image.py`, `cmds.py` and the
//! `targets/<chip>.py` memory map; per-chip values live in [`CHIPS`].

#![cfg_attr(not(test), allow(dead_code))]

use dagon_format::sha256;

const MAGIC: u8 = 0xE9;
const CHECKSUM_SEED: u8 = 0xEF;
const WP_PIN_DISABLED: u8 = 0xEE;
const HEADER_LEN: usize = 24;
const SEGMENT_HEADER_LEN: usize = 8;
/// The ROM loader takes at most this many segments.
const MAX_SEGMENTS: usize = 16;
const EM_XTENSA: u16 = 94;
const PT_LOAD: u32 = 1;

/// What the ROM of one chip expects of an image.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ChipImage {
    pub name: &'static str,
    pub chip_id: u16,
    /// ELF `e_machine` of the chip's cores.
    pub elf_machine: u16,
    /// Highest chip revision (major * 100 + minor) the image accepts.
    pub max_rev_full: u16,
    /// Header byte 2: SPI flash read mode (2 = DIO).
    pub flash_mode: u8,
    /// Low nibble of header byte 3: SPI clock code (0xF = 80 MHz).
    pub flash_freq: u8,
    /// Half-open load ranges the ROM can write segments into.
    pub load_ranges: &'static [(u32, u32)],
}

pub(crate) const CHIPS: &[ChipImage] = &[ChipImage {
    name: "esp32s3",
    chip_id: 9,
    elf_machine: EM_XTENSA,
    max_rev_full: 99,
    flash_mode: 2,
    flash_freq: 0xF,
    // esptool targets/esp32s3.py MEMORY_MAP: DRAM, IRAM.
    load_ranges: &[(0x3FC8_8000, 0x3FD0_0000), (0x4037_0000, 0x403E_0000)],
}];

pub(crate) fn chip(name: &str) -> Result<&'static ChipImage, String> {
    CHIPS
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| format!("no ROM image format known for chip `{name}`"))
}

/// The ROM-loadable image of `elf` for `chip` with `flash_size` (a
/// definition's `flash_size`). Refuses a segment outside the chip's load
/// ranges (a load address of 0 is the flasher's PADDING, not memory), an
/// unaligned or overlapping segment, and an entry outside the segments.
pub(crate) fn build_image(
    elf: &[u8],
    chip: &ChipImage,
    flash_size: &str,
) -> Result<Vec<u8>, String> {
    let size_freq = size_freq_byte(chip, flash_size)?;
    let (entry, segs) = load_segments(elf, chip)?;
    if segs.is_empty() || segs.len() > MAX_SEGMENTS {
        return Err(format!(
            "{} loadable segments (1..={MAX_SEGMENTS} allowed)",
            segs.len()
        ));
    }
    let mut out = vec![MAGIC, segs.len() as u8, chip.flash_mode, size_freq];
    out.extend(entry.to_le_bytes());
    out.extend([WP_PIN_DISABLED, 0, 0, 0]);
    out.extend(chip.chip_id.to_le_bytes());
    out.push(0); // min_rev
    out.extend(0u16.to_le_bytes()); // min_rev_full
    out.extend(chip.max_rev_full.to_le_bytes());
    out.extend([0u8; 4]);
    out.push(1); // SHA-256 appended
    debug_assert_eq!(out.len(), HEADER_LEN);
    let (mut checksum, mut prev_end, mut entry_ok) = (CHECKSUM_SEED, 0u32, false);
    for (addr, data) in segs {
        let len = (data.len() + 3) & !3;
        if addr % 4 != 0 || !in_ranges(chip, addr, len as u32) {
            return Err(format!(
                "segment {addr:#x}+{len:#x} is outside the load ranges of {}",
                chip.name
            ));
        }
        if addr < prev_end {
            return Err(format!("segment {addr:#x} overlaps the one before it"));
        }
        prev_end = addr + len as u32;
        entry_ok |= (addr..prev_end).contains(&entry);
        out.extend(addr.to_le_bytes());
        out.extend((len as u32).to_le_bytes());
        out.extend(data);
        out.resize(out.len() + len - data.len(), 0);
        checksum = data.iter().fold(checksum, |c, b| c ^ b);
    }
    if !entry_ok {
        return Err(format!("entry {entry:#x} is not inside a loaded segment"));
    }
    out.resize(out.len() + (15 - out.len() % 16), 0);
    out.push(checksum);
    let digest = sha256(&out);
    out.extend(digest);
    Ok(out)
}

/// Header byte 3 high nibble for a flash size written as in a definition
/// (`16mb`): esptool's `FLASH_SIZES` code divided by 16.
fn flash_size_code(size: &str) -> Result<u8, String> {
    const SIZES: [&str; 6] = ["1mb", "2mb", "4mb", "8mb", "16mb", "32mb"];
    SIZES
        .iter()
        .position(|s| s.eq_ignore_ascii_case(size))
        .map(|i| i as u8)
        .ok_or_else(|| format!("flash size `{size}` is not one of {}", SIZES.join(", ")))
}

fn size_freq_byte(chip: &ChipImage, flash_size: &str) -> Result<u8, String> {
    Ok(flash_size_code(flash_size)? << 4 | chip.flash_freq)
}

fn word(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn half(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn in_ranges(chip: &ChipImage, addr: u32, len: u32) -> bool {
    chip.load_ranges
        .iter()
        .any(|&(lo, hi)| addr >= lo && addr.checked_add(len).is_some_and(|end| end <= hi))
}

/// A load address and the bytes loaded there.
type Loadable<'a> = (u32, &'a [u8]);

/// The entry point and the loadable bytes of `elf` as `(load address,
/// bytes)`, by address. Segments without file bytes (bss) are not part of
/// the image: the ROM does not zero memory, the program does.
fn load_segments<'a>(elf: &'a [u8], chip: &ChipImage) -> Result<(u32, Vec<Loadable<'a>>), String> {
    let cut = || "ELF is truncated".to_string();
    if elf.get(..4) != Some(b"\x7fELF") || elf.get(4) != Some(&1) || elf.get(5) != Some(&1) {
        return Err("not a 32-bit little-endian ELF".into());
    }
    if half(elf, 0x12).ok_or_else(cut)? != chip.elf_machine {
        return Err(format!("ELF machine is not that of {}", chip.name));
    }
    let entry = word(elf, 0x18).ok_or_else(cut)?;
    let phoff = word(elf, 0x1C).ok_or_else(cut)? as usize;
    let phentsize = half(elf, 0x2A).ok_or_else(cut)? as usize;
    let phnum = half(elf, 0x2C).ok_or_else(cut)? as usize;
    let mut segs = Vec::new();
    for i in 0..phnum {
        let p = phoff + i * phentsize;
        let field = |n: usize| word(elf, p + 4 * n).ok_or_else(cut);
        let (kind, off, paddr, filesz) = (field(0)?, field(1)?, field(3)?, field(4)?);
        if kind != PT_LOAD || filesz == 0 {
            continue;
        }
        let data = elf
            .get(off as usize..(off as usize).saturating_add(filesz as usize))
            .ok_or_else(cut)?;
        segs.push((paddr, data));
    }
    segs.sort_by_key(|s| s.0);
    Ok((entry, segs))
}

/// What [`check_image`] read from an image it accepted.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ImageInfo {
    pub entry: u32,
    /// `(load address, length)` per segment, in image order.
    pub segments: Vec<(u32, u32)>,
}

/// Parses `img` as the ROM would and reports every way it is not an image
/// of `chip` with `flash_size`, or the segments it holds.
pub(crate) fn check_image(
    img: &[u8],
    chip: &ChipImage,
    flash_size: &str,
) -> Result<ImageInfo, String> {
    let want_size_freq = size_freq_byte(chip, flash_size)?;
    if img.len() < HEADER_LEN {
        return Err("image is shorter than its header".into());
    }
    let mut errs: Vec<String> = Vec::new();
    let (count, entry) = (img[1] as usize, word(img, 4).unwrap_or(0));
    if img[0] != MAGIC {
        errs.push(format!("magic {:#x}", img[0]));
    }
    if count == 0 || count > MAX_SEGMENTS {
        errs.push(format!("{count} segments"));
    }
    if img[2] != chip.flash_mode {
        errs.push(format!("flash mode {}", img[2]));
    }
    if img[3] != want_size_freq {
        errs.push(format!(
            "size/frequency byte {:#x}, expected {want_size_freq:#x}",
            img[3]
        ));
    }
    let id = half(img, 12).unwrap_or(0);
    if id != chip.chip_id {
        errs.push(format!("chip id {id}"));
    }
    if half(img, 17) != Some(chip.max_rev_full) {
        errs.push("maximum chip revision".into());
    }
    if img[8..12] != [WP_PIN_DISABLED, 0, 0, 0] {
        errs.push("write-protect pin or SPI drive settings".into());
    }
    if img[14..17] != [0, 0, 0] {
        errs.push("minimum chip revision".into());
    }
    if img[19..23] != [0; 4] {
        errs.push("reserved header bytes".into());
    }
    if img[23] != 1 {
        errs.push("no SHA-256 appended".into());
    }
    let (mut at, mut checksum, mut segments) = (HEADER_LEN, CHECKSUM_SEED, Vec::new());
    for _ in 0..count.min(MAX_SEGMENTS) {
        let (Some(addr), Some(len)) = (word(img, at), word(img, at + 4)) else {
            errs.push("truncated segment header".into());
            break;
        };
        let body = at + SEGMENT_HEADER_LEN;
        let Some(data) = img.get(body..body.saturating_add(len as usize)) else {
            errs.push(format!("segment {addr:#x} runs past the image"));
            break;
        };
        if len % 4 != 0 || addr % 4 != 0 || !in_ranges(chip, addr, len) {
            errs.push(format!(
                "segment {addr:#x}+{len:#x} cannot be loaded by the ROM"
            ));
        }
        checksum = data.iter().fold(checksum, |c, b| c ^ b);
        segments.push((addr, len));
        at = body + len as usize;
    }
    if !segments
        .iter()
        .any(|&(a, l)| (a..a.saturating_add(l)).contains(&entry))
    {
        errs.push(format!("entry {entry:#x} not in a segment"));
    }
    let mut spans = segments.clone();
    spans.sort_unstable();
    if spans
        .windows(2)
        .any(|w| u64::from(w[0].0) + u64::from(w[0].1) > u64::from(w[1].0))
    {
        errs.push("segments overlap".into());
    }
    at += 15 - at % 16;
    match img.get(at) {
        Some(&c) if c == checksum => {}
        found => errs.push(format!("checksum {found:?}, expected {checksum:#x}")),
    }
    let digest_at = at + 1;
    let covered = &img[..digest_at.min(img.len())];
    if img.get(digest_at..digest_at + 32) != Some(&sha256(covered)[..]) {
        errs.push("SHA-256".into());
    }
    if img.len() != digest_at + 32 {
        errs.push(format!(
            "image length {}, expected {}",
            img.len(),
            digest_at + 32
        ));
    }
    if errs.is_empty() {
        Ok(ImageInfo { entry, segments })
    } else {
        Err(errs.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAM: u32 = 0x3FCE_5700;
    const IRAM: u32 = 0x403C_B700;

    /// A 32-bit little-endian Xtensa ELF with one PT_LOAD per
    /// `(paddr, bytes)`; `memsz_extra` is added to each segment's memsz.
    fn elf(entry: u32, segs: &[(u32, &[u8])], memsz_extra: u32) -> Vec<u8> {
        let mut e = vec![0x7f, b'E', b'L', b'F', 1, 1, 1, 0];
        e.resize(0x10, 0);
        e.extend(2u16.to_le_bytes());
        e.extend(94u16.to_le_bytes());
        e.extend(1u32.to_le_bytes());
        e.extend(entry.to_le_bytes());
        e.extend(52u32.to_le_bytes()); // phoff
        e.extend(0u32.to_le_bytes());
        e.extend(0u32.to_le_bytes());
        e.extend(52u16.to_le_bytes());
        e.extend(32u16.to_le_bytes());
        e.extend((segs.len() as u16).to_le_bytes());
        e.extend([0u8; 6]);
        let mut off = 52 + 32 * segs.len() as u32;
        for (addr, data) in segs {
            let len = data.len() as u32;
            for v in [1, off, *addr, *addr, len, len + memsz_extra, 5, 4] {
                e.extend(u32::to_le_bytes(v));
            }
            off += len;
        }
        for (_, data) in segs {
            e.extend(*data);
        }
        e
    }

    fn s3() -> &'static ChipImage {
        chip("esp32s3").unwrap()
    }

    fn two_segment_elf() -> Vec<u8> {
        elf(
            IRAM + 4,
            &[(DRAM, &[1, 2, 3, 4, 5, 6]), (IRAM, &[0xAA; 16])],
            0,
        )
    }

    #[test]
    fn header_carries_chip_flash_parameters_and_entry() {
        let img = build_image(&two_segment_elf(), s3(), "16mb").unwrap();
        assert_eq!(img[0], 0xE9);
        assert_eq!(img[1], 2);
        assert_eq!(img[2], 2, "DIO");
        assert_eq!(img[3], 0x4F, "16 MB, 80 MHz");
        assert_eq!(u32::from_le_bytes(img[4..8].try_into().unwrap()), IRAM + 4);
        assert_eq!(img[8], 0xEE, "write-protect pin disabled");
        assert_eq!(u16::from_le_bytes(img[12..14].try_into().unwrap()), 9);
        assert_eq!(u16::from_le_bytes(img[17..19].try_into().unwrap()), 99);
        assert_eq!(img[23], 1, "SHA-256 appended");
    }

    #[test]
    fn segments_are_sorted_and_padded_to_words() {
        let img = build_image(&two_segment_elf(), s3(), "16mb").unwrap();
        let info = check_image(&img, s3(), "16mb").unwrap();
        assert_eq!(info.entry, IRAM + 4);
        assert_eq!(info.segments, vec![(DRAM, 8), (IRAM, 16)]);
        assert_eq!(&img[32..40], &[1, 2, 3, 4, 5, 6, 0, 0]);
    }

    #[test]
    fn checksum_byte_and_sha256_cover_the_image() {
        let img = build_image(&two_segment_elf(), s3(), "16mb").unwrap();
        let (body, sha) = img.split_at(img.len() - 32);
        assert_eq!(sha, sha256(body));
        assert_eq!(body.len() % 16, 0, "checksum byte ends a 16-byte row");
        let want = [1u8, 2, 3, 4, 5, 6]
            .iter()
            .chain(&[0xAA; 16])
            .fold(0xEF, |c, b| c ^ b);
        assert_eq!(body[body.len() - 1], want);
    }

    #[test]
    fn size_and_frequency_follow_the_definition() {
        let img = build_image(&two_segment_elf(), s3(), "4mb").unwrap();
        assert_eq!(img[3], 0x2F);
        assert!(check_image(&img, s3(), "16mb")
            .unwrap_err()
            .contains("size/frequency"));
        assert!(build_image(&two_segment_elf(), s3(), "3mb").is_err());
    }

    #[test]
    fn bss_only_segments_are_not_in_the_image() {
        let e = elf(IRAM, &[(IRAM, &[7; 4]), (DRAM, &[])], 0x100);
        let info = check_image(&build_image(&e, s3(), "16mb").unwrap(), s3(), "16mb").unwrap();
        assert_eq!(info.segments, vec![(IRAM, 4)]);
    }

    #[test]
    fn refuses_segments_outside_the_load_ranges() {
        for addr in [
            0u32,
            0x3C00_0020,
            0x4200_0020,
            0x3FC8_7FFC,
            0x3FCF_FFFC,
            0x403D_FFFC,
        ] {
            let e = elf(addr, &[(addr, &[0; 8])], 0);
            let r = build_image(&e, s3(), "16mb");
            assert!(r.unwrap_err().contains("outside"), "{addr:#x}");
        }
    }

    #[test]
    fn refuses_a_padding_segment_between_loadable_ones() {
        let e = elf(IRAM, &[(0, &[0; 32]), (IRAM, &[1; 4])], 0);
        assert!(build_image(&e, s3(), "16mb")
            .unwrap_err()
            .contains("outside"));
    }

    #[test]
    fn refuses_unaligned_overlapping_and_entryless_input() {
        let unaligned = elf(IRAM + 2, &[(IRAM + 2, &[0; 4])], 0);
        assert!(build_image(&unaligned, s3(), "16mb").is_err());
        let overlap = elf(IRAM, &[(IRAM, &[0; 8]), (IRAM + 4, &[0; 8])], 0);
        assert!(build_image(&overlap, s3(), "16mb")
            .unwrap_err()
            .contains("overlaps"));
        let no_entry = elf(DRAM, &[(IRAM, &[0; 8])], 0);
        assert!(build_image(&no_entry, s3(), "16mb")
            .unwrap_err()
            .contains("entry"));
        let nothing = elf(IRAM, &[], 0);
        assert!(build_image(&nothing, s3(), "16mb").is_err());
    }

    #[test]
    fn refuses_what_is_not_a_chip_elf() {
        assert!(build_image(b"not an elf", s3(), "16mb").is_err());
        let mut arm = two_segment_elf();
        arm[0x12] = 40;
        assert!(build_image(&arm, s3(), "16mb")
            .unwrap_err()
            .contains("machine"));
        let mut be = two_segment_elf();
        be[5] = 2;
        assert!(build_image(&be, s3(), "16mb").is_err());
        assert!(chip("esp32").is_err());
    }

    #[test]
    fn check_names_each_damaged_field() {
        let good = build_image(&two_segment_elf(), s3(), "16mb").unwrap();
        let hit = |at: usize, want: &str| {
            let mut img = good.clone();
            img[at] ^= 1;
            let e = check_image(&img, s3(), "16mb").unwrap_err();
            assert!(e.contains(want), "byte {at}: {e}");
        };
        hit(0, "magic");
        hit(2, "flash mode");
        hit(12, "chip id");
        hit(23, "SHA-256 appended");
        hit(32, "checksum");
        hit(good.len() - 1, "SHA-256");
        let mut longer = good.clone();
        longer.push(0);
        assert!(check_image(&longer, s3(), "16mb")
            .unwrap_err()
            .contains("length"));
        assert!(check_image(&good[..10], s3(), "16mb").is_err());
    }

    /// Changes `img` and recomputes checksum and SHA-256, as a hand-made
    /// image would carry them, so only the changed field can be at fault.
    fn reseal(mut img: Vec<u8>, change: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        img.truncate(img.len() - 32);
        change(&mut img);
        let digest = sha256(&img);
        img.extend(digest);
        img
    }

    #[test]
    fn check_refuses_every_header_field_the_rom_reads_when_resealed() {
        let good = build_image(&two_segment_elf(), s3(), "16mb").unwrap();
        for (at, want) in [
            (8, "write-protect"),
            (9, "write-protect"),
            (10, "write-protect"),
            (11, "write-protect"),
            (14, "minimum chip revision"),
            (15, "minimum chip revision"),
            (16, "minimum chip revision"),
            (17, "maximum chip revision"),
            (18, "maximum chip revision"),
            (19, "reserved"),
            (22, "reserved"),
        ] {
            let img = reseal(good.clone(), |i| i[at] ^= 0x10);
            let e = check_image(&img, s3(), "16mb").unwrap_err();
            assert!(e.contains(want), "byte {at}: {e}");
        }
    }

    #[test]
    fn check_refuses_overlapping_segments_when_resealed() {
        // Second segment moved onto the first; sizes and checksum unchanged.
        let e = elf(IRAM, &[(IRAM, &[1; 16]), (IRAM + 0x100, &[2; 16])], 0);
        let good = build_image(&e, s3(), "16mb").unwrap();
        let second = HEADER_LEN + SEGMENT_HEADER_LEN + 16;
        let img = reseal(good, |i| {
            i[second..second + 4].copy_from_slice(&(IRAM + 8).to_le_bytes())
        });
        let e = check_image(&img, s3(), "16mb").unwrap_err();
        assert!(e.contains("overlap"), "{e}");
    }

    #[test]
    fn check_refuses_an_image_the_flasher_packed() {
        // The shape espflash makes of an app: a segment at load address 0.
        let mut img = build_image(&two_segment_elf(), s3(), "16mb").unwrap();
        img[24..28].copy_from_slice(&0u32.to_le_bytes());
        let e = check_image(&img, s3(), "16mb").unwrap_err();
        assert!(e.contains("cannot be loaded"), "{e}");
    }
}
