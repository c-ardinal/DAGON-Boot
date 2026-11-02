//! Builds the hub's initial-install UF2 from the three firmware UF2s
//! (elf2uf2-rs output): stage0 with boot2, the boot core in BL-A and the hub
//! app in APP-A, each slot with a signed inline header, plus boot core and
//! app state logs (face A, generation 1) holding PROVISION + CONFIRMED for
//! slot A. The B faces, APP-B's header sector and the topology copy are
//! written erased so old flash content cannot linger.
//!
//! pack --stage0 S.uf2 --bootcore B.uf2 --hub H.uf2 --key dagon-signing.key
//!      [--version 1.0.0] --out hub-install.uf2

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::{env, fs};

use dagon_format::header::{
    self, build_flags, capabilities, image_kind, mcu_id, placement, port_kind, slot_binding,
    ImageHeader, SIGNED_LEN, SIGN_DOMAIN,
};
use dagon_format::statelog::{kind, target, Record};
use dagon_format::{key_id, sha256, Version};
use dagon_port::{Flash, FlashError, FlashRegion};
use ed25519_compact::{KeyPair, Seed};
use rp2040_common::image;
use rp2040_common::layout::*;
use rp2040_common::log::face_bytes;

const UF2_MAGIC0: u32 = 0x0A32_4655;
const UF2_MAGIC1: u32 = 0x9E5D_5157;
const UF2_MAGIC_END: u32 = 0x0AB1_6F30;
const UF2_FAMILY: u32 = 0x2000;
const RP2040_FAMILY: u32 = 0xE48B_FF56;
const PAGE: u32 = 256;
/// Must match the hub app's accepted `target_tag`.
const HUB_TAG: &str = "hub-rp2040-lcd";

type Pages = BTreeMap<u32, [u8; PAGE as usize]>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pack: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let need = |name: &str| arg(name).ok_or(format!("missing {name} <path>"));
    let version = parse_version(&arg("--version").unwrap_or("1.0.0".into()))?;
    let kp = load_key(&need("--key")?)?;

    let stage0 = read_uf2(&need("--stage0")?, XIP_BASE + STAGE0, BL_LOG[0] - STAGE0)?;
    let bl = read_uf2(
        &need("--bootcore")?,
        XIP_BASE + BL_SLOT[0] + HEADER_LEN,
        BL_SLOT_SIZE,
    )?;
    let app = read_uf2(
        &need("--hub")?,
        XIP_BASE + APP_SLOT[0] + HEADER_LEN,
        APP_SLOT_SIZE,
    )?;

    let mut flash = Pages::new();
    let erased = vec![0xFF; SECTOR as usize];
    for at in [BL_LOG[1], APP_LOG[1], APP_SLOT[1], TOPOLOGY] {
        put(&mut flash, at, &erased);
    }
    put(&mut flash, STAGE0, &stage0);
    let bl_hdr = sign(&kp, image_kind::BOOTCORE, BL_SLOT[0], &bl, version);
    put(&mut flash, BL_SLOT[0], &[&bl_hdr[..], &bl].concat());
    let app_hdr = sign(&kp, image_kind::APP, APP_SLOT[0], &app, version);
    put(&mut flash, APP_SLOT[0], &[&app_hdr[..], &app].concat());

    for (log, t, hdr) in [
        (BL_LOG[0], target::BOOTCORE, &bl_hdr),
        (APP_LOG[0], target::APP, &app_hdr),
    ] {
        let rec = |kind, seq| Record {
            kind,
            target: t,
            slot: 0,
            reason: 0,
            seq,
            version,
            digest16: header::header_digest16(hdr),
        };
        let mut face = vec![0u8; LOG_FACE_SIZE as usize];
        face_bytes(
            1,
            &[rec(kind::PROVISION, 1), rec(kind::CONFIRMED, 2)],
            &mut face,
        );
        put(&mut flash, log, &face);
    }

    self_check(&flash, &kp)?;
    let out = need("--out")?;
    fs::write(&out, write_uf2(&flash)).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: stage0 {} B, boot core {} B, hub app {} B, version {version}, key_id {}",
        stage0.len(),
        bl.len(),
        app.len(),
        key_id(&kp.pk)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    Ok(())
}

fn parse_version(s: &str) -> Result<Version, String> {
    let p: Vec<&str> = s.split('.').collect();
    let bad = || format!("bad --version {s} (want major.minor.patch)");
    if p.len() != 3 {
        return Err(bad());
    }
    Ok(Version::new(
        p[0].parse().map_err(|_| bad())?,
        p[1].parse().map_err(|_| bad())?,
        p[2].parse().map_err(|_| bad())?,
    ))
}

/// Secret key file from `dagon key gen`: the 32-byte seed in hex.
fn load_key(path: &str) -> Result<KeyPair, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let hex = text.trim();
    let seed: Option<Vec<u8>> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    let seed: [u8; 32] = seed
        .and_then(|s| s.try_into().ok())
        .ok_or(format!("{path}: expected 64 hex digits"))?;
    Ok(KeyPair::from_seed(Seed::new(seed)))
}

fn sign(kp: &KeyPair, kind: u8, slot_base: u32, body: &[u8], version: Version) -> [u8; 512] {
    let (flags, caps) = if kind == image_kind::BOOTCORE {
        (build_flags::VERIFY_ENFORCED, capabilities::SIG_VERIFIER)
    } else {
        (0, capabilities::CONFIRM_BY_HUB)
    };
    let mut h = ImageHeader {
        image_kind: kind,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::RP2040,
        slot_binding: slot_binding::A,
        placement: placement::INLINE,
        load_addr: XIP_BASE + slot_base + HEADER_LEN,
        payload_size: body.len() as u32,
        version,
        build_flags: flags,
        release_seq: 0,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: header::target_tag(HUB_TAG),
        proto_version: 1,
        capabilities: caps,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    h.to_bytes()
}

/// Bytes of a UF2 that must start at XIP address `start` and fit in `cap`.
fn read_uf2(path: &str, start: u32, cap: u32) -> Result<Vec<u8>, String> {
    let raw = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let mut bytes = BTreeMap::new();
    for b in raw.chunks_exact(512) {
        let w = |i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        if w(0) != UF2_MAGIC0 || w(1) != UF2_MAGIC1 || w(127) != UF2_MAGIC_END {
            return Err(format!("{path}: not a UF2 file"));
        }
        let (addr, len) = (w(3), w(4) as usize);
        for (i, &x) in b[32..32 + len].iter().enumerate() {
            let a = addr + i as u32;
            // elf2uf2-rs pads whole sectors: zero fill before the image start
            // is where the header goes.
            if a >= start || x != 0 {
                bytes.insert(a, x);
            }
        }
    }
    let (Some((&lo, _)), Some((&hi, _))) = (bytes.first_key_value(), bytes.last_key_value()) else {
        return Err(format!("{path}: empty"));
    };
    if lo != start || hi - start >= cap {
        return Err(format!(
            "{path}: covers {lo:#x}..={hi:#x}, expected to start at {start:#x} within {cap} B"
        ));
    }
    Ok((start..=hi).map(|a| *bytes.get(&a).unwrap_or(&0)).collect())
}

/// Places `data` at flash offset `at` (pages not covered keep 0xFF).
fn put(flash: &mut Pages, at: u32, data: &[u8]) {
    for (i, &b) in data.iter().enumerate() {
        let a = at + i as u32;
        flash
            .entry(a / PAGE * PAGE)
            .or_insert([0xFF; PAGE as usize])[(a % PAGE) as usize] = b;
    }
}

/// Reads the assembled image back the way stage0 and the boot core will:
/// both logs name slot A, both images pass their checks.
fn self_check(flash: &Pages, kp: &KeyPair) -> Result<(), String> {
    let mut f = PageFlash(flash);
    for (log, t, base, cap, kind) in [
        (
            BL_LOG,
            target::BOOTCORE,
            BL_SLOT[0],
            BL_SLOT_SIZE,
            image_kind::BOOTCORE,
        ),
        (
            APP_LOG,
            target::APP,
            APP_SLOT[0],
            APP_SLOT_SIZE,
            image_kind::APP,
        ),
    ] {
        let (fold, _) = rp2040_common::log::read(&mut f, log, t).map_err(|e| format!("{e:?}"))?;
        let c = fold.confirmed.ok_or("self-check: log names no slot")?;
        image::verify(&mut f, base, &kp.pk).map_err(|r| format!("self-check: reason {r}"))?;
        image::check(&mut f, base, cap, c.slot, Some(&c), kind)
            .map_err(|r| format!("self-check: reason {r}"))?;
    }
    Ok(())
}

struct PageFlash<'a>(&'a Pages);

impl Flash for PageFlash<'_> {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        for (i, b) in buf.iter_mut().enumerate() {
            let a = addr + i as u32;
            *b = self
                .0
                .get(&(a / PAGE * PAGE))
                .map_or(0xFF, |p| p[(a % PAGE) as usize]);
        }
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
    fn write(&mut self, _: u32, _: &[u8]) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
}

fn write_uf2(flash: &Pages) -> Vec<u8> {
    let n = flash.len() as u32;
    let mut out = Vec::with_capacity(flash.len() * 512);
    for (i, (&at, page)) in flash.iter().enumerate() {
        let mut b = [0u8; 512];
        let words = [
            UF2_MAGIC0,
            UF2_MAGIC1,
            UF2_FAMILY,
            XIP_BASE + at,
            PAGE,
            i as u32,
            n,
            RP2040_FAMILY,
        ];
        for (k, w) in words.iter().enumerate() {
            b[k * 4..k * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        b[32..32 + PAGE as usize].copy_from_slice(page);
        b[508..].copy_from_slice(&UF2_MAGIC_END.to_le_bytes());
        out.extend_from_slice(&b);
    }
    out
}
