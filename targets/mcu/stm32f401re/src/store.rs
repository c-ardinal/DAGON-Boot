//! Kind S receiver: APP slots with the header inline at the slot start, boot
//! target switches and confirms appended to the state log.

use dagon_agent::{SlotStore, StoreError, Transition};
use dagon_core::boot::SlotLayout;
use dagon_core::log::{Log, LogError, LogLayout, LogState, SlotRef};
use dagon_format::header::{header_digest16, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{kind, reason, slot};
use dagon_port::{Flash, FlashError};
use dagon_proto::payload::{target, SlotState, StatusResp};

const HDR: u32 = HEADER_SIZE as u32;
/// Flash write unit this store pads to (STM32F4: 32-bit programming).
const UNIT: usize = 4;

/// Body bytes waiting for a full write unit.
#[derive(Clone, Copy, Debug, Default)]
struct Tail {
    /// Body offset of `bytes[0]`.
    at: u32,
    bytes: [u8; UNIT],
    len: usize,
    /// Body length given to the last erase: the write ending there flushes.
    end: u32,
}

pub struct KindSStore<F> {
    flash: F,
    log: Log,
    slots: SlotLayout,
    tail: Tail,
}

impl<F: Flash> KindSStore<F> {
    /// Loads the state log; fails when no log face is valid.
    pub fn load(mut flash: F, log: &LogLayout, slots: SlotLayout) -> Result<Self, LogError> {
        let log = Log::load(&mut flash, log)?;
        Ok(KindSStore {
            flash,
            log,
            slots,
            tail: Tail::default(),
        })
    }

    pub fn log_state(&self) -> LogState {
        self.log.state
    }

    pub fn flash(&mut self) -> &mut F {
        &mut self.flash
    }

    fn base(&self, s: u8) -> Result<u32, StoreError> {
        self.slots
            .slots
            .get(s as usize)
            .copied()
            .ok_or(StoreError::Range)
    }

    /// Body start of APP `s`, checking that `offset..offset+len` fits.
    fn body(&self, t: u8, s: u8, offset: u32, len: usize) -> Result<u32, StoreError> {
        if t != target::APP {
            return Err(StoreError::Unsupported);
        }
        let base = self.base(s)?;
        let end = offset as u64 + len as u64;
        if end > (self.slots.slot_len - HDR) as u64 {
            return Err(StoreError::Range);
        }
        Ok(base + HDR)
    }

    /// The slot's header if it parses (an erased header does not).
    fn header(&mut self, s: u8) -> Option<[u8; HEADER_SIZE]> {
        let mut h = [0u8; HEADER_SIZE];
        self.read_header(s, &mut h).ok()?;
        ImageHeader::parse(&h).ok().map(|_| h)
    }

    fn append(&mut self, k: u8, s: u8, hdr: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let version = ImageHeader::parse(hdr)
            .map_err(|_| StoreError::Range)?
            .version;
        let at = SlotRef {
            slot: s,
            version,
            digest16: header_digest16(hdr),
        };
        self.log
            .append(&mut self.flash, k, at, reason::NONE)
            .map_err(|_| StoreError::Flash)
    }

    fn erase_size(&self, addr: u32) -> Result<u32, StoreError> {
        self.flash
            .geometry()
            .iter()
            .find(|r| addr >= r.start && addr - r.start < r.len)
            .map(|r| r.erase_size)
            .ok_or(StoreError::Range)
    }
}

fn flash_err(_: FlashError) -> StoreError {
    StoreError::Flash
}

impl<F: Flash> SlotStore for KindSStore<F> {
    /// Slots with a parsing header are Written; the log upgrades the slot
    /// whose header digest it names.
    fn status(&mut self) -> StatusResp {
        let mut st = StatusResp::default();
        for s in 0..2u8 {
            if let Some(h) = self.header(s) {
                let info = &mut st.slots[s as usize];
                info.state = SlotState::Written;
                info.version = ImageHeader::parse(&h)
                    .map(|h| h.version)
                    .unwrap_or_default();
                info.digest16 = header_digest16(&h);
            }
        }
        let ls = self.log.state;
        let trial = if ls.on_trial() {
            SlotState::Trying
        } else {
            SlotState::Pending
        };
        for (r, state) in [(ls.confirmed, SlotState::Confirmed), (ls.pending, trial)] {
            let Some(r) = r else { continue };
            if let Some(info) = st.slots.get_mut(r.slot as usize) {
                if info.state != SlotState::Empty && info.digest16 == r.digest16 {
                    info.state = state;
                }
            }
        }
        if let Some(info) = ls.reverted.and_then(|s| st.slots.get_mut(s as usize)) {
            if info.state == SlotState::Written {
                info.state = SlotState::Reverted;
            }
        }
        st.active = ls.active();
        st.confirmed = ls.confirmed.map_or(slot::NONE, |c| c.slot);
        st.attempts = ls.attempts;
        st.last_reason = ls.last_reason;
        st.log_generation = self.log.generation;
        st
    }

    fn capacity(&self, t: u8, s: u8) -> u32 {
        match t {
            target::APP if (s as usize) < self.slots.slots.len() => self.slots.slot_len - HDR,
            _ => 0,
        }
    }

    /// Erases from the slot start (header included) through the erase unit
    /// holding the last body byte.
    fn erase(&mut self, t: u8, s: u8, len: u32, _: &Transition) -> Result<(), StoreError> {
        self.body(t, s, 0, len as usize)?;
        let base = self.base(s)?;
        let unit = self.erase_size(base)?;
        let span = (HDR + len).div_ceil(unit) * unit;
        self.flash.erase(base, span).map_err(flash_err)?;
        self.tail = Tail {
            end: len,
            ..Tail::default()
        };
        Ok(())
    }

    fn write(
        &mut self,
        t: u8,
        s: u8,
        offset: u32,
        data: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let body = self.body(t, s, offset, data.len())?;
        let mut tail = self.tail;
        if offset != tail.at + tail.len as u32 {
            return Err(StoreError::Range);
        }
        let mut rest = data;
        if tail.len > 0 {
            let n = (UNIT - tail.len).min(rest.len());
            tail.bytes[tail.len..tail.len + n].copy_from_slice(&rest[..n]);
            tail.len += n;
            rest = &rest[n..];
            if tail.len == UNIT {
                self.flash
                    .write(body + tail.at, &tail.bytes)
                    .map_err(flash_err)?;
                tail.at += UNIT as u32;
                tail.len = 0;
            }
        }
        let bulk = rest.len() / UNIT * UNIT;
        if bulk > 0 {
            self.flash
                .write(body + tail.at, &rest[..bulk])
                .map_err(flash_err)?;
            tail.at += bulk as u32;
        }
        let left = &rest[bulk..];
        tail.bytes[tail.len..tail.len + left.len()].copy_from_slice(left);
        tail.len += left.len();
        if tail.len > 0 && offset + data.len() as u32 == tail.end {
            tail.bytes[tail.len..].fill(0xFF);
            self.flash
                .write(body + tail.at, &tail.bytes)
                .map_err(flash_err)?;
            tail.at += UNIT as u32;
            tail.len = 0;
        }
        self.tail = tail;
        Ok(())
    }

    fn read(&mut self, t: u8, s: u8, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let body = self.body(t, s, offset, buf.len())?;
        self.flash.read(body + offset, buf).map_err(flash_err)
    }

    fn write_header(
        &mut self,
        s: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let base = self.base(s)?;
        self.flash.write(base, header).map_err(flash_err)
    }

    fn read_header(&mut self, s: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let base = self.base(s)?;
        self.flash.read(base, out).map_err(flash_err)
    }

    fn set_pending(
        &mut self,
        s: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(kind::SLOT_PENDING, s, header)
    }

    fn confirm(
        &mut self,
        s: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(kind::CONFIRMED, s, header)
    }
}
