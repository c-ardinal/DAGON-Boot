//! Boot core A/B on a self-boot MCU with a stage0.
//!
//! The app's agent writes a new boot core into the idle boot core slot and
//! records it pending (SLOT_PENDING, target BOOTCORE) after verifying its
//! signature. Stage0, which is never updated and never writes flash, starts
//! the pending core on trial while the trial cell (two words of retained
//! memory, [`trial_cell`]) counts fewer than `max` starts for that image;
//! otherwise it starts the confirmed core. The core that runs settles the
//! trial in the log ([`settle`]): the pending core confirms itself after
//! its own check; the old core, started while another core is pending,
//! records the rollback. A core that cannot settle resets, so stage0 counts
//! the start and, after `max`, falls back.
//!
//! The count lives in retained memory, not in the log, so stage0 needs no
//! flash writer: a reset keeps it, a power loss clears it and the trial
//! starts over, which always ends at a core that runs.

use crate::log::{Log, LogError, LogLayout, LogState, SlotRef};
use dagon_format::header::{header_digest16, image_kind, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{
    kind, reason, slot, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE,
};
use dagon_format::Sha256;
use dagon_port::Flash;

/// What stage0 starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The pending core, as start number `attempt` (write it to the trial
    /// cell before the jump); `fallback` when its image does not check out.
    Try {
        pending: SlotRef,
        attempt: u8,
        fallback: Option<SlotRef>,
    },
    /// The confirmed core; `None`: the log names none (slot A, unchecked
    /// against a record).
    Run(Option<SlotRef>),
}

/// Stage0's plan from the boot core fold `st` and the starts `tried` the
/// trial cell counts for the pending core.
pub fn plan(st: &LogState, tried: u8, max: u8) -> Plan {
    match st.pending {
        Some(p) if tried < max => Plan::Try {
            pending: p,
            attempt: tried + 1,
            fallback: st.confirmed,
        },
        _ => Plan::Run(st.confirmed),
    }
}

/// What the running boot core records at start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settle {
    /// No trial open.
    Nothing,
    /// This core is the pending one: CONFIRMED after its own check passes.
    Confirm(SlotRef),
    /// Another core is pending and stage0 started this one: ROLLBACK naming
    /// the slot the boot went back to (as the app log's ROLLBACK does).
    Rollback { back: SlotRef, reason: u8 },
}

/// The record the boot core running from slot `running` writes, given the
/// boot core fold and the starts the trial cell counts.
pub fn settle(st: &LogState, running: u8, tried: u8, max: u8) -> Settle {
    match st.pending {
        Some(p) if p.slot == running => Settle::Confirm(p),
        Some(p) => Settle::Rollback {
            back: SlotRef { slot: running, ..p },
            reason: if tried >= max {
                reason::TRIAL_EXHAUSTED
            } else {
                reason::HASH_MISMATCH
            },
        },
        None => Settle::Nothing,
    }
}

/// Marks a trial cell word.
const CELL_TAG: u32 = 0xB7 << 24;

/// The trial cell for start `attempt` of image `p`: a word holding a tag,
/// the first two bytes of the image's header digest and the count, then its
/// complement. Retained memory reads as anything after power-up; only the
/// pair written for `p` reads as a count for it.
pub const fn trial_cell(p: &SlotRef, attempt: u8) -> [u32; 2] {
    let w = CELL_TAG | (p.digest16[0] as u32) << 16 | (p.digest16[1] as u32) << 8 | attempt as u32;
    [w, !w]
}

/// Starts the cell counts for image `p` (0 for anything else).
pub fn tried(cell: [u32; 2], p: &SlotRef) -> u8 {
    let [w, check] = cell;
    let ours = trial_cell(p, 0)[0];
    if check != !w || w & !0xFF != ours {
        return 0;
    }
    w as u8
}

/// The cell value that counts nothing (written once a trial is settled).
pub const CLEARED: [u32; 2] = [0, 0];

/// The boot core records of the log at `layout`, folded as [`Log::load`]
/// folds them (newest valid face, records up to the first erased or
/// corrupt one), without the writer's state: the part of `Log` a stage0
/// with a tight size budget needs. `None`: no valid face.
pub fn read_state<F: Flash>(flash: &mut F, layout: &LogLayout) -> Option<LogState> {
    let mut best: Option<(u32, u32)> = None;
    for face in layout.faces {
        let mut h = [0u8; FACE_HEADER_SIZE];
        flash.read(face, &mut h).ok()?;
        if let Ok(fh) = FaceHeader::parse(&h) {
            if best.is_none_or(|(_, g)| fh.generation > g) {
                best = Some((face, fh.generation));
            }
        }
    }
    let (face, _) = best?;
    let mut st = LogState::default();
    let mut at = face + FACE_HEADER_SIZE as u32;
    while at + RECORD_SIZE as u32 <= face + layout.face_len {
        let mut b = [0u8; RECORD_SIZE];
        flash.read(at, &mut b).ok()?;
        let Ok(r) = Record::parse(&b) else { break };
        st = st.apply_core(&r);
        at += RECORD_SIZE as u32;
    }
    Some(st)
}

/// Why [`check_inline`] refused a boot core image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreFail {
    /// Header unreadable or not a valid header.
    Header,
    /// Header digest is not the one the log record names.
    Digest,
    /// Body larger than the slot.
    Size,
    /// Not a boot core for this MCU bound to this slot.
    Kind,
    /// `load_addr` is not this slot's body.
    LoadAddr,
    /// Body digest differs from the header's.
    Body,
}

/// Stage0's check of the boot core in the slot at `base` (`size` bytes,
/// header inline at the start) as slot `slot`: its header is the one
/// `expect` names (when a record names one), it is a boot core for MCU
/// `mcu_id` bound to this slot, linked for the slot's body, fitting, and
/// its body hashes to the header's digest. No signature: the agent checked
/// it before the record was written. Returns the body address.
pub fn check_inline<F: Flash>(
    flash: &mut F,
    (base, size): (u32, u32),
    slot: u8,
    expect: Option<&SlotRef>,
    mcu_id: u16,
) -> Result<u32, CoreFail> {
    let mut raw = [0u8; HEADER_SIZE];
    flash.read(base, &mut raw).map_err(|_| CoreFail::Header)?;
    let h = ImageHeader::parse(&raw).map_err(|_| CoreFail::Header)?;
    if expect.is_some_and(|r| r.digest16 != header_digest16(&raw)) {
        return Err(CoreFail::Digest);
    }
    let body = base + HEADER_SIZE as u32;
    if h.payload_size > size - HEADER_SIZE as u32 {
        return Err(CoreFail::Size);
    }
    if h.image_kind != image_kind::BOOTCORE || h.mcu_id != mcu_id || h.slot_binding != slot {
        return Err(CoreFail::Kind);
    }
    if h.load_addr != body {
        return Err(CoreFail::LoadAddr);
    }
    let mut sha = Sha256::new();
    let mut buf = [0u8; 256];
    let mut done = 0u32;
    while done < h.payload_size {
        let n = (h.payload_size - done).min(buf.len() as u32) as usize;
        flash
            .read(body + done, &mut buf[..n])
            .map_err(|_| CoreFail::Body)?;
        sha.update(&buf[..n]);
        done += n as u32;
    }
    if sha.finalize() != h.payload_sha256 {
        return Err(CoreFail::Body);
    }
    Ok(body)
}

/// The boot core stage0 starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Start<B> {
    pub slot: u8,
    /// What the MCU's check returned for it (its body address).
    pub body: B,
    /// The pending core, on trial.
    pub trial: bool,
    /// The trial cell to leave for the next reset.
    pub cell: [u32; 2],
}

/// Stage0 has nothing to start: whether the log named the core it checked,
/// and why the MCU's check refused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoCore<E> {
    pub named: bool,
    pub why: E,
}

/// Stage0's choice from the boot core fold `st` and the trial cell `cell`
/// read at reset: the pending core while its starts last and `check`
/// accepts its image, else the confirmed one (slot A, checked against no
/// record, while the log names none). `check` is the MCU's image check of
/// the core a record names (`None`: slot A on its own).
pub fn choose<B, E>(
    st: &LogState,
    cell: [u32; 2],
    max: u8,
    mut check: impl FnMut(Option<&SlotRef>) -> Result<B, E>,
) -> Result<Start<B>, NoCore<E>> {
    let n = st.pending.map_or(0, |p| tried(cell, &p));
    let run = match plan(st, n, max) {
        Plan::Try {
            pending,
            attempt,
            fallback,
        } => match check(Some(&pending)) {
            Ok(body) => {
                return Ok(Start {
                    slot: pending.slot,
                    body,
                    trial: true,
                    cell: trial_cell(&pending, attempt),
                })
            }
            Err(_) => fallback,
        },
        Plan::Run(c) => c,
    };
    let body = check(run.as_ref()).map_err(|why| NoCore {
        named: run.is_some(),
        why,
    })?;
    Ok(Start {
        slot: run.map_or(slot::A, |c| c.slot),
        body,
        trial: false,
        // The count stays while a core is pending, so the core that runs
        // can tell why stage0 gave the pending one up.
        cell: if st.pending.is_some() { cell } else { CLEARED },
    })
}

/// What the boot core recorded about the boot core trial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    Nothing,
    /// This core was pending, passed its own check and confirmed itself.
    Confirmed,
    /// Another core was pending; this one recorded its rollback.
    RolledBack,
}

/// The core must not start the app (it is on trial and failed its own
/// check, or its confirmation did not land): it resets, and stage0 counts
/// the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MustReset;

/// The boot core running from slot `running` settles the boot core trial
/// in the log at `layout` before it starts the app: the pending core
/// confirms itself only when `healthy` (its own check passed) and the
/// record reads back; the old core records the rollback, and runs the app
/// even when that record does not land (its next start writes it again).
/// Clear the trial cell after `Confirmed` or `RolledBack`.
pub fn settle_log<F: Flash>(
    flash: &mut F,
    layout: &LogLayout,
    running: u8,
    (cell, max): ([u32; 2], u8),
    healthy: bool,
) -> Result<Settled, MustReset> {
    let mut log = match Log::load(flash, layout) {
        Ok(l) => l,
        // No boot core record at all: stage0 started slot A unchecked.
        Err(LogError::NoValidFace) => return Ok(Settled::Nothing),
        Err(_) => return Err(MustReset),
    };
    let n = log.core.pending.map_or(0, |p| tried(cell, &p));
    match settle(&log.core, running, n, max) {
        Settle::Nothing => Ok(Settled::Nothing),
        Settle::Confirm(p) => {
            if !healthy {
                return Err(MustReset);
            }
            log.append_core(flash, kind::CONFIRMED, p, reason::NONE)
                .map_err(|_| MustReset)?;
            let back = Log::load(flash, layout).map_err(|_| MustReset)?;
            if back.core.confirmed != Some(p) || back.core.pending.is_some() {
                return Err(MustReset);
            }
            Ok(Settled::Confirmed)
        }
        Settle::Rollback { back, reason } => {
            match log.append_core(flash, kind::ROLLBACK, back, reason) {
                Ok(()) => Ok(Settled::RolledBack),
                Err(_) => Ok(Settled::Nothing),
            }
        }
    }
}
