//! Receiver side of an RP2040 app: the APP slots, the app state log and
//! the boot core slots with the boot core log, behind dagon-agent's
//! `SlotStore`.

use crate::image;
use crate::layout::{APP_LOG_LAYOUT, BL_LOG_LAYOUT, STAGE0, STAGE0_SIZE};
use crate::TRIAL_ATTEMPTS;
use dagon_agent::{erase_plan, Transition};
use dagon_core::log::{trial_room, Log, SlotRef};
use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{kind, slot};
use dagon_format::{stage0, Version};
use dagon_node::guard::Watch;
use dagon_port::timeout::{EraseLayout, EraseRegion, Plan};
use dagon_port::Flash;
use dagon_port::{SlotStore, StoreError};
use dagon_proto::payload::{target, Peer, PeerLog, SlotInfo, StatusResp};

/// Slot this build is linked for (`slot-b`).
pub const ACTIVE: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };

/// The app's slot store. `task` is the guard
/// task that serves the agent: long erases report progress for it, and
/// CONFIRM ends the trial.
pub struct Store<F> {
    pub(crate) flash: F,
    pub(crate) watch: &'static dyn Watch,
    pub(crate) task: usize,
    /// The erase regions the board returns (`BoardDef::ERASE_REGIONS`).
    pub(crate) regions: &'static [EraseRegion],
    /// The APP slot the caller runs from (`slot::NONE`: no app runs).
    pub(crate) running: u8,
    pub(crate) chip: Chip,
}

/// What the store asks of the running chip (read when asked, never kept).
#[derive(Clone, Copy)]
pub struct Chip {
    /// Whether the trial cell holds a value across a reset: without it
    /// stage0 could not count a new boot core's starts.
    pub trial_cell_holds: fn() -> bool,
    /// Whether the boot core receives on a link (its update mode).
    pub boot_core_receives: fn() -> bool,
}

/// The store on the RP2040 itself: its flash driver and its own [`Chip`].
#[cfg(feature = "hal")]
pub type AppStore = Store<crate::flash::SharedFlash>;

#[cfg(feature = "hal")]
impl AppStore {
    /// The slot store over `flash`, this MCU's flash driver. A long erase
    /// reports its progress to guard task `task` of `watch`; erases are
    /// planned over `regions` (`BoardDef::ERASE_REGIONS`). `running`: the
    /// APP slot the caller runs from, STATUS's active one (the app's own
    /// link slot, [`ACTIVE`]; `slot::NONE` in the boot core, where no app
    /// runs). The log cannot tell it: a boot core that could not append
    /// its BOOT_ATTEMPT starts the confirmed slot with the trial the log
    /// names left open.
    pub fn new(
        flash: crate::flash::SharedFlash,
        watch: &'static dyn Watch,
        task: usize,
        regions: &'static [EraseRegion],
        running: u8,
    ) -> Self {
        let chip = Chip {
            trial_cell_holds: crate::arm::trial::holds,
            boot_core_receives: crate::clock::boot_core_receives,
        };
        Store::over(flash, watch, task, regions, running, chip)
    }
}

impl<F: Flash> Store<F> {
    /// [`AppStore::new`] over any flash, with what `chip` tells.
    pub fn over(
        flash: F,
        watch: &'static dyn Watch,
        task: usize,
        regions: &'static [EraseRegion],
        running: u8,
        chip: Chip,
    ) -> Self {
        Store {
            flash,
            watch,
            task,
            regions,
            running,
            chip,
        }
    }
}

/// Where a target's region starts, where its body starts, body capacity.
fn region(t: u8, slot: u8) -> Result<(u32, u32, u32), StoreError> {
    crate::regions::region(t, slot).ok_or(StoreError::Unsupported)
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

impl<F: Flash> Store<F> {
    fn append(&mut self, k: u8, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let at = image::ref_of(header, slot).ok_or(StoreError::Flash)?;
        // A log with no valid face (lost, and no slot image passed the
        // boot core's rebuild) is created empty: recovery mode records the
        // app it receives.
        let mut log =
            Log::load_or_create(&mut self.flash, &APP_LOG_LAYOUT).map_err(|_| StoreError::Flash)?;
        if k == kind::SLOT_PENDING {
            // The trial then writes into this room, never erasing.
            log.reserve(&mut self.flash, trial_room(TRIAL_ATTEMPTS))
                .map_err(|_| StoreError::Flash)?;
        }
        log.append(&mut self.flash, k, at, 0)
            .map_err(|_| StoreError::Flash)
    }
}

impl<F: Flash> Store<F> {
    /// The header in each slot of `t` STATUS can carry (`wire`), as a log
    /// reference; `None` past the slots the definition gives.
    fn heads<const N: usize>(&mut self, t: u8, _wire: &[SlotInfo; N]) -> [Option<SlotRef>; N] {
        core::array::from_fn(|s| {
            let (base, _, _) = crate::regions::region(t, s as u8)?;
            image::head_ref(&mut self.flash, base, s as u8)
        })
    }
}

impl<F: Flash> SlotStore for Store<F> {
    type Transition = Transition;

    fn status(&mut self) -> StatusResp {
        let (state, generation) = Log::load(&mut self.flash, &APP_LOG_LAYOUT)
            .map_or((Default::default(), 0), |l| (l.state, l.generation));
        let heads = self.heads(target::APP, &StatusResp::default().slots);
        state.status(&heads, self.running, generation)
    }

    /// No boot core capacity while the trial cell does not hold: stage0
    /// could not count a new core's starts (the agent then refuses
    /// BOOTCORE as unsupported).
    fn capacity(&self, t: u8, slot: u8) -> u32 {
        if t == target::BOOTCORE && !(self.chip.trial_cell_holds)() {
            return 0;
        }
        region(t, slot).map_or(0, |(_, _, cap)| cap)
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
            slots: &crate::SLOT_LAYOUTS,
        })
    }

    /// Erases the planned sectors (from the region start, header included,
    /// through the sector holding the last body byte).
    fn erase_planned(
        &mut self,
        t: u8,
        slot: u8,
        len: u32,
        plan: Plan<'_>,
        _: &Transition,
    ) -> Result<(), StoreError> {
        let (_, _, cap) = region(t, slot)?;
        if len > cap {
            return Err(StoreError::Range);
        }
        // One operation at a time so interrupts run and the watchdog is
        // fed between.
        for op in plan {
            self.flash.erase(op.addr, op.len).map_err(flash_err)?;
            // Lease renewal of the serving task, then a gated feed.
            self.watch.progress(self.task);
        }
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
        let at = body_at(t, slot, offset, data.len())?;
        self.flash.write(at, data).map_err(flash_err)
    }

    fn read(&mut self, t: u8, slot: u8, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let at = body_at(t, slot, offset, buf.len())?;
        self.flash.read(at, buf).map_err(flash_err)
    }

    fn write_header(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let (base, _, _) = region(target::APP, slot)?;
        self.flash.write(base, header).map_err(flash_err)
    }

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let (base, _, _) = region(target::APP, slot)?;
        self.flash.read(base, out).map_err(flash_err)
    }

    fn set_pending(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(kind::SLOT_PENDING, slot, header)
    }

    fn confirm(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(kind::CONFIRMED, slot, header)?;
        self.watch.set_trial(false);
        Ok(())
    }

    /// Both logs keep room for the trials they start (the boot stage
    /// never erases).
    fn keep_room(&mut self) -> Result<(), StoreError> {
        for layout in [&APP_LOG_LAYOUT, &BL_LOG_LAYOUT] {
            let mut log = Log::load(&mut self.flash, layout).map_err(|_| StoreError::Flash)?;
            log.reserve(&mut self.flash, trial_room(TRIAL_ATTEMPTS))
                .map_err(|_| StoreError::Flash)?;
        }
        Ok(())
    }

    /// The hand-over to the boot core's update mode
    /// ([`crate::bl::request_update_mode`]); `Unsupported` (E_UNSUPPORTED)
    /// when the boot core receives on no link.
    fn request_update_mode(&mut self) -> Result<(), StoreError> {
        let receives = (self.chip.boot_core_receives)();
        crate::bl::request_update_mode(&mut self.flash, receives).map_err(|e| match e {
            crate::bl::HandOverError::Unsupported => StoreError::Unsupported,
            crate::bl::HandOverError::Log(_) => StoreError::Flash,
        })
    }

    /// One PEER_SEEN in the app log, the room of a trial kept after it
    /// (`Log::note_peer`: a compaction first when the face lacks it).
    fn note_peer(&mut self, peer: Peer) -> Result<(), StoreError> {
        let mut log = Log::load(&mut self.flash, &APP_LOG_LAYOUT).map_err(|_| StoreError::Flash)?;
        log.note_peer(&mut self.flash, peer, trial_room(TRIAL_ATTEMPTS))
            .map(|_| ())
            .map_err(|_| StoreError::Flash)
    }

    /// The peers the app log records; `None` with no valid face.
    fn peers(&mut self) -> Option<PeerLog> {
        Log::load(&mut self.flash, &APP_LOG_LAYOUT)
            .ok()
            .map(|l| l.peers)
    }

    /// Stage0's version mark in its region (`dagon_format::stage0`).
    fn stage0_version(&mut self) -> Option<Version> {
        stage0::find(STAGE0, STAGE0_SIZE, |a, b| self.flash.read(a, b).is_ok())
    }

    /// The boot core slots: the boot core settles its trial before it
    /// starts the app, so the confirmed core is the one running (slot A,
    /// as stage0 starts it, while the log names none).
    fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        match t {
            target::APP => Some(self.status()),
            target::BOOTCORE => {
                let (core, generation) = Log::load(&mut self.flash, &BL_LOG_LAYOUT)
                    .map_or((Default::default(), 0), |l| (l.core, l.generation));
                let heads = self.heads(target::BOOTCORE, &StatusResp::default().slots);
                let running = core.confirmed.map_or(slot::A, |c| c.slot);
                Some(core.status(&heads, running, generation))
            }
            _ => None,
        }
    }

    fn write_target_header(
        &mut self,
        t: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let (base, _, _) = region(t, slot)?;
        self.flash.write(base, header).map_err(flash_err)
    }

    fn read_target_header(
        &mut self,
        t: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        let (base, _, _) = region(t, slot)?;
        self.flash.read(base, out).map_err(flash_err)
    }

    fn set_target_pending(
        &mut self,
        t: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        match t {
            target::APP => self.append(kind::SLOT_PENDING, slot, header),
            target::BOOTCORE => {
                let at = image::ref_of(header, slot).ok_or(StoreError::Flash)?;
                let mut log =
                    Log::load(&mut self.flash, &BL_LOG_LAYOUT).map_err(|_| StoreError::Flash)?;
                log.reserve(&mut self.flash, trial_room(TRIAL_ATTEMPTS))
                    .map_err(|_| StoreError::Flash)?;
                log.append_core(&mut self.flash, kind::SLOT_PENDING, at, 0)
                    .map_err(|_| StoreError::Flash)
            }
            _ => Err(StoreError::Unsupported),
        }
    }
}
