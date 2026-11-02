//! Host tool for boot-core bench tests on the NUCLEO-F401RE: turns an app ELF
//! into a signed APP image and writes a state log face, both for flashing
//! with probe-rs. Signs only with the RFC 8032 section 7.1 TEST 1 key (public
//! test data; images carry TEST_KEY), so the boot core must be built without
//! `DAGON_PUBKEY`. Release images come from `dagon image sign` instead.
//!
//! Usage:
//!   f401-testimg image <app.elf> <out.img> <a|b> <major.minor.patch>
//!   f401-testimg log <out.bin> <confirmed.img> [<pending.img>]

use std::process::ExitCode;

use dagon_format::header::*;
use dagon_format::statelog::{kind, reason, target, FaceHeader, Record};
use dagon_format::{key_id, sha256, Version};
use ed25519_compact::{KeyPair, Seed};

#[allow(dead_code)]
#[path = "../../mcu/stm32f401re/src/layout.rs"]
mod layout;

const TEST_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

const PT_LOAD: u32 = 1;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["image", elf, out, slot, version] => image(elf, out, slot, version),
        ["log", out, confirmed] => log(out, confirmed, None),
        ["log", out, confirmed, pending] => log(out, confirmed, Some(pending)),
        _ => Err(
            "usage: f401-testimg image <app.elf> <out.img> <a|b> <x.y.z>\n       \
                  f401-testimg log <out.bin> <confirmed.img> [<pending.img>]"
                .into(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type Res<T> = Result<T, String>;

fn image(elf: &str, out: &str, slot: &str, version: &str) -> Res<()> {
    let slot = match slot {
        "a" => slot_binding::A,
        "b" => slot_binding::B,
        _ => return Err(format!("slot must be a or b, got {slot}")),
    };
    let version = parse_version(version)?;
    let load_addr = layout::SLOTS.slots[slot as usize] + HEADER_SIZE as u32;
    let elf_bytes = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let body = flat_binary(&elf_bytes, load_addr)?;
    let max = layout::SLOTS.slot_len - HEADER_SIZE as u32;
    if body.len() as u32 > max {
        return Err(format!("body is {} bytes, slot holds {max}", body.len()));
    }
    let kp = KeyPair::from_seed(Seed::new(TEST_SEED));
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: layout::MCU_ID,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr,
        payload_size: body.len() as u32,
        version,
        build_flags: build_flags::TEST_KEY,
        release_seq: 0,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp.pk),
        target_tag: layout::TARGET_TAG,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    let img = [&h.to_bytes()[..], &body].concat();
    std::fs::write(out, img).map_err(|e| format!("{out}: {e}"))?;
    // Keep the matching ELF next to the image for RTT/defmt decoding.
    let elf_copy = format!("{out}.elf");
    std::fs::copy(elf, &elf_copy).map_err(|e| format!("{elf_copy}: {e}"))?;
    println!(
        "{out}: slot {} at {:#010x}, version {version}, body {} bytes",
        if slot == 0 { "A" } else { "B" },
        load_addr - HEADER_SIZE as u32,
        body.len()
    );
    Ok(())
}

fn parse_version(s: &str) -> Res<Version> {
    let bad = || format!("version must be major.minor.patch, got {s}");
    let mut it = s.split('.');
    let (Some(a), Some(b), Some(c), None) = (it.next(), it.next(), it.next(), it.next()) else {
        return Err(bad());
    };
    Ok(Version::new(
        a.parse().map_err(|_| bad())?,
        b.parse().map_err(|_| bad())?,
        c.parse().map_err(|_| bad())?,
    ))
}

/// Flattens the ELF32 little-endian PT_LOAD segments (by physical address)
/// into one binary starting at `base`; gaps are 0xFF like erased flash.
fn flat_binary(elf: &[u8], base: u32) -> Res<Vec<u8>> {
    let u16_at = |o: usize| elf.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        elf.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if elf.get(..6) != Some(&[0x7F, b'E', b'L', b'F', 1, 1][..]) {
        return Err("not a 32-bit little-endian ELF".into());
    }
    let truncated = || "truncated ELF".to_string();
    let phoff = u32_at(0x1C).ok_or_else(truncated)? as usize;
    let phentsize = u16_at(0x2A).ok_or_else(truncated)? as usize;
    let phnum = u16_at(0x2C).ok_or_else(truncated)? as usize;
    let mut out: Vec<u8> = Vec::new();
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        let field = |n: usize| u32_at(ph + 4 * n).ok_or_else(truncated);
        let (p_type, p_offset, p_paddr, p_filesz) = (field(0)?, field(1)?, field(3)?, field(4)?);
        if p_type != PT_LOAD || p_filesz == 0 {
            continue;
        }
        let at = p_paddr
            .checked_sub(base)
            .ok_or_else(|| format!("segment at {p_paddr:#010x} is below the slot body {base:#010x}; built for the other slot?"))?
            as usize;
        let data = elf
            .get(p_offset as usize..(p_offset + p_filesz) as usize)
            .ok_or_else(truncated)?;
        if out.len() < at + data.len() {
            out.resize(at + data.len(), 0xFF);
        }
        out[at..at + data.len()].copy_from_slice(data);
    }
    if out.is_empty() {
        return Err("no loadable segments".into());
    }
    Ok(out)
}

/// Face A with PROVISION + CONFIRMED for the confirmed image and, if given,
/// SLOT_PENDING for the pending image, followed by an erased face B (the file
/// spans both sectors, so flashing it also blanks a leftover face B whose
/// higher generation would otherwise win over the new face A).
fn log(out: &str, confirmed: &str, pending: Option<&str>) -> Res<()> {
    let mut face = FaceHeader { generation: 1 }.to_bytes().to_vec();
    let c = read_ref(confirmed)?;
    let mut recs = vec![(kind::PROVISION, c), (kind::CONFIRMED, c)];
    if let Some(p) = pending {
        let p = read_ref(p)?;
        if p.0 == c.0 {
            return Err("pending and confirmed images are bound to the same slot".into());
        }
        recs.push((kind::SLOT_PENDING, p));
    }
    for (seq, (k, (slot, version, digest16))) in recs.into_iter().enumerate() {
        let r = Record {
            kind: k,
            target: target::APP,
            slot,
            reason: reason::NONE,
            seq: seq as u32 + 1,
            version,
            digest16,
        };
        face.extend_from_slice(&r.to_bytes());
    }
    let [a, b] = layout::LOG.faces;
    face.resize((b - a + layout::LOG.face_len) as usize, 0xFF);
    std::fs::write(out, face).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: flash at {:#010x} (state log face A, face B erased)",
        layout::LOG.faces[0]
    );
    Ok(())
}

fn read_ref(path: &str) -> Res<(u8, Version, [u8; 16])> {
    let img = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let raw: &[u8; HEADER_SIZE] = img
        .get(..HEADER_SIZE)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| format!("{path}: shorter than a header"))?;
    let h = ImageHeader::parse(raw).map_err(|e| format!("{path}: {e:?}"))?;
    Ok((h.slot_binding, h.version, header_digest16(raw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parses() {
        assert_eq!(parse_version("1.2.3").unwrap(), Version::new(1, 2, 3));
        assert!(parse_version("1.2").is_err());
        assert!(parse_version("1.2.3.4").is_err());
    }

    #[test]
    fn flattens_two_segments_with_gap() {
        // Minimal ELF32 header + two PT_LOAD program headers.
        let mut elf = vec![0u8; 0x34 + 2 * 32];
        elf[..6].copy_from_slice(&[0x7F, b'E', b'L', b'F', 1, 1]);
        elf[0x1C..0x20].copy_from_slice(&0x34u32.to_le_bytes());
        elf[0x2A..0x2C].copy_from_slice(&32u16.to_le_bytes());
        elf[0x2C..0x2E].copy_from_slice(&2u16.to_le_bytes());
        let data_off = elf.len() as u32;
        elf.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        for (i, (off, paddr, len)) in [(data_off, 0x100u32, 4u32), (data_off + 4, 0x106, 2)]
            .into_iter()
            .enumerate()
        {
            let ph = 0x34 + 32 * i;
            for (n, v) in [(0, PT_LOAD), (1, off), (3, paddr), (4, len)] {
                elf[ph + 4 * n..ph + 4 * n + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        assert_eq!(
            flat_binary(&elf, 0x100).unwrap(),
            [1, 2, 3, 4, 0xFF, 0xFF, 5, 6]
        );
        assert!(flat_binary(&elf, 0x104).is_err());
    }
}
