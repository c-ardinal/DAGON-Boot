//! M5StopWatch (ESP32-S3) DAGON node: the dagon-agent on the rear UART0
//! (and USB Serial/JTAG for direct work), receiving into the idle OTA slot.
//!
//! - [`store::EspStore`]: the agent's `SlotStore` (OTA partitions, detached
//!   headers in the dagon partition, otadata)
//! - [`trial::TrialWatchdog`]: bounded self-feed while on trial, gated by
//!   [`liveness::Liveness`] (every task within its announced deadline)
//! - [`rom`]: the ENTER_ROM request (cooperative ROM download mode)
//! - `node` (Xtensa): `node::start` takes the peripherals DAGON owns and
//!   runs the link task (rear UART0 and USB Serial/JTAG, async) on a
//!   high-priority interrupt executor. Used by the `factory` binary here and
//!   by the app in `targets/esp32s3-app`.
//!
//! Build-time identity: `DAGON_PUBKEY` (path to the `.pub` file, or 64 hex
//! digits; unset: the build fails unless `DAGON_ALLOW_TEST_KEY=1` opts in to
//! the RFC 8032 test key, with a build warning) and
//! `DAGON_TARGET_TAG` (default `watch-s3`).
#![cfg_attr(not(test), no_std)]

pub mod liveness;
pub mod rom;
pub mod serve;
pub mod store;
pub mod trial;

#[cfg(target_arch = "xtensa")]
pub mod node;

use dagon_agent::Config;
use dagon_format::header::{mcu_id, port_kind};
use dagon_proto::payload::role;

/// Release public key the agent accepts (resolved by `build.rs`).
pub const PUBKEY: [u8; 32] = hex32(env!("DAGON_PUBKEY_HEX"));

/// Image header `target_tag` this node accepts (the topology node's tag).
pub const TARGET_TAG: [u8; 16] = tag16(match option_env!("DAGON_TARGET_TAG") {
    Some(t) => t,
    None => "watch-s3",
});

/// Agent identity; `mode` is `payload::mode::APP` or `AGENT`.
pub fn config(mode: u8) -> Config {
    Config {
        pubkey: PUBKEY,
        mcu_id: mcu_id::ESP32S3,
        port_kind: port_kind::VENDOR_STAGE2,
        target_tag: TARGET_TAG,
        role: role::NODE,
        mode,
        max_payload: dagon_proto::max_payload::UART as u16,
    }
}

/// Response frame size of [`refuse_set_pending_on_trial`].
pub const REFUSAL_LEN: usize = dagon_proto::frame_len(1);

/// E_STATE for SET_PENDING while the running slot is on trial; `None` lets
/// the request through to the agent. Writing a trial entry then would end
/// the trial watchdog window and could replace the fallback's Valid entry.
/// Going back from a trial needs no SET_PENDING: a REBOOT without CONFIRM
/// rolls back.
pub fn refuse_set_pending_on_trial<S: dagon_agent::SlotStore>(
    store: &mut S,
    request: &[u8],
    out: &mut [u8; REFUSAL_LEN],
) -> Option<usize> {
    use dagon_proto::{flags, Cmd, Frame, Status, HEADER_LEN};
    let (f, _) = Frame::decode(request, dagon_proto::max_payload::UART).ok()?;
    if f.cmd != Cmd::SetPending as u8 || f.flags & (flags::RESP | flags::EVENT) != 0 {
        return None;
    }
    let st = store.status();
    if st.slots.get(st.active as usize)?.state != dagon_proto::payload::SlotState::Trying {
        return None;
    }
    out[HEADER_LEN] = Status::State as u8;
    dagon_proto::finish(out, flags::RESP, f.seq, f.cmd, 1).ok()
}

const fn hex32(s: &str) -> [u8; 32] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("DAGON_PUBKEY: not hex"),
        }
    }
    let b = s.as_bytes();
    assert!(b.len() == 64, "DAGON_PUBKEY: need 64 hex digits");
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nib(b[2 * i]) << 4 | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

const fn tag16(s: &str) -> [u8; 16] {
    let b = s.as_bytes();
    assert!(b.len() <= 16, "DAGON_TARGET_TAG: at most 16 bytes");
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_factory;
