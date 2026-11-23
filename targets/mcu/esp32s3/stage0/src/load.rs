//! Starting an ESP image: the plan `dagon_mcu_esp32s3::app_image::plan`
//! checked (segments inside the slot and the ranges an app may use, below
//! stage0's window), its RAM segments copied with 32-bit stores (IRAM takes
//! no byte stores), its IROM and DROM segments mapped through the MMU as
//! ESP-IDF's 2nd stage maps them (`bootloader_utility.c`
//! `set_cache_and_start_app`), then a jump to its entry point.

use crate::rom;
use dagon_core::boot::Place;
use dagon_mcu_esp32s3::app_image::{plan, Bus, Plan, Refused};
use dagon_port::Flash;

/// EXTMEM_DCACHE_CTRL1 and EXTMEM_ICACHE_CTRL1: bits 0 and 1 shut the
/// cache bus of core 0 and core 1 (ESP32-S3 PAC 0.36).
const DCACHE_CTRL1: u32 = 0x600C_4004;
const ICACHE_CTRL1: u32 = 0x600C_4064;
const SHUT_BUSES: u32 = 0b11;
/// The MMU page in KiB, as the ROM's `*_MMU_Set` take it.
const PAGE_KIB: u32 = 64;

/// The plan of the ESP image whose body is at `at`.
pub fn plan_at<F: Flash>(flash: &mut F, at: Place) -> Result<Plan, Refused> {
    plan(flash, at.body, at.body.saturating_add(at.room))
}

/// Copies and maps what `p` names, then runs its entry point.
pub fn start<F: Flash>(flash: &mut F, p: &Plan) -> Result<core::convert::Infallible, Refused> {
    let mut words = [0u32; 64];
    for c in p.copies() {
        let mut done = 0;
        while done < c.len {
            let n = (c.len - done).min(256);
            let bytes = &mut [0u8; 256][..n as usize];
            flash
                .read(c.from + done, bytes)
                .map_err(|_| Refused::Read)?;
            for (w, b) in words.iter_mut().zip(bytes.chunks(4)) {
                let mut le = [0u8; 4];
                le[..b.len()].copy_from_slice(b);
                *w = u32::from_le_bytes(le);
            }
            for (i, w) in words.iter().take(n.div_ceil(4) as usize).enumerate() {
                rom::reg_write(c.to + done + 4 * i as u32, *w);
            }
            done += n;
        }
    }
    map(p)?;
    // SAFETY: the entry point lies in code just copied or mapped (the
    // plan refuses any other), called as the ROM calls an image's entry.
    let entry: extern "C" fn() -> ! = unsafe { core::mem::transmute(p.entry as usize) };
    entry()
}

fn map(p: &Plan) -> Result<(), Refused> {
    // SAFETY: the ROM's cache and MMU functions in the order ESP-IDF's 2nd
    // stage calls them; nothing runs from the external flash meanwhile
    // (stage0 runs from internal SRAM).
    unsafe {
        let d = rom::rom_Cache_Suspend_DCache();
        let i = rom::rom_Cache_Suspend_ICache();
        rom::Cache_MMU_Init();
        for m in p.maps() {
            let (v, f, n) = m.pages();
            let set = match m.bus {
                Bus::Instruction => rom::Cache_Ibus_MMU_Set,
                Bus::Data => rom::Cache_Dbus_MMU_Set,
            };
            if set(0, v, f, PAGE_KIB, n, 0) != 0 {
                return Err(Refused::Range(m.vaddr));
            }
        }
        rom::reg_write(DCACHE_CTRL1, rom::reg_read(DCACHE_CTRL1) & !SHUT_BUSES);
        rom::reg_write(ICACHE_CTRL1, rom::reg_read(ICACHE_CTRL1) & !SHUT_BUSES);
        rom::Cache_Invalidate_ICache_All();
        rom::Cache_Invalidate_DCache_All();
        rom::Cache_Resume_ICache(i);
        rom::Cache_Resume_DCache(d);
    }
    Ok(())
}
