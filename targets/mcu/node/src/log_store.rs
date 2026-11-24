//! The slot store of an MCU whose boot stage keeps a DAGON state log
//! (`dagon_core::log`): APP slots, and optionally boot core slots, each a
//! [`Place`] (the header inline at the slot start or detached into its own
//! erase unit); boot target switches and confirms appended to the log.
//! What differs between MCUs (the write unit, the trial attempts, the slot
//! layouts the erase plan covers, stage0's region) comes from [`LogMcu`].

use core::marker::PhantomData;
use dagon_port::{SlotStore, StoreError};

use dagon_agent::{erase_plan, Transition};
use dagon_core::boot::{Place, SlotLayout};
use dagon_core::log::{trial_room, Log, LogError, LogLayout, LogState, SlotRef};
use dagon_format::header::{header_digest16, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{kind, reason, slot};
use dagon_format::{stage0, Version};
use dagon_port::timeout::{EraseLayout, EraseRegion, Plan};
use dagon_port::{Flash, FlashError};
use dagon_proto::payload::{target, SlotInfo, StatusResp};

/// The largest flash write unit a [`LogStore`] pads to.
pub const MAX_WRITE_UNIT: usize = 16;

/// What a [`LogStore`] takes from its MCU's definition.
pub trait LogMcu {
    /// The flash write unit the store pads to (`[execution] write_unit`),
    /// at most [`MAX_WRITE_UNIT`].
    const WRITE_UNIT: usize;
    /// The value of an erased byte.
    const ERASED: u8;
    /// The boots a pending image gets (`[watchdog] trial_attempts`): the
    /// room the log keeps for them.
    const TRIAL_ATTEMPTS: u8;
    /// The slot layouts the erase plan covers (`[[slot_layout]]`).
    const SLOT_LAYOUTS: &'static [dagon_port::timeout::SlotLayout];
    /// Stage0's region (start, length), where its version mark is found;
    /// `None`: no stage0 this store reads.
    const STAGE0: Option<(u32, u32)>;
}

/// Body bytes waiting for a full write unit.
#[derive(Clone, Copy, Debug, Default)]
struct Tail {
    /// Body offset of `bytes[0]`.
    at: u32,
    bytes: [u8; MAX_WRITE_UNIT],
    len: usize,
    /// Body length given to the last erase: the write ending there flushes.
    end: u32,
}

pub struct LogStore<F, M> {
    flash: F,
    log: Log,
    /// The APP slot the running image was started from (`slot::NONE`:
    /// no app runs, as in the boot core's update mode).
    running: u8,
    slots: SlotLayout<'static>,
    /// Boot core slots (start, length), A first, header inline; `None`:
    /// the MCU keeps none.
    cores: Option<&'static [(u32, u32)]>,
    /// The trial cell holds what is written to it; without it no boot core
    /// is taken (stage0 could not count its starts).
    cell_holds: bool,
    /// The erase regions the board returns (`BoardDef::ERASE_REGIONS`).
    regions: &'static [EraseRegion],
    tail: Tail,
    mcu: PhantomData<M>,
}

impl<F: Flash, M: LogMcu> LogStore<F, M> {
    const UNIT: usize = {
        assert!(M::WRITE_UNIT > 0 && M::WRITE_UNIT <= MAX_WRITE_UNIT);
        M::WRITE_UNIT
    };

    /// Loads the state log; fails when no log face is valid. `regions`:
    /// what the board erases in (`BoardDef::ERASE_REGIONS`). `running`:
    /// the APP slot the caller runs from (the app's own link slot;
    /// `slot::NONE` when no app runs). The log cannot tell it: a boot core
    /// that could not append its BOOT_ATTEMPT starts the confirmed slot
    /// with the trial the log names left open.
    pub fn load(
        mut flash: F,
        log: &LogLayout,
        slots: SlotLayout<'static>,
        regions: &'static [EraseRegion],
        running: u8,
    ) -> Result<Self, LogError> {
        let log = Log::load(&mut flash, log)?;
        Ok(Self::over(flash, log, slots, regions, running))
    }

    /// [`LogStore::load`] over [`Log::load_or_create`]: a log with no
    /// valid face is created empty, so a boot core's recovery mode can
    /// write the app even when the log was lost.
    pub fn load_or_create(
        mut flash: F,
        log: &LogLayout,
        slots: SlotLayout<'static>,
        regions: &'static [EraseRegion],
        running: u8,
    ) -> Result<Self, LogError> {
        let log = Log::load_or_create(&mut flash, log)?;
        Ok(Self::over(flash, log, slots, regions, running))
    }

    fn over(
        flash: F,
        log: Log,
        slots: SlotLayout<'static>,
        regions: &'static [EraseRegion],
        running: u8,
    ) -> Self {
        LogStore {
            flash,
            log,
            running,
            slots,
            cores: None,
            cell_holds: false,
            regions,
            tail: Tail::default(),
            mcu: PhantomData,
        }
    }

    /// The boot core slots (start, length, A first) this store also
    /// receives into; their records go to the same log. `cell_holds`: the
    /// trial cell holds what is written to it; without it the slots report
    /// no capacity, so the agent refuses BOOTCORE.
    pub fn with_cores(self, cores: &'static [(u32, u32)], cell_holds: bool) -> Self {
        LogStore {
            cores: Some(cores),
            cell_holds,
            ..self
        }
    }

    pub fn log_state(&self) -> LogState {
        self.log.state
    }

    /// The place of slot `s` of target `t`.
    fn slot(&self, t: u8, s: u8) -> Result<Place, StoreError> {
        let i = usize::from(s);
        match t {
            target::APP => self.slots.slots.get(i).copied().ok_or(StoreError::Range),
            target::BOOTCORE => self
                .cores
                .ok_or(StoreError::Unsupported)?
                .get(i)
                .map(|&(start, len)| Place::inline(start, len))
                .ok_or(StoreError::Range),
            _ => Err(StoreError::Unsupported),
        }
    }

    /// Body start of slot `s` of `t`, checking that `offset..offset+len`
    /// fits.
    fn body(&self, t: u8, s: u8, offset: u32, len: usize) -> Result<u32, StoreError> {
        let p = self.slot(t, s)?;
        if offset as u64 + len as u64 > u64::from(p.room) {
            return Err(StoreError::Range);
        }
        Ok(p.body)
    }

    /// The header of slot `s` of `t` as a log reference, if it parses.
    fn head(&mut self, t: u8, s: u8) -> Option<SlotRef> {
        let at = self.slot(t, s).ok()?.header;
        let mut h = [0u8; HEADER_SIZE];
        self.flash.read(at, &mut h).ok()?;
        Some(SlotRef {
            slot: s,
            version: ImageHeader::parse(&h).ok()?.version,
            digest16: header_digest16(&h),
        })
    }

    /// The header in each slot of `t` STATUS can carry (`wire`); `None`
    /// past the slots this store has.
    fn heads<const N: usize>(&mut self, t: u8, _wire: &[SlotInfo; N]) -> [Option<SlotRef>; N] {
        core::array::from_fn(|s| self.head(t, s as u8))
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
                .reserve(&mut self.flash, trial_room(M::TRIAL_ATTEMPTS))
                .map_err(|_| StoreError::Flash)?;
        }
        let r = match t {
            target::BOOTCORE => self.log.append_core(&mut self.flash, k, at, reason::NONE),
            _ => self.log.append(&mut self.flash, k, at, reason::NONE),
        };
        r.map_err(|_| StoreError::Flash)
    }
}

fn flash_err(_: FlashError) -> StoreError {
    StoreError::Flash
}

impl<F: Flash, M: LogMcu> SlotStore for LogStore<F, M> {
    type Transition = Transition;

    /// Slots with a parsing header are Written; the log upgrades the slot
    /// whose header it names (`LogState::status`), the running slot the
    /// active one.
    fn status(&mut self) -> StatusResp {
        let heads = self.heads(target::APP, &StatusResp::default().slots);
        self.log
            .state
            .status(&heads, self.running, self.log.generation)
    }

    fn capacity(&self, t: u8, s: u8) -> u32 {
        if t == target::BOOTCORE && !self.cell_holds {
            return 0;
        }
        self.slot(t, s).map_or(0, |p| p.room)
    }

    /// The definition's plan ([`SlotStore::erase_planned`]).
    fn erase(&mut self, t: u8, s: u8, len: u32, tr: &Transition) -> Result<(), StoreError> {
        let layout = self.erase_layout().ok_or(StoreError::Unsupported)?;
        let plan = erase_plan(&layout, t, s, len)?;
        self.erase_planned(t, s, len, plan, tr)
    }

    /// The board's erase regions and the MCU's slot layouts.
    fn erase_layout(&self) -> Option<EraseLayout> {
        Some(EraseLayout {
            regions: self.regions,
            slots: M::SLOT_LAYOUTS,
        })
    }

    /// Erases the planned sectors (the header's, inline at the slot start
    /// or detached, and the body's through the sector holding its last
    /// byte), one flash operation each.
    fn erase_planned(
        &mut self,
        t: u8,
        s: u8,
        len: u32,
        plan: Plan<'_>,
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.body(t, s, 0, len as usize)?;
        for op in plan {
            self.flash.erase(op.addr, op.len).map_err(flash_err)?;
        }
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
        let unit = Self::UNIT;
        let body = self.body(t, s, offset, data.len())?;
        let mut tail = self.tail;
        if offset != tail.at + tail.len as u32 {
            return Err(StoreError::Range);
        }
        let mut rest = data;
        if tail.len > 0 {
            let n = (unit - tail.len).min(rest.len());
            tail.bytes[tail.len..tail.len + n].copy_from_slice(&rest[..n]);
            tail.len += n;
            rest = &rest[n..];
            if tail.len == unit {
                self.flash
                    .write(body + tail.at, &tail.bytes[..unit])
                    .map_err(flash_err)?;
                tail.at += unit as u32;
                tail.len = 0;
            }
        }
        let bulk = rest.len() / unit * unit;
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
            tail.bytes[tail.len..unit].fill(M::ERASED);
            self.flash
                .write(body + tail.at, &tail.bytes[..unit])
                .map_err(flash_err)?;
            tail.at += unit as u32;
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
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.write_target_header(target::APP, s, header, t)
    }

    fn read_header(&mut self, s: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.read_target_header(target::APP, s, out)
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
            .reserve(&mut self.flash, trial_room(M::TRIAL_ATTEMPTS))
            .map_err(|_| StoreError::Flash)
    }

    /// Stage0's version mark in its region (`dagon_format::stage0`).
    fn stage0_version(&mut self) -> Option<Version> {
        let (at, len) = M::STAGE0?;
        stage0::find(at, len, |a, b| self.flash.read(a, b).is_ok())
    }

    /// The boot core slots: the core settles its trial before it starts
    /// the app, so the confirmed core is the one running (slot A, as
    /// stage0 starts it, while the log names none).
    fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        match t {
            target::APP => Some(self.status()),
            target::BOOTCORE if self.cores.is_some() => {
                let heads = self.heads(target::BOOTCORE, &StatusResp::default().slots);
                let c = self.log.core;
                let running = c.confirmed.map_or(slot::A, |r| r.slot);
                Some(c.status(&heads, running, self.log.generation))
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
        let at = self.slot(t, s)?.header;
        self.flash.write(at, header).map_err(flash_err)
    }

    fn read_target_header(
        &mut self,
        t: u8,
        s: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        let at = self.slot(t, s)?.header;
        self.flash.read(at, out).map_err(flash_err)
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
