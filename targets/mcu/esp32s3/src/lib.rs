//! DAGON for the ESP32-S3 (MCU layer): kind V node behind the stock ESP-IDF
//! 2nd stage with app rollback.
//!
//! - [`store::EspStore`]: the agent's `SlotStore` (OTA partitions, detached
//!   headers in the dagon partition, otadata)
//! - [`trial::TrialWatchdog`] and [`liveness`]: no feeding before CONFIRM,
//!   then only while every task is within its announced deadline
//! - [`rom`], [`serve`]: ENTER_ROM and what follows a reply
//! - [`layout`]: the partition table (`partitions.csv`), [`otadata`],
//!   [`framer`]
//! - Xtensa only: `flash::EspFlash` (`Flash`), `boot::EspBootSelect`
//!   (`BootSelectV`), `sys` (`Clock`, `Watchdog`, `Reset`), `link::UartLink`
//!   (`Link`), and `node`: `node::start` runs the link task on a
//!   high-priority interrupt executor
//! - [`board::BoardDef`]: what a board crate supplies (bring-up, optional PMIC
//!   ROM entry); pins and wiring live there, not here
//!
//! Build-time identity: `DAGON_PUBKEY` (path to the `.pub` file, or 64 hex
//! digits; unset: the build fails unless `DAGON_ALLOW_TEST_KEY=1` opts in to
//! the RFC 8032 test key, with a build warning) and `DAGON_TARGET_TAG` (the
//! topology node tag; overrides the board crate's default).
#![cfg_attr(not(test), no_std)]

pub mod board;
pub mod framer;
pub mod layout;
pub mod links;
pub mod liveness;
pub mod otadata;
pub mod rom;
pub mod serve;
pub mod store;
pub mod trial;

#[cfg(target_arch = "xtensa")]
pub mod boot;
#[cfg(target_arch = "xtensa")]
pub mod flash;
#[cfg(target_arch = "xtensa")]
pub mod link;
#[cfg(target_arch = "xtensa")]
pub mod node;
#[cfg(target_arch = "xtensa")]
pub mod sys;

/// Trial watchdog: an unconfirmed app is reset after this long without a
/// feed, and the 2nd stage then rolls back to the previous slot.
pub const TRIAL_WDT_MS: u32 = 4000;

/// A panic resets the chip: a hung image would never let the 2nd stage roll
/// back. Turn off the `panic-reset` feature to bring your own handler.
#[cfg(all(target_arch = "xtensa", feature = "panic-reset", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    esp_hal::system::software_reset()
}

use dagon_agent::Config;
use dagon_format::header::{mcu_id, port_kind};
/// Image version (re-exported for `node::running_version`).
pub use dagon_format::Version;
use dagon_proto::payload::role;

/// Release public key the agent accepts (resolved by `build.rs`).
pub const PUBKEY: [u8; 32] = hex32(env!("DAGON_PUBKEY_HEX"));

/// `DAGON_TARGET_TAG` at build time, if set: it overrides the board's tag.
pub const TARGET_TAG_OVERRIDE: Option<&str> = option_env!("DAGON_TARGET_TAG");

/// Image header `target_tag` this node accepts (the topology node's tag):
/// `DAGON_TARGET_TAG` if set at build time, else the board's default.
pub const fn target_tag(board_default: &str) -> [u8; 16] {
    tag16(match TARGET_TAG_OVERRIDE {
        Some(t) => t,
        None => board_default,
    })
}

/// Agent identity; `mode` is `payload::mode::APP` or `AGENT`.
pub fn config(mode: u8, target_tag: [u8; 16]) -> Config {
    Config {
        pubkey: PUBKEY,
        mcu_id: mcu_id::ESP32S3,
        port_kind: port_kind::VENDOR_STAGE2,
        target_tag,
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
    assert!(b.len() <= 16, "target tag: at most 16 bytes");
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
