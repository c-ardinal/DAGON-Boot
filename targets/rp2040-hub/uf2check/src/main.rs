//! Reads a hub install UF2 (`dagon rescue hub`) the way the hub will boot
//! it: stage0's choice and check of the boot core, then the boot core's
//! dagon-core decision for the app. Exits non-zero if either would not start.
//!
//! uf2check <hub.uf2> <dagon-signing.pub>

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::{env, fs};

use dagon_core::boot::{self, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_format::header::{image_kind, mcu_id, target_tag};
use dagon_format::statelog::slot;
use dagon_port::{Flash, FlashError, FlashRegion};
use rp2040_common::layout::*;
use rp2040_common::{bl, image};

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
    let [uf2, pubkey] = &args[..] else {
        return Err("usage: uf2check <hub.uf2> <dagon-signing.pub>".into());
    };
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
        target_tag: target_tag(HUB_TAG),
        max_attempts: MAX_ATTEMPTS,
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

/// Flash contents (offsets from the start of flash) the UF2 writes.
struct Uf2Flash(BTreeMap<u32, u8>);

fn load_uf2(path: &str) -> Result<Uf2Flash, String> {
    let raw = fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let mut bytes = BTreeMap::new();
    for b in raw.chunks_exact(512) {
        let w = |i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        if w(0) != UF2_MAGIC0 || w(1) != UF2_MAGIC1 || w(127) != UF2_MAGIC_END {
            return Err(format!("{path}: not a UF2 file"));
        }
        let (addr, len) = (w(3), (w(4) as usize).min(476));
        for (i, &x) in b[32..32 + len].iter().enumerate() {
            bytes.insert(addr + i as u32, x);
        }
    }
    Ok(Uf2Flash(bytes))
}

impl Flash for Uf2Flash {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = *self.0.get(&(addr + i as u32)).unwrap_or(&0xFF);
        }
        Ok(())
    }
    fn erase(&mut self, _: u32, _: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        for (i, &b) in data.iter().enumerate() {
            self.0.insert(addr + i as u32, b);
        }
        Ok(())
    }
}
