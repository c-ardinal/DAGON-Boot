//! Kind S boot decision (it never erases: records go into the room the
//! agent reserved, `Log::reserve`): try the pending slot (a BOOT_ATTEMPT
//! before every start, rollback once the attempts run out), otherwise start
//! the confirmed slot. A log with no valid face is rebuilt from the slots
//! first ([`rebuild`], the one erase of the boot path: no record is left
//! to lose). Every slot is checked before it starts: its header is
//! the one the log names, its signature verifies, and its body matches the
//! header's SHA-256. The log carries no key, so the signature is checked at
//! every boot, not only on trial.

use crate::log::{Log, LogError, LogLayout, SlotRef};
use dagon_format::header::{
    header_digest16, image_kind, placement, verify_header, ImageHeader, HEADER_SIZE,
};
use dagon_format::statelog::{kind, reason, target};
use dagon_format::SigVerifier;
use dagon_port::Flash;

/// Where one slot's image lies, as its MCU definition places the header:
/// the 512-byte header at `header`, the body at `body` with room for
/// `room` bytes. An inline header starts the slot and the body follows it;
/// a detached header has a sector of its own (a partition the MCU
/// definition names). The image's header names the same placement and links the
/// body for `body`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub header: u32,
    pub body: u32,
    pub room: u32,
    /// `placement::INLINE` or `placement::DETACHED`.
    pub placement: u8,
}

impl Place {
    /// A slot of `size` bytes at `base` whose header is inline.
    pub const fn inline(base: u32, size: u32) -> Self {
        Place {
            header: base,
            body: base + HEADER_SIZE as u32,
            room: size.saturating_sub(HEADER_SIZE as u32),
            placement: placement::INLINE,
        }
    }

    /// A body region of `size` bytes at `body` whose header is at `header`.
    pub const fn detached(header: u32, body: u32, size: u32) -> Self {
        Place {
            header,
            body,
            room: size,
            placement: placement::DETACHED,
        }
    }

    /// Whether header `h` was made for this place: the same placement, the
    /// body linked for `body` and fitting the room.
    pub fn fits(&self, h: &dagon_format::header::ImageHeader) -> bool {
        h.placement == self.placement && h.load_addr == self.body && h.payload_size <= self.room
    }
}

/// The places of the generated MCU definition's slots (`slots::<KIND>_PLACES`:
/// header, body, body room, placement byte).
pub const fn places<const N: usize>(rows: [(u32, u32, u32, u8); N]) -> [Place; N] {
    let mut out = [Place::inline(0, 0); N];
    let mut i = 0;
    while i < N {
        let (header, body, room, placement) = rows[i];
        out[i] = Place {
            header,
            body,
            room,
            placement,
        };
        i += 1;
    }
    out
}

/// Where the APP slots live, index = slot (A, then B when there is one);
/// each slot has its own place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotLayout<'a> {
    pub slots: &'a [Place],
}

/// What this boot core accepts (the signature key lives in the verifier
/// [`decide`] is given).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootConfig {
    pub mcu_id: u16,
    pub target_tag: [u8; 16],
    pub max_attempts: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Start the body at `entry`; `trial` selects the trial watchdog.
    Boot { slot: u8, entry: u32, trial: bool },
    /// No slot can be started.
    NoImage,
}

/// Decides what to start, appending BOOT_ATTEMPT or ROLLBACK to `log`.
/// `verifier` checks the signature of every image before it starts, once
/// per slot considered; a [`Decision::Boot`] is an image it accepted.
pub fn decide<F: Flash, V: SigVerifier + ?Sized>(
    flash: &mut F,
    log: &mut Log,
    slots: &SlotLayout,
    cfg: &BootConfig,
    verifier: &V,
) -> Decision {
    if let Some(p) = log.state.pending {
        if let Some(d) = try_pending(flash, log, slots, cfg, verifier, p) {
            return d;
        }
    }
    let Some(c) = log.state.confirmed else {
        return Decision::NoImage;
    };
    match check(flash, slots, cfg, verifier, c) {
        Ok(entry) => Decision::Boot {
            slot: c.slot,
            entry,
            trial: false,
        },
        Err(_) => Decision::NoImage,
    }
}

/// `None` falls back to the confirmed slot.
fn try_pending<F: Flash, V: SigVerifier + ?Sized>(
    flash: &mut F,
    log: &mut Log,
    slots: &SlotLayout,
    cfg: &BootConfig,
    verifier: &V,
    p: SlotRef,
) -> Option<Decision> {
    let why = if log.state.attempts >= cfg.max_attempts {
        reason::TRIAL_EXHAUSTED
    } else {
        match check(flash, slots, cfg, verifier, p) {
            // Counted before the jump, so a crash in the new image still counts.
            // If the log cannot take the record, no trial starts.
            Ok(entry) => {
                return log
                    .append_in_place(flash, target::APP, kind::BOOT_ATTEMPT, p, reason::NONE)
                    .ok()
                    .map(|_| Decision::Boot {
                        slot: p.slot,
                        entry,
                        trial: true,
                    })
            }
            Err(why) => why,
        }
    };
    // ROLLBACK names the slot the boot goes back to. With no confirmed slot
    // there is none, so nothing is recorded (and nothing starts). A ROLLBACK
    // the log cannot take is retried on the next boot.
    if let Some(c) = log.state.confirmed {
        let back = SlotRef { slot: c.slot, ..p };
        let _ = log.append_in_place(flash, target::APP, kind::ROLLBACK, back, why);
    }
    None
}

/// Checks the slot image `r` refers to; returns the body address or the
/// state log reason code of the failure.
fn check<F: Flash, V: SigVerifier + ?Sized>(
    flash: &mut F,
    slots: &SlotLayout,
    cfg: &BootConfig,
    verifier: &V,
    r: SlotRef,
) -> Result<u32, u8> {
    let at = *slots
        .slots
        .get(r.slot as usize)
        .ok_or(reason::NO_VALID_IMAGE)?;
    let mut raw = [0u8; HEADER_SIZE];
    flash
        .read(at.header, &mut raw)
        .map_err(|_| reason::HASH_MISMATCH)?;
    if header_digest16(&raw) != r.digest16 {
        return Err(reason::HASH_MISMATCH);
    }
    let h = verify_header(&raw, verifier).map_err(|_| reason::SIG_INVALID)?;
    let body = at.body;
    let fits = h.image_kind == image_kind::APP
        && at.fits(&h)
        && h.mcu_id == cfg.mcu_id
        && h.target_tag == cfg.target_tag
        && h.slot_binding == r.slot;
    if !fits {
        return Err(reason::SIG_INVALID);
    }
    let digest = flash
        .sha256(body, h.payload_size)
        .map_err(|_| reason::HASH_MISMATCH)?;
    if digest != h.payload_sha256 {
        return Err(reason::HASH_MISMATCH);
    }
    Ok(body)
}

/// The records [`rebuild`] writes for the APP images that pass the boot
/// checks (`passed`, by slot): with two, PROVISION for the older one (the
/// one the node ran before the newer one was written) and SLOT_PENDING for
/// the newer one, so the newer one starts on trial and falls back to the
/// older one; with two of the same version, PROVISION for the lower slot
/// alone; with one, SLOT_PENDING alone (an image no log confirmed starts
/// only on trial, and a failed trial ends in recovery mode); with none,
/// nothing.
pub fn rebuilt_records(passed: [Option<SlotRef>; 2]) -> ([(u8, SlotRef); 2], usize) {
    let none = (kind::EMPTY, crate::log::NO_SLOT);
    match passed {
        [Some(a), Some(b)] if a.version == b.version => ([(kind::PROVISION, a), none], 1),
        [Some(a), Some(b)] => {
            let (old, new) = if a.version < b.version {
                (a, b)
            } else {
                (b, a)
            };
            ([(kind::PROVISION, old), (kind::SLOT_PENDING, new)], 2)
        }
        [Some(only), None] | [None, Some(only)] => ([(kind::SLOT_PENDING, only), none], 1),
        [None, None] => ([none, none], 0),
    }
}

/// Rebuilds a state log that has no valid face (both faces corrupt) from
/// the APP slots, so the node starts the images it holds without a new
/// transfer: every slot whose image passes the checks a start makes
/// (header, signature, kind, MCU, node tag, slot binding, place, body)
/// is recorded as [`rebuilt_records`] says, in a new face
/// ([`Log::create`], its header last). A log with a valid face is
/// returned as it is, nothing written; with no image passing, nothing is
/// written and the log stays lost (`LogError::NoValidFace`: recovery
/// mode receives the app).
pub fn rebuild<F: Flash, V: SigVerifier + ?Sized>(
    flash: &mut F,
    layout: &LogLayout,
    slots: &SlotLayout,
    cfg: &BootConfig,
    verifier: &V,
) -> Result<Log, LogError> {
    match Log::load(flash, layout) {
        Err(LogError::NoValidFace) => {}
        other => return other,
    }
    let mut passed = [None; 2];
    for (s, out) in passed.iter_mut().enumerate() {
        let Some(at) = slots.slots.get(s) else { break };
        let mut raw = [0u8; HEADER_SIZE];
        if flash.read(at.header, &mut raw).is_err() {
            continue;
        }
        let Ok(h) = ImageHeader::parse(&raw) else {
            continue;
        };
        let r = SlotRef {
            slot: s as u8,
            version: h.version,
            digest16: header_digest16(&raw),
        };
        *out = check(flash, slots, cfg, verifier, r).ok().map(|_| r);
    }
    let (records, n) = rebuilt_records(passed);
    if n == 0 {
        return Err(LogError::NoValidFace);
    }
    Log::create(flash, layout, &records[..n])
}
