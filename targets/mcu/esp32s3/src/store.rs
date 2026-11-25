//! The agent's receiver on the ESP32-S3 (kind V).
//!
//! - Each target and slot (app A and B, the agent's factory slot) is where
//!   the definition's slot layout puts it (`[[slot_layout]]`: start,
//!   capacity, detached header): the body from the start, only what it
//!   covers erased; the 512-byte header in its own erase unit of the dagon
//!   partition.
//! - SET_PENDING writes an otadata trial entry; CONFIRM marks the running
//!   entry Valid. No DAGON state log is kept: STATUS maps otadata.
//! - Otadata keeps no digest, so an erase first takes the slot out of the
//!   selection: a new image there is only written (not pending, not
//!   reverted) until its own SET_PENDING.
//! - The recovery agent (target AGENT, slot factory) goes to the factory
//!   partition the same way, header last; otadata never names it (the 2nd
//!   stage starts factory when no OTA slot is valid), so there is nothing
//!   to withdraw or select.
//! - A first write also leaves the agent's header in the last erase unit
//!   of the factory partition ([`header::header_copy_at`]). STATUS reports
//!   that copy when the detached header is gone (the dagon partition
//!   erased) and the copy's digest is the body's.

use crate::layout::{SLOT_FACTORY, WRITE_UNIT};
use crate::otadata::{selected_view, Counted, ENTRY_LEN};
use dagon_agent::{erase_plan, Transition};
use dagon_format::header::{self, image_kind, ImageHeader, HEADER_SIZE};
use dagon_format::Digest;
use dagon_port::timeout::{EraseLayout, EraseRegion, HeaderPlace, Plan, SlotLayout};
use dagon_port::{Flash, Watchdog};
use dagon_port::{SlotStore, StoreError};
use dagon_proto::payload::{target, SlotState, StatusResp};

/// One detached header sector (an erase unit) per slot in the dagon
/// partition.
pub const HEADER_SECTOR: u32 = crate::layout::ERASE_UNIT;
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
    /// Takes OTA `slot` (never the running one) out of the selection:
    /// erases every counted entry selecting it ([`crate::otadata::selecting`]).
    fn withdraw(&mut self, slot: u8) -> Result<(), StoreError>;
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
    pub(crate) flash: F,
    /// The boot selection; changed only through the image machine's
    /// transitions (the `SlotStore` mutators).
    pub(crate) ota: O,
    /// Fed while erasing and writing (a handle; the MCU package's link
    /// task holds the watchdog).
    pub(crate) wdt: W,
    /// The erase regions the board returns (`BoardDef::erase_regions`).
    regions: &'static [EraseRegion],
    /// Where each target and slot lies (the definition's slot layouts).
    slots: &'static [SlotLayout],
    rx: Option<Rx>,
    /// The agent's header copy, once looked for (`Some(None)`: none that
    /// names the body); forgotten when the agent's slot is erased.
    agent_copy: Option<Option<[u8; HEADER_SIZE]>>,
}

/// Where a body (start, capacity) and its detached header go, for a target
/// this store receives (the app and the agent).
#[derive(Clone, Copy)]
struct Place {
    start: u32,
    capacity: u32,
    header: u32,
}

/// The place of slot `slot` of target `t` in `slots`.
fn place(slots: &[SlotLayout], t: u8, slot: u8) -> Result<Place, StoreError> {
    if t != target::APP && t != target::AGENT {
        return Err(StoreError::Unsupported);
    }
    let l = slots
        .iter()
        .find(|l| (l.target, l.slot) == (t, slot))
        .ok_or(StoreError::Unsupported)?;
    match l.header {
        HeaderPlace::Detached { at } => Ok(Place {
            start: l.start,
            capacity: l.capacity,
            header: at,
        }),
        HeaderPlace::Inline => Err(StoreError::Unsupported),
    }
}

/// Start of the detached header sector of slot `slot` of target `t` in the
/// definition's slot layouts.
pub fn header_addr(t: u8, slot: u8) -> Result<u32, StoreError> {
    place(&crate::SLOT_LAYOUTS, t, slot)
        .map(|p| p.header)
        .map_err(|_| StoreError::Range)
}

fn flash_err<T>(r: Result<T, dagon_port::FlashError>) -> Result<T, StoreError> {
    r.map_err(|_| StoreError::Flash)
}

impl<F: Flash, O: Otadata, W: Watchdog> EspStore<F, O, W> {
    /// `regions`: what the board erases in (`BoardDef::erase_regions`).
    pub fn new(flash: F, ota: O, wdt: W, regions: &'static [EraseRegion]) -> Self {
        Self {
            flash,
            ota,
            wdt,
            regions,
            slots: &crate::SLOT_LAYOUTS,
            rx: None,
            agent_copy: None,
        }
    }

    /// Receives into the places `slots` gives instead of the definition's.
    pub fn with_slots(self, slots: &'static [SlotLayout]) -> Self {
        Self { slots, ..self }
    }

    fn place(&self, t: u8, slot: u8) -> Result<Place, StoreError> {
        place(self.slots, t, slot)
    }

    /// Parsable header of slot `slot` of target `t`, if any: the detached
    /// one, else the agent's copy in its own partition.
    fn target_header(&mut self, t: u8, slot: u8) -> Option<[u8; HEADER_SIZE]> {
        let p = self.place(t, slot).ok()?;
        let mut h = [0u8; HEADER_SIZE];
        flash_err(self.flash.read(p.header, &mut h)).ok()?;
        if ImageHeader::parse(&h).is_ok() {
            return Some(h);
        }
        if t != target::AGENT {
            return None;
        }
        if self.agent_copy.is_none() {
            self.agent_copy = Some(self.header_copy(p));
        }
        self.agent_copy.flatten()
    }

    /// The agent header a first write left at the end of the body's region
    /// `p`, only when it names the body there (kind, length, SHA-256): an
    /// agent received since has another body, so its stale copy is not
    /// reported.
    fn header_copy(&mut self, p: Place) -> Option<[u8; HEADER_SIZE]> {
        let unit = self
            .regions
            .iter()
            .find(|r| p.start >= r.start && p.start - r.start < r.len)?
            .unit;
        let mut raw = [0u8; HEADER_SIZE];
        // A copy is only where a body of no length leaves room for one.
        let at = header::header_copy_at(p.start, p.capacity, unit, 0)?;
        flash_err(self.flash.read(at, &mut raw)).ok()?;
        let h = ImageHeader::parse(&raw).ok()?;
        if h.image_kind != image_kind::AGENT
            || header::header_copy_at(p.start, p.capacity, unit, h.payload_size) != Some(at)
        {
            return None;
        }
        let mut sha = dagon_format::Sha256::default();
        let mut buf = [0u8; 256];
        let mut done = 0u32;
        while done < h.payload_size {
            let n = (h.payload_size - done).min(buf.len() as u32) as usize;
            self.wdt.feed();
            flash_err(self.flash.read(p.start + done, &mut buf[..n])).ok()?;
            sha.update(&buf[..n]);
            done += n as u32;
        }
        (sha.finalize() == h.payload_sha256).then_some(raw)
    }

    fn program(&mut self, addr: u32, data: &[u8]) -> Result<(), StoreError> {
        self.wdt.feed();
        flash_err(self.flash.write(addr, data))
    }
}

impl<F: Flash, O: Otadata, W: Watchdog> SlotStore for EspStore<F, O, W> {
    type Transition = Transition;

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
        for l in self.slots {
            let s = l.slot;
            if usize::from(s) >= st.slots.len() {
                continue;
            }
            if let Some(h) = self.target_header(l.target, s) {
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
            // An erase withdraws the selection (otadata keeps no digest),
            // so a selected slot still holds the image it was selected
            // with.
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
        self.place(t, slot).map_or(0, |p| p.capacity)
    }

    /// The definition's plan ([`SlotStore::erase_planned`]).
    fn erase(&mut self, t: u8, slot: u8, len: u32, tr: &Transition) -> Result<(), StoreError> {
        let layout = self.erase_layout().ok_or(StoreError::Unsupported)?;
        let plan = erase_plan(&layout, t, slot, len)?;
        self.erase_planned(t, slot, len, plan, tr)
    }

    /// The board's erase regions and the MCU's slot layouts.
    fn erase_layout(&self) -> Option<EraseLayout> {
        Some(EraseLayout {
            regions: self.regions,
            slots: self.slots,
        })
    }

    /// Takes the slot out of the otadata selection, then erases the planned
    /// units: the header sector first (the slot then has no image), then
    /// the body's blocks, feeding between operations.
    fn erase_planned(
        &mut self,
        t: u8,
        slot: u8,
        len: u32,
        plan: Plan<'_>,
        _: &Transition,
    ) -> Result<(), StoreError> {
        let p = self.place(t, slot)?;
        // Never wipe what is running, whatever the caller thinks.
        if slot == self.ota.running() {
            return Err(StoreError::Unsupported);
        }
        if len > p.capacity {
            return Err(StoreError::Range);
        }
        self.rx = None;
        if t == target::APP {
            self.ota.withdraw(slot)?;
        } else {
            self.agent_copy = None;
        }
        for op in plan {
            self.wdt.feed();
            flash_err(self.flash.erase(op.addr, op.len))?;
        }
        self.wdt.feed();
        self.rx = Some(Rx {
            slot,
            len,
            flushed: 0,
            tail: [crate::ERASED_VALUE; WORD],
            tail_len: 0,
        });
        Ok(())
    }

    fn write(
        &mut self,
        t: u8,
        slot: u8,
        offset: u32,
        data: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let p = self.place(t, slot)?;
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
                self.program(p.start + rx.flushed, &rx.tail)?;
                rx.flushed += WRITE_UNIT;
                rx.tail_len = 0;
            }
        }
        let whole = data.len() / WORD * WORD;
        if whole > 0 {
            self.program(p.start + rx.flushed, &data[..whole])?;
            rx.flushed += whole as u32;
        }
        let rest = &data[whole..];
        rx.tail[rx.tail_len..rx.tail_len + rest.len()].copy_from_slice(rest);
        rx.tail_len += rest.len();
        if end == rx.len as u64 && rx.tail_len > 0 {
            rx.tail[rx.tail_len..].fill(crate::ERASED_VALUE);
            self.program(p.start + rx.flushed, &rx.tail)?;
            rx.flushed += WRITE_UNIT;
            rx.tail_len = 0;
        }
        self.rx = Some(rx);
        Ok(())
    }

    fn read(&mut self, t: u8, slot: u8, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let p = self.place(t, slot)?;
        if offset as u64 + buf.len() as u64 > p.capacity as u64 {
            return Err(StoreError::Range);
        }
        self.wdt.feed();
        flash_err(self.flash.read(p.start + offset, buf))
    }

    fn write_header(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let at = self.place(target::APP, slot)?.header;
        self.rx = None;
        self.program(at, hdr)
    }

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let at = self
            .place(target::APP, slot)
            .map_err(|_| StoreError::Range)?
            .header;
        flash_err(self.flash.read(at, out))
    }

    fn set_pending(
        &mut self,
        slot: u8,
        _header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.place(target::APP, slot)?;
        self.ota.set_next(slot)
    }

    fn confirm(
        &mut self,
        _slot: u8,
        _header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.ota.confirm()
    }

    fn write_target_header(
        &mut self,
        t: u8,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let at = self.place(t, slot)?.header;
        self.rx = None;
        self.program(at, hdr)
    }

    fn read_target_header(
        &mut self,
        t: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        let at = self.place(t, slot).map_err(|_| StoreError::Range)?.header;
        flash_err(self.flash.read(at, out))
    }
}
