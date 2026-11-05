//! Kind S receiver: APP slots and boot core slots with the header inline at
//! the slot start, boot target switches and confirms appended to the state
//! log (one log holds the APP and the boot core records).

use dagon_agent::{SlotStore, StoreError, Transition};
use dagon_core::boot::SlotLayout;
use dagon_core::log::{trial_room, Log, LogError, LogLayout, LogState, SlotRef};
use dagon_format::header::{header_digest16, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{kind, reason, slot};
use dagon_format::{stage0, Version};

use crate::layout::{STAGE0, STAGE0_SIZE};

use dagon_port::{Flash, FlashError};
use dagon_proto::payload::{target, StatusResp};

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
    /// Boot core slots A/B (start, length), header inline; `None`: the
    /// MCU keeps none.
    cores: Option<[(u32, u32); 2]>,
    /// The trial cell holds what is written to it; without it no boot core
    /// is taken (stage0 could not count its starts).
    cell_holds: bool,
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
            cores: None,
            cell_holds: false,
            tail: Tail::default(),
        })
    }

    /// The boot core slots A/B (start, length) this store also receives
    /// into; their records go to the same log. `cell_holds`: the trial
    /// cell holds what is written to it (`trial::holds`); without it the
    /// slots report no capacity, so the agent refuses BOOTCORE.
    pub fn with_cores(self, cores: [(u32, u32); 2], cell_holds: bool) -> Self {
        KindSStore {
            cores: Some(cores),
            cell_holds,
            ..self
        }
    }

    pub fn log_state(&self) -> LogState {
        self.log.state
    }

    /// Start and length of slot `s` of target `t`.
    fn slot(&self, t: u8, s: u8) -> Result<(u32, u32), StoreError> {
        let i = usize::from(s);
        match t {
            target::APP => self
                .slots
                .slots
                .get(i)
                .map(|&b| (b, self.slots.slot_len))
                .ok_or(StoreError::Range),
            target::BOOTCORE => self
                .cores
                .ok_or(StoreError::Unsupported)?
                .get(i)
                .copied()
                .ok_or(StoreError::Range),
            _ => Err(StoreError::Unsupported),
        }
    }

    fn base(&self, s: u8) -> Result<u32, StoreError> {
        self.slot(target::APP, s).map(|(b, _)| b)
    }

    /// Body start of slot `s` of `t`, checking that `offset..offset+len`
    /// fits.
    fn body(&self, t: u8, s: u8, offset: u32, len: usize) -> Result<u32, StoreError> {
        let (base, size) = self.slot(t, s)?;
        let end = offset as u64 + len as u64;
        if end > (size - HDR) as u64 {
            return Err(StoreError::Range);
        }
        Ok(base + HDR)
    }

    /// The header of slot `s` of `t` as a log reference, if it parses.
    fn head(&mut self, t: u8, s: u8) -> Option<SlotRef> {
        let (base, _) = self.slot(t, s).ok()?;
        let mut h = [0u8; HEADER_SIZE];
        self.flash.read(base, &mut h).ok()?;
        Some(SlotRef {
            slot: s,
            version: ImageHeader::parse(&h).ok()?.version,
            digest16: header_digest16(&h),
        })
    }

    fn append(&mut self, t: u8, k: u8, s: u8, hdr: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let version = ImageHeader::parse(hdr)
            .map_err(|_| StoreError::Range)?
            .version;
        let at = SlotRef {
            slot: s,
            version,
            digest16: header_digest16(hdr),
        };
        if k == kind::SLOT_PENDING {
            // The trials then write into this room, never erasing.
            self.log
                .reserve(&mut self.flash, trial_room(crate::TRIAL_ATTEMPTS))
                .map_err(|_| StoreError::Flash)?;
        }
        let r = match t {
            target::BOOTCORE => self.log.append_core(&mut self.flash, k, at, reason::NONE),
            _ => self.log.append(&mut self.flash, k, at, reason::NONE),
        };
        r.map_err(|_| StoreError::Flash)
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
    /// whose header it names (`LogState::status`).
    fn status(&mut self) -> StatusResp {
        let heads = [0u8, 1].map(|s| self.head(target::APP, s));
        let ls = self.log.state;
        ls.status(heads, ls.active(), self.log.generation)
    }

    fn capacity(&self, t: u8, s: u8) -> u32 {
        if t == target::BOOTCORE && !self.cell_holds {
            return 0;
        }
        self.slot(t, s).map_or(0, |(_, size)| size - HDR)
    }

    /// Erases from the slot start (header included) through the erase unit
    /// holding the last body byte.
    fn erase(&mut self, t: u8, s: u8, len: u32, _: &Transition) -> Result<(), StoreError> {
        self.body(t, s, 0, len as usize)?;
        let (base, _) = self.slot(t, s)?;
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
        self.append(target::APP, kind::SLOT_PENDING, s, header)
    }

    fn confirm(
        &mut self,
        s: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(target::APP, kind::CONFIRMED, s, header)
    }

    /// The log keeps room for the trials it starts (the boot stage never
    /// erases).
    fn keep_room(&mut self) -> Result<(), StoreError> {
        self.log
            .reserve(&mut self.flash, trial_room(crate::TRIAL_ATTEMPTS))
            .map_err(|_| StoreError::Flash)
    }

    /// Stage0's version mark in its region (`dagon_format::stage0`).
    fn stage0_version(&mut self) -> Option<Version> {
        stage0::find(STAGE0, STAGE0_SIZE, |a, b| self.flash.read(a, b).is_ok())
    }

    /// The boot core slots: the core settles its trial before it starts
    /// the app, so the confirmed core is the one running (slot A, as
    /// stage0 starts it, while the log names none).
    fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        match t {
            target::APP => Some(self.status()),
            target::BOOTCORE if self.cores.is_some() => {
                let heads = [0u8, 1].map(|s| self.head(target::BOOTCORE, s));
                let c = self.log.core;
                let running = c.confirmed.map_or(slot::A, |r| r.slot);
                Some(c.status(heads, running, self.log.generation))
            }
            _ => None,
        }
    }

    fn write_target_header(
        &mut self,
        t: u8,
        s: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let (base, _) = self.slot(t, s)?;
        self.flash.write(base, header).map_err(flash_err)
    }

    fn read_target_header(
        &mut self,
        t: u8,
        s: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        let (base, _) = self.slot(t, s)?;
        self.flash.read(base, out).map_err(flash_err)
    }

    fn set_target_pending(
        &mut self,
        t: u8,
        s: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.slot(t, s)?;
        self.append(t, kind::SLOT_PENDING, s, header)
    }
}
