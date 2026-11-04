//! Reads a hub install UF2 (`dagon rescue hub`) the way the hub will boot
//! it: stage0's choice and check of the boot core, then the boot core's
//! dagon-core decision for the app. Exits non-zero if either would not start.
//!
//! uf2check <hub.uf2> <dagon-signing.pub> <target tag>
//!
//! The target tag is the one the boot core was built with
//! (`DAGON_TARGET_TAG`, e.g. `hub-rp2040-lcd` for the demo hub).

use std::process::ExitCode;
use std::{env, fs};

use dagon_core::boot::{self, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_format::header::{image_kind, mcu_id, target_tag};
use dagon_format::statelog::slot;
use dagon_mcu_rp2040::layout::*;
use dagon_mcu_rp2040::mapped::Mapped;
use dagon_mcu_rp2040::{bl, image};

const UF2_MAGIC0: u32 = 0x0A32_4655;
const UF2_MAGIC1: u32 = 0x9E5D_5157;
const UF2_MAGIC_END: u32 = 0x0AB1_6F30;

fn main() -> ExitCode {
    match run() {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("uf2check: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let [uf2, pubkey, tag] = &args[..] else {
        return Err("usage: uf2check <hub.uf2> <dagon-signing.pub> <target tag>".into());
    };
    if tag.is_empty() || tag.len() > 16 {
        return Err("target tag: 1 to 16 characters".into());
    }
    let mut flash = load_uf2(uf2)?;
    let pubkey = load_pubkey(pubkey)?;

    let named = bl::confirmed(&mut flash, BL_LOG).filter(|m| m.slot <= slot::B);
    let s = named.map_or(slot::A, |m| m.slot);
    image::check(
        &mut flash,
        BL_SLOT[s as usize],
        BL_SLOT_SIZE,
        s,
        named.as_ref(),
        image_kind::BOOTCORE,
    )
    .map_err(|r| format!("stage0 would not start boot core slot {s} ({r:?})"))?;

    let mut log = Log::load(&mut flash, &APP_LOG_LAYOUT).map_err(|e| format!("app log: {e:?}"))?;
    let cfg = BootConfig {
        pubkey,
        mcu_id: mcu_id::RP2040,
        target_tag: target_tag(tag),
        max_attempts: dagon_mcu_rp2040::TRIAL_ATTEMPTS,
    };
    match boot::decide(&mut flash, &mut log, &APP_SLOTS, &cfg) {
        Decision::Boot { slot, trial, .. } => Ok(format!(
            "ok: boot core slot {s}, app slot {slot} (trial {trial}), log generation {}",
            log.generation
        )),
        d => Err(format!("boot core would not start the app: {d:?}")),
    }
}

fn load_pubkey(path: &str) -> Result<[u8; 32], String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let hex = text.trim();
    let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    bytes
        .and_then(|b| b.try_into().ok())
        .ok_or(format!("{path}: expected 64 hex digits"))
}

/// The bootrom erases a 4 KiB sector when a block starts it, so every
/// sector a UF2 touches must be 16 consecutive 256-byte blocks, starting at
/// a block index that is a multiple of 16 at the sector start; otherwise a
/// later partial write leaves or erases the wrong bytes.
fn whole_sectors(addrs: &[u32]) -> Result<(), String> {
    const PAGES: usize = (SECTOR / 256) as usize;
    if !addrs.len().is_multiple_of(PAGES) {
        return Err(format!("{} blocks is not whole sectors", addrs.len()));
    }
    for (i, run) in addrs.chunks(PAGES).enumerate() {
        let start = run[0];
        let whole = start % SECTOR == 0
            && run
                .iter()
                .enumerate()
                .all(|(k, &a)| a == start + 256 * k as u32);
        if !whole {
            return Err(format!(
                "block {} ({start:#x}) does not start a whole sector",
                i * PAGES
            ));
        }
    }
    Ok(())
}

/// The flash a UF2 leaves (erased where it writes nothing), read through
/// the same mapped reader stage0 uses on the board.
fn load_uf2(path: &str) -> Result<Mapped, String> {
    let raw = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let mem = flash_image(&raw).map_err(|e| format!("{path}: {e}"))?;
    Ok(Mapped::over(Box::leak(mem.into_boxed_slice()), XIP_BASE))
}

/// Checks a UF2 file block by block (magic, blockNo = index, numBlocks =
/// block count, 256-byte payloads in flash, whole sectors, nothing after
/// the last block) and returns the flash it leaves.
fn flash_image(raw: &[u8]) -> Result<Vec<u8>, String> {
    if raw.is_empty() || !raw.len().is_multiple_of(512) {
        return Err(format!("{} bytes is not whole 512-byte blocks", raw.len()));
    }
    let total = (raw.len() / 512) as u32;
    let mut mem = vec![0xFFu8; MIN_FLASH_SIZE];
    let mut addrs = Vec::new();
    for (i, b) in raw.chunks_exact(512).enumerate() {
        let w = |i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        if w(0) != UF2_MAGIC0 || w(1) != UF2_MAGIC1 || w(127) != UF2_MAGIC_END {
            return Err("not a UF2 file".into());
        }
        let (addr, len) = (w(3), w(4) as usize);
        if w(5) != i as u32 || w(6) != total {
            return Err(format!(
                "block {i} says it is block {} of {} (file has {total})",
                w(5),
                w(6)
            ));
        }
        if len != 256 {
            return Err(format!("block at {addr:#x} carries {len} bytes, not 256"));
        }
        let off = addr
            .checked_sub(XIP_BASE)
            .map(|o| o as usize)
            .filter(|o| o + len <= MIN_FLASH_SIZE)
            .ok_or(format!("block at {addr:#x} outside flash"))?;
        mem[off..off + len].copy_from_slice(&b[32..32 + len]);
        addrs.push(addr);
    }
    whole_sectors(&addrs)?;
    Ok(mem)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A UF2 of `pages` consecutive pages from `start`, numbered as given.
    fn uf2(start: u32, pages: u32, number: impl Fn(u32) -> (u32, u32)) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..pages {
            let (no, total) = number(i);
            let mut b = [0u8; 512];
            let words = [
                UF2_MAGIC0,
                UF2_MAGIC1,
                0x2000,
                start + 256 * i,
                256,
                no,
                total,
            ];
            for (k, v) in words.iter().enumerate() {
                b[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            b[508..].copy_from_slice(&UF2_MAGIC_END.to_le_bytes());
            out.extend_from_slice(&b);
        }
        out
    }

    #[test]
    fn whole_sector_in_order_passes() {
        assert!(flash_image(&uf2(XIP_BASE, 16, |i| (i, 16))).is_ok());
    }

    #[test]
    fn block_numbers_must_match_their_place() {
        // Right addresses, block numbers shifted by one.
        assert!(flash_image(&uf2(XIP_BASE, 16, |i| (i + 1, 16))).is_err());
        // numBlocks not the file's block count.
        assert!(flash_image(&uf2(XIP_BASE, 16, |i| (i, 17))).is_err());
    }

    #[test]
    fn partial_sector_and_trailing_bytes_fail() {
        assert!(flash_image(&uf2(XIP_BASE, 15, |i| (i, 15))).is_err());
        assert!(flash_image(&uf2(XIP_BASE + 256, 16, |i| (i, 16))).is_err());
        let mut raw = uf2(XIP_BASE, 16, |i| (i, 16));
        raw.extend_from_slice(b"junk");
        assert!(flash_image(&raw).is_err());
    }
}
