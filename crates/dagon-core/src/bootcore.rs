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
use dagon_format::header::{header_digest16, image_kind, verify_header, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{
    kind, reason, slot, target, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE,
};
use dagon_format::{Sha256, SigVerifier};
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
    let max = max.min(MAX_COUNT);
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
            reason: if tried >= max.min(MAX_COUNT) {
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

/// Largest count a cell word holds; stage0 gives a pending core at most
/// this many starts whatever the definition asks.
pub const MAX_COUNT: u8 = 0xF;

/// One cell word for start `n` of image `p`: the tag, the first two bytes
/// of the image's header digest, the count and its complement in the low
/// byte (`n << 4 | !n & 0xF`).
const fn word(p: &SlotRef, n: u8) -> u32 {
    let n = (if n > MAX_COUNT { MAX_COUNT } else { n }) as u32;
    CELL_TAG | (p.digest16[0] as u32) << 16 | (p.digest16[1] as u32) << 8 | n << 4 | (!n & 0xF)
}

/// The trial cell for start `attempt` of image `p`: the same self-checking
/// word twice. Each word is one store, so a reset between the two leaves
/// one new and one old word; the cell counts the larger, so an update cut
/// half-way never lowers the count. Retained memory reads as anything
/// after power-up; a word reads as a count for `p` only when its tag,
/// digest bytes and count check all hold.
pub const fn trial_cell(p: &SlotRef, attempt: u8) -> [u32; 2] {
    let w = word(p, attempt);
    [w, w]
}

/// Starts the cell counts for image `p` (0 for anything else): the larger
/// of its two words' counts.
pub fn tried(cell: [u32; 2], p: &SlotRef) -> u8 {
    let ours = word(p, 0) & !0xFF;
    cell.into_iter()
        .filter(|&w| w & !0xFF == ours && counts(w))
        .map(|w| ((w >> 4) & 0xF) as u8)
        .max()
        .unwrap_or(0)
}

/// `w` is a cell word holding a count (for some image): its tag and its
/// count check hold.
const fn counts(w: u32) -> bool {
    w & 0xFF00_0000 == CELL_TAG && w & 0xF == !(w >> 4) & 0xF
}

/// The cell value that counts nothing (written once a trial is settled).
pub const CLEARED: [u32; 2] = [0, 0];

/// A value no trial cell word takes (its tag is not the cell's), written
/// and read back to see the cell hold values.
const PROBE: [u32; 2] = [0x5AA5_C33C, 0x0FF0_A55A];

/// Whether the cell holds what is written to it. A cell that does not (no
/// such register, a stuck or write-protected one) cannot count a trial's
/// starts, so the MCU package refuses boot core updates and a pending core
/// rolls itself back ([`settle_log`]). Keeping the value across a reset is
/// the vendor's documented behaviour and is checked on the board, not here.
///
/// A cell with a word that holds a count was written by stage0, so it
/// holds, and the check writes nothing: a probe over a count, cut by a
/// reset before the count is back, would leave the cell counting 0 and
/// let stage0 start a failing core again and again. Only a cell that
/// counts nothing gets the probe (a value no cell word takes), then the
/// value it had, each read back, one word per `write(index, word)` as the
/// MCUs store them; a reset in between leaves it counting nothing, as
/// before.
pub fn cell_holds(read: impl Fn() -> [u32; 2], mut write: impl FnMut(usize, u32)) -> bool {
    let was = read();
    if was.into_iter().any(counts) {
        return true;
    }
    let mut put = |v: [u32; 2]| v.into_iter().enumerate().for_each(|(i, w)| write(i, w));
    put(PROBE);
    let probe_held = read() == PROBE;
    put(was);
    probe_held && read() == was
}

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

/// Why [`check_inline`] refused a boot core image. Each reason has its own
/// code ([`CoreFail::code`]), the one stage0 hands its diagnostic output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CoreFail {
    /// Header unreadable or not a valid header.
    Header = 1,
    /// Header digest is not the one the log record names.
    Digest = 2,
    /// Body larger than the slot.
    Size = 3,
    /// Not a boot core for this MCU bound to this slot.
    Kind = 4,
    /// `load_addr` is not this slot's body.
    LoadAddr = 5,
    /// Body digest differs from the header's.
    Body = 6,
    /// The header's signature does not verify under stage0's verifier.
    Signature = 7,
}

impl CoreFail {
    /// The reason's code (1 to 7).
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// Stage0's check of the boot core in the slot at `base` (`size` bytes,
/// header inline at the start) as slot `slot`: its header parses and its
/// signature verifies under `verifier` (stage0's, holding the key at the
/// place its MCU definition names), it is the header `expect` names (when a
/// record names one), it is a boot core for MCU `mcu_id` bound to this
/// slot, linked for the slot's body, fitting, and its body hashes to the
/// header's digest. Returns the body address.
pub fn check_inline<F: Flash, V: SigVerifier + ?Sized>(
    flash: &mut F,
    (base, size): (u32, u32),
    slot: u8,
    (expect, verifier): (Option<&SlotRef>, &V),
    mcu_id: u16,
) -> Result<u32, CoreFail> {
    let mut raw = [0u8; HEADER_SIZE];
    flash.read(base, &mut raw).map_err(|_| CoreFail::Header)?;
    // Parsed first, so a slot holding no core reads as one, not as a
    // signature that does not verify.
    ImageHeader::parse(&raw).map_err(|_| CoreFail::Header)?;
    let h = verify_header(&raw, verifier).map_err(|_| CoreFail::Signature)?;
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
/// record, while the log names none). When that one does not check out
/// either (a corrupt slot, or a log naming a slot that holds no core), the
/// other slots are tried on their own (checked against no record), A
/// before B: a core whose image checks out is always preferred to halting.
/// Only when no slot checks out is there nothing to start. `check(slot,
/// record)` is the MCU's image check of the core in `slot`, against the
/// record when one names it. `slots` is how many boot core slots the MCU
/// definition gives (slot A, then B; at least one): a record naming any
/// other slot is left out as if the log did not hold it, and no other slot
/// is ever checked.
pub fn choose<B, E>(
    st: &LogState,
    cell: [u32; 2],
    max: u8,
    slots: u8,
    mut check: impl FnMut(u8, Option<&SlotRef>) -> Result<B, E>,
) -> Result<Start<B>, NoCore<E>> {
    let inside = |r: &SlotRef| r.slot < slots;
    let st = &LogState {
        pending: st.pending.filter(inside),
        confirmed: st.confirmed.filter(inside),
        ..*st
    };
    let n = st.pending.map_or(0, |p| tried(cell, &p));
    let mut tried_slot = Tried::default();
    let run = match plan(st, n, max) {
        Plan::Try {
            pending,
            attempt,
            fallback,
        } => {
            mark(&mut tried_slot, pending.slot);
            match check(pending.slot, Some(&pending)) {
                Ok(body) => {
                    return Ok(Start {
                        slot: pending.slot,
                        body,
                        trial: true,
                        cell: trial_cell(&pending, attempt),
                    })
                }
                Err(_) => fallback,
            }
        }
        Plan::Run(c) => c,
    };
    // The count stays while a core is pending, so the core that runs can
    // tell why stage0 gave the pending one up.
    let kept = if st.pending.is_some() { cell } else { CLEARED };
    let first = run.map_or(slot::A, |c| c.slot);
    mark(&mut tried_slot, first);
    let why = match check(first, run.as_ref()) {
        Ok(body) => {
            return Ok(Start {
                slot: first,
                body,
                trial: false,
                cell: kept,
            })
        }
        Err(why) => why,
    };
    for s in 0..slots {
        if tried_slot.has(s) {
            continue;
        }
        if let Ok(body) = check(s, None) {
            return Ok(Start {
                slot: s,
                body,
                trial: false,
                cell: kept,
            });
        }
    }
    // The pending core that ran out of starts, as the very last resort.
    if let Some(p) = st.pending.filter(|p| p.slot != first) {
        if let Ok(body) = check(p.slot, Some(&p)) {
            return Ok(Start {
                slot: p.slot,
                body,
                trial: false,
                cell: kept,
            });
        }
    }
    Err(NoCore {
        named: run.is_some(),
        why,
    })
}

/// Where stage0 tells why it started no core: an output the board may
/// give (an indicator) or not. Stage0 decides the same whether it is there
/// or not; it hears only the reason's code ([`CoreFail::code`]) once the
/// decision is made. An output that shows the code until the next reset
/// need not return.
pub trait Diag {
    fn report(&mut self, code: u8);
}

/// No diagnostic output: a node whose board gives none, or a node without
/// a board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoDiag;

impl Diag for NoDiag {
    fn report(&mut self, _: u8) {}
}

/// Shows `code` on an indicator until reset, for a board whose halt pattern
/// blinks every `period_ms`: `code` blinks five times faster than that
/// pattern, so a stage0 halt is told from the boot core's, then dark for
/// one period, and again. `set` lights or darkens the indicator; `wait_ms`
/// waits. A period below 10 ms would make each step no delay: the
/// definition loader refuses such a board for an MCU with a stage0.
pub fn blink_code(
    code: u8,
    period_ms: u32,
    mut set: impl FnMut(bool),
    mut wait_ms: impl FnMut(u32),
) -> ! {
    let half = period_ms / 10;
    loop {
        for _ in 0..code {
            set(true);
            wait_ms(half);
            set(false);
            wait_ms(half);
        }
        wait_ms(period_ms);
    }
}

/// What stage0 knows of its MCU: the boot core log, the boot core slots
/// (`(base, size)`, slot A first), the starts a pending core gets
/// (`trial_attempts`) and the MCU id a core's header names.
#[derive(Clone, Copy, Debug)]
pub struct Stage0<'a> {
    pub log: &'a LogLayout,
    pub slots: &'a [(u32, u32)],
    pub max: u8,
    pub mcu_id: u16,
}

/// Stage0's decision, the same on every MCU: [`choose`] over the boot core
/// log at `at.log` and the trial cell `cell` read at reset, where every
/// core it may start, on whichever path, passes [`check_inline`] under
/// `verifier`; a core whose signature does not verify never starts. When
/// no core passes, it reports the reason to `diag` and returns it: the
/// caller starts nothing and waits for recovery.
pub fn decide<F: Flash, V: SigVerifier + ?Sized, D: Diag>(
    flash: &mut F,
    at: &Stage0<'_>,
    cell: [u32; 2],
    verifier: &V,
    diag: &mut D,
) -> Result<Start<u32>, NoCore<CoreFail>> {
    let st = read_state(flash, at.log).unwrap_or_default();
    let slots = u8::try_from(at.slots.len()).unwrap_or(u8::MAX);
    let chosen = choose(&st, cell, at.max, slots, |s, r| {
        let slot = *at.slots.get(usize::from(s)).ok_or(CoreFail::Kind)?;
        check_inline(flash, slot, s, (r, verifier), at.mcu_id)
    });
    if let Err(no) = &chosen {
        diag.report(no.why.code());
    }
    chosen
}

/// The slots [`choose`] has checked, as bits by slot number.
#[derive(Default)]
struct Tried(u32);

impl Tried {
    fn has(&self, s: u8) -> bool {
        self.0.checked_shr(u32::from(s)).is_some_and(|b| b & 1 == 1)
    }
}

fn mark(tried: &mut Tried, s: u8) {
    tried.0 |= 1u32.checked_shl(u32::from(s)).unwrap_or(0);
}

/// The trial cell as the boot core sees it at start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Its value.
    pub value: [u32; 2],
    /// Starts a pending core gets (`trial_attempts`).
    pub max: u8,
    /// It holds what is written to it ([`cell_holds`]).
    pub holds: bool,
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
/// Clear the trial cell after `Confirmed` or `RolledBack`. It never erases
/// flash: the record goes into the room the agent reserved when it set the
/// core pending ([`Log::reserve`]); a log with no room (a torn tail) takes
/// no record, which ends as a reset (confirm) or a retry at the next start
/// (rollback), never as an erase under the trial watchdog. A pending core
/// whose trial cell does not hold (`cell.holds` false: stage0 could not
/// count its starts, so a broken core would be retried for ever) records
/// its own ROLLBACK to the confirmed core and resets.
pub fn settle_log<F: Flash>(
    flash: &mut F,
    layout: &LogLayout,
    running: u8,
    cell: Cell,
    healthy: bool,
) -> Result<Settled, MustReset> {
    let Cell {
        value: cell,
        max,
        holds,
    } = cell;
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
            if !holds {
                if let Some(c) = log.core.confirmed {
                    let back = SlotRef { slot: c.slot, ..p };
                    let why = reason::TRIAL_EXHAUSTED;
                    let _ = log.append_in_place(flash, target::BOOTCORE, kind::ROLLBACK, back, why);
                }
                return Err(MustReset);
            }
            if !healthy {
                return Err(MustReset);
            }
            log.append_in_place(flash, target::BOOTCORE, kind::CONFIRMED, p, reason::NONE)
                .map_err(|_| MustReset)?;
            let back = Log::load(flash, layout).map_err(|_| MustReset)?;
            if back.core.confirmed != Some(p) || back.core.pending.is_some() {
                return Err(MustReset);
            }
            Ok(Settled::Confirmed)
        }
        Settle::Rollback { back, reason } => {
            match log.append_in_place(flash, target::BOOTCORE, kind::ROLLBACK, back, reason) {
                Ok(()) => Ok(Settled::RolledBack),
                Err(_) => Ok(Settled::Nothing),
            }
        }
    }
}
