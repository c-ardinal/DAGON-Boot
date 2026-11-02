//! The agent's receiver on the ESP32-S3 (kind V).
//!
//! - Bodies go to the OTA partition of the slot (ota_0 = A, ota_1 = B), from
//!   the partition start; only what the body covers is erased.
//! - The 512-byte header is detached: one 4 KiB sector per slot at the start
//!   of the dagon partition (+0x0000 A, +0x1000 B, +0x2000 factory).
//! - SET_PENDING writes an otadata trial entry; CONFIRM marks the running
//!   entry Valid. No DAGON state log is kept: STATUS maps otadata.

use crate::layout::{
    app_partition, Partition, DAGON, ERASE_UNIT, SLOT_A, SLOT_B, SLOT_FACTORY, WRITE_UNIT,
};
use crate::otadata::{selected_view, Counted, ENTRY_LEN};
use dagon_agent::{SlotStore, StoreError};
use dagon_format::header::{self, ImageHeader, HEADER_SIZE};
use dagon_port::{Flash, Watchdog};
use dagon_proto::payload::{target, SlotState, StatusResp};

/// One detached header sector per slot in the dagon partition.
pub const HEADER_SECTOR: u32 = 0x1000;
/// Erase step on 64 KiB aligned ranges (one block erase stays well inside
/// the trial watchdog).
const BLOCK: u32 = 0x1_0000;
const WORD: usize = WRITE_UNIT as usize;

/// otadata as the receiver needs it.
pub trait Otadata {
    /// Slot the 2nd stage booted: 0 = ota_0, 1 = ota_1, 2 = factory.
    fn running(&self) -> u8;
    /// Both raw entries (sector 0, sector 1).
    fn entries(&self) -> Result<[[u8; ENTRY_LEN]; 2], StoreError>;
    /// Selects OTA `slot` for one trial boot.
    fn set_next(&mut self, slot: u8) -> Result<(), StoreError>;
    /// Marks the running OTA slot Valid and reads it back.
    fn confirm(&mut self) -> Result<(), StoreError>;
}

/// Receive in progress: body bytes are programmed one write unit at a time,
/// a partial unit waits in `tail`.
#[derive(Clone, Copy, Debug)]
struct Rx {
    slot: u8,
    len: u32,
    flushed: u32,
    tail: [u8; WORD],
    tail_len: usize,
}

pub struct EspStore<F, O, W> {
    pub flash: F,
    pub ota: O,
    pub wdt: W,
    rx: Option<Rx>,
}

fn ota_slot(t: u8, slot: u8) -> Result<Partition, StoreError> {
    match (t, slot) {
        (target::APP, SLOT_A | SLOT_B) => app_partition(slot).ok_or(StoreError::Unsupported),
        _ => Err(StoreError::Unsupported),
    }
}

/// Start of the detached header sector of `slot` (A, B or factory).
pub fn header_addr(slot: u8) -> Result<u32, StoreError> {
    if slot > SLOT_FACTORY {
        return Err(StoreError::Range);
    }
    Ok(DAGON.offset + u32::from(slot) * HEADER_SECTOR)
}

fn flash_err<T>(r: Result<T, dagon_port::FlashError>) -> Result<T, StoreError> {
    r.map_err(|_| StoreError::Flash)
}

impl<F: Flash, O: Otadata, W: Watchdog> EspStore<F, O, W> {
    pub fn new(flash: F, ota: O, wdt: W) -> Self {
        Self {
            flash,
            ota,
            wdt,
            rx: None,
        }
    }

    /// Parsable header of `slot`, if any.
    fn header(&mut self, slot: u8) -> Option<[u8; HEADER_SIZE]> {
        let mut h = [0u8; HEADER_SIZE];
        self.read_header(slot, &mut h).ok()?;
        ImageHeader::parse(&h).ok().map(|_| h)
    }

    /// Erases `addr..addr + len` (erase-unit aligned), feeding in between.
    fn erase_range(&mut self, mut addr: u32, len: u32) -> Result<(), StoreError> {
        let end = addr + len;
        while addr < end {
            let step = if addr.is_multiple_of(BLOCK) && end - addr >= BLOCK {
                BLOCK
            } else {
                ERASE_UNIT
            };
            self.wdt.feed();
            flash_err(self.flash.erase(addr, step))?;
            addr += step;
        }
        self.wdt.feed();
        Ok(())
    }

    fn program(&mut self, addr: u32, data: &[u8]) -> Result<(), StoreError> {
        self.wdt.feed();
        flash_err(self.flash.write(addr, data))
    }
}

impl<F: Flash, O: Otadata, W: Watchdog> SlotStore for EspStore<F, O, W> {
    /// Slots with a header are Written; otadata then sets the selected
    /// slot (Pending, Trying, Reverted) and the running one (Trying or
    /// Confirmed). While on trial, `confirmed` is the slot the 2nd stage
    /// falls back to.
    fn status(&mut self) -> StatusResp {
        let running = self.ota.running();
        let mut st = StatusResp {
            active: running,
            ..Default::default()
        };
        let mut present = [false; 3];
        for s in [SLOT_A, SLOT_B, SLOT_FACTORY] {
            if let Some(h) = self.header(s) {
                let info = &mut st.slots[s as usize];
                info.state = SlotState::Written;
                info.version = ImageHeader::parse(&h)
                    .map(|h| h.version)
                    .unwrap_or_default();
                info.digest16 = header::header_digest16(&h);
                present[s as usize] = true;
            }
        }
        // Unreadable otadata reads as blank: the 2nd stage boots factory.
        let counted = self
            .ota
            .entries()
            .map(|e| [Counted::of(&e[0]), Counted::of(&e[1])])
            .unwrap_or([None, None]);
        let selected = counted.iter().flatten().max_by_key(|c| c.seq);
        let view = selected.and_then(|c| selected_view(running, c.slot, Some(c.state)));
        let mut trying = false;
        if let Some(v) = view {
            trying = v.slot == running && v.state == SlotState::Trying;
            // ponytail: otadata keeps no digest, so a slot re-received after a
            // rollback still shows Reverted until its next SET_PENDING.
            if v.slot != running && present[v.slot as usize] {
                st.slots[v.slot as usize].state = v.state;
            }
            if let Some(r) = v.reason {
                st.last_reason = r;
            }
        }
        if let Some(info) = st.slots.get_mut(running as usize) {
            info.state = if trying {
                SlotState::Trying
            } else {
                SlotState::Confirmed
            };
        }
        st.confirmed = if trying {
            counted
                .iter()
                .flatten()
                .filter(|c| c.bootable() && c.slot != running)
                .max_by_key(|c| c.seq)
                .map_or(SLOT_FACTORY, |c| c.slot)
        } else {
            running
        };
        st.attempts = u8::from(trying);
        st
    }

    fn capacity(&self, t: u8, slot: u8) -> u32 {
        ota_slot(t, slot).map_or(0, |p| p.size)
    }

    /// Erases the header sector first (the slot then has no image), then
    /// the body's erase units.
    fn erase(&mut self, t: u8, slot: u8, len: u32) -> Result<(), StoreError> {
        let p = ota_slot(t, slot)?;
        // Never wipe what is running, whatever the caller thinks.
        if slot == self.ota.running() {
            return Err(StoreError::Unsupported);
        }
        if len > p.size {
            return Err(StoreError::Range);
        }
        self.rx = None;
        self.erase_range(header_addr(slot)?, HEADER_SECTOR)?;
        self.erase_range(p.offset, len.div_ceil(ERASE_UNIT) * ERASE_UNIT)?;
        self.rx = Some(Rx {
            slot,
            len,
            flushed: 0,
            tail: [0xFF; WORD],
            tail_len: 0,
        });
        Ok(())
    }

    fn write(&mut self, t: u8, slot: u8, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        let p = ota_slot(t, slot)?;
        let mut rx = self
            .rx
            .filter(|r| r.slot == slot)
            .ok_or(StoreError::Range)?;
        let end = offset as u64 + data.len() as u64;
        if offset != rx.flushed + rx.tail_len as u32 || end > rx.len as u64 {
            return Err(StoreError::Range);
        }
        // Any failure below ends the receive.
        self.rx = None;
        let mut data = data;
        if rx.tail_len > 0 {
            let n = (WORD - rx.tail_len).min(data.len());
            rx.tail[rx.tail_len..rx.tail_len + n].copy_from_slice(&data[..n]);
            rx.tail_len += n;
            data = &data[n..];
            if rx.tail_len == WORD {
                self.program(p.offset + rx.flushed, &rx.tail)?;
                rx.flushed += WRITE_UNIT;
                rx.tail_len = 0;
            }
        }
        let whole = data.len() / WORD * WORD;
        if whole > 0 {
            self.program(p.offset + rx.flushed, &data[..whole])?;
            rx.flushed += whole as u32;
        }
        let rest = &data[whole..];
        rx.tail[rx.tail_len..rx.tail_len + rest.len()].copy_from_slice(rest);
        rx.tail_len += rest.len();
        if end == rx.len as u64 && rx.tail_len > 0 {
            rx.tail[rx.tail_len..].fill(0xFF);
            self.program(p.offset + rx.flushed, &rx.tail)?;
            rx.flushed += WRITE_UNIT;
            rx.tail_len = 0;
        }
        self.rx = Some(rx);
        Ok(())
    }

    fn read(&mut self, t: u8, slot: u8, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let p = ota_slot(t, slot)?;
        if offset as u64 + buf.len() as u64 > p.size as u64 {
            return Err(StoreError::Range);
        }
        self.wdt.feed();
        flash_err(self.flash.read(p.offset + offset, buf))
    }

    fn write_header(&mut self, slot: u8, hdr: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        ota_slot(target::APP, slot)?;
        self.rx = None;
        self.program(header_addr(slot)?, hdr)
    }

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        flash_err(self.flash.read(header_addr(slot)?, out))
    }

    fn set_pending(&mut self, slot: u8, _header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        ota_slot(target::APP, slot)?;
        self.ota.set_next(slot)
    }

    fn confirm(&mut self, _slot: u8, _header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.ota.confirm()
    }
}
