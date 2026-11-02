//! Kind S boot decision: try the pending slot (signature and SHA-256, a
//! BOOT_ATTEMPT before every start, rollback once the attempts run out),
//! otherwise start the confirmed slot after checking its SHA-256 and that its
//! header is the one the log confirmed.

use crate::log::{Log, SlotRef};
use dagon_format::header::{
    header_digest16, image_kind, placement, verify_header, ImageHeader, HEADER_SIZE,
};
use dagon_format::statelog::{kind, reason, slot};
use dagon_format::Sha256;
use dagon_port::Flash;

/// Where the APP slots live. Each slot starts with its 512-byte header; the
/// body (vector table first) follows at `+HEADER_SIZE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotLayout {
    pub slots: [u32; 2],
    pub slot_len: u32,
}

/// What this boot core accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootConfig {
    pub pubkey: [u8; 32],
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
pub fn decide<F: Flash>(
    flash: &mut F,
    log: &mut Log,
    slots: &SlotLayout,
    cfg: &BootConfig,
) -> Decision {
    if let Some(p) = log.state.pending {
        if let Some(d) = try_pending(flash, log, slots, cfg, p) {
            return d;
        }
    }
    let Some(c) = log.state.confirmed else {
        return Decision::NoImage;
    };
    match check(flash, slots, cfg, c, false) {
        Ok(entry) => Decision::Boot {
            slot: c.slot,
            entry,
            trial: false,
        },
        Err(_) => Decision::NoImage,
    }
}

/// `None` falls back to the confirmed slot.
fn try_pending<F: Flash>(
    flash: &mut F,
    log: &mut Log,
    slots: &SlotLayout,
    cfg: &BootConfig,
    p: SlotRef,
) -> Option<Decision> {
    let why = if log.state.attempts >= cfg.max_attempts {
        reason::TRIAL_EXHAUSTED
    } else {
        match check(flash, slots, cfg, p, true) {
            // Counted before the jump, so a crash in the new image still counts.
            // If the log cannot take the record, no trial starts.
            Ok(entry) => {
                return log
                    .append(flash, kind::BOOT_ATTEMPT, p, reason::NONE)
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
    let back = SlotRef {
        slot: log.state.confirmed.map_or(slot::NONE, |c| c.slot),
        ..p
    };
    // A ROLLBACK the log cannot take is retried on the next boot.
    let _ = log.append(flash, kind::ROLLBACK, back, why);
    None
}

/// Checks the slot image `r` refers to; returns the body address or the
/// state log reason code of the failure. `signature` adds the Ed25519 check
/// (pending slots); confirmed slots rely on the header digest the log holds.
fn check<F: Flash>(
    flash: &mut F,
    slots: &SlotLayout,
    cfg: &BootConfig,
    r: SlotRef,
    signature: bool,
) -> Result<u32, u8> {
    let base = *slots
        .slots
        .get(r.slot as usize)
        .ok_or(reason::NO_VALID_IMAGE)?;
    let mut raw = [0u8; HEADER_SIZE];
    flash
        .read(base, &mut raw)
        .map_err(|_| reason::HASH_MISMATCH)?;
    if header_digest16(&raw) != r.digest16 {
        return Err(reason::HASH_MISMATCH);
    }
    let h = if signature {
        verify_header(&raw, &cfg.pubkey).map_err(|_| reason::SIG_INVALID)?
    } else {
        ImageHeader::parse(&raw).map_err(|_| reason::HASH_MISMATCH)?
    };
    let fits = h.image_kind == image_kind::APP
        && h.mcu_id == cfg.mcu_id
        && h.target_tag == cfg.target_tag
        && h.slot_binding == r.slot
        && h.placement == placement::INLINE
        && h.payload_size <= slots.slot_len - HEADER_SIZE as u32;
    if !fits {
        return Err(reason::SIG_INVALID);
    }
    let body = base + HEADER_SIZE as u32;
    if body_sha256(flash, body, h.payload_size)? != h.payload_sha256 {
        return Err(reason::HASH_MISMATCH);
    }
    Ok(body)
}

fn body_sha256<F: Flash>(flash: &mut F, start: u32, len: u32) -> Result<[u8; 32], u8> {
    let mut sha = Sha256::new();
    let mut buf = [0u8; 256];
    let mut done = 0u32;
    while done < len {
        let n = (len - done).min(buf.len() as u32) as usize;
        flash
            .read(start + done, &mut buf[..n])
            .map_err(|_| reason::HASH_MISMATCH)?;
        sha.update(&buf[..n]);
        done += n as u32;
    }
    Ok(sha.finalize())
}
