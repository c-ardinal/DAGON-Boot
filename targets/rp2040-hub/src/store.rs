//! Receiver side of the hub itself: APP-A/B slots, the app state log and the
//! topology display copy.

use dagon_agent::{SlotStore, StoreError};
use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{kind, target as log_target};
use dagon_port::Flash;
use dagon_proto::payload::{target, StatusResp};
use rp2040_common::layout::{
    body_capacity, sectors, APP_LOG, APP_SLOT, APP_SLOT_SIZE, HEADER_LEN, SECTOR, TOPOLOGY,
    TOPOLOGY_SIZE,
};
use rp2040_common::rp::RpFlash;
use rp2040_common::{image, log, status};

use crate::wdt;

/// Slot this build is linked for.
pub const ACTIVE: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };

pub struct HubStore {
    pub flash: RpFlash,
}

/// Where a target's region starts, where its body starts, body capacity.
fn region(t: u8, slot: u8) -> Result<(u32, u32, u32), StoreError> {
    match (t, slot) {
        (target::APP, 0 | 1) => {
            let base = APP_SLOT[slot as usize];
            Ok((base, base + HEADER_LEN, body_capacity(APP_SLOT_SIZE)))
        }
        (target::TOPOLOGY, 0) => Ok((TOPOLOGY, TOPOLOGY, TOPOLOGY_SIZE)),
        _ => Err(StoreError::Unsupported),
    }
}

fn body_at(t: u8, slot: u8, offset: u32, len: usize) -> Result<u32, StoreError> {
    let (_, body, cap) = region(t, slot)?;
    match offset.checked_add(len as u32) {
        Some(end) if end <= cap => Ok(body + offset),
        _ => Err(StoreError::Range),
    }
}

fn flash_err(_: dagon_port::FlashError) -> StoreError {
    StoreError::Flash
}

impl HubStore {
    fn append(&mut self, k: u8, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let mark = image::mark_of(header, slot).ok_or(StoreError::Flash)?;
        log::append(&mut self.flash, APP_LOG, k, log_target::APP, mark, 0).map_err(flash_err)
    }
}

impl SlotStore for HubStore {
    fn status(&mut self) -> StatusResp {
        let (fold, cur) = log::read(&mut self.flash, APP_LOG, log_target::APP).unwrap_or_default();
        let heads = [0u8, 1].map(|s| image::head_mark(&mut self.flash, APP_SLOT[s as usize], s));
        status::status(&fold, heads, ACTIVE, cur.map_or(0, |c| c.generation))
    }

    fn capacity(&self, t: u8, slot: u8) -> u32 {
        region(t, slot).map_or(0, |(_, _, cap)| cap)
    }

    fn erase(&mut self, t: u8, slot: u8, len: u32) -> Result<(), StoreError> {
        let (base, body, cap) = region(t, slot)?;
        if len > cap {
            return Err(StoreError::Range);
        }
        let end = base + sectors(body - base + len);
        // Sector by sector so interrupts run and the watchdog is fed between.
        let mut at = base;
        while at < end {
            self.flash.erase(at, SECTOR).map_err(flash_err)?;
            wdt::keepalive();
            at += SECTOR;
        }
        Ok(())
    }

    fn write(&mut self, t: u8, slot: u8, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        let at = body_at(t, slot, offset, data.len())?;
        self.flash.write(at, data).map_err(flash_err)
    }

    fn read(&mut self, t: u8, slot: u8, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let at = body_at(t, slot, offset, buf.len())?;
        self.flash.read(at, buf).map_err(flash_err)
    }

    fn write_header(&mut self, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let (base, _, _) = region(target::APP, slot)?;
        self.flash.write(base, header).map_err(flash_err)
    }

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let (base, _, _) = region(target::APP, slot)?;
        self.flash.read(base, out).map_err(flash_err)
    }

    fn set_pending(&mut self, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.append(kind::SLOT_PENDING, slot, header)
    }

    fn confirm(&mut self, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.append(kind::CONFIRMED, slot, header)?;
        wdt::set_confirmed();
        Ok(())
    }
}
