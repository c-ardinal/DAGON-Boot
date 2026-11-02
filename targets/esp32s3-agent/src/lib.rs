//! M5StopWatch (ESP32-S3) DAGON node: the dagon-agent on the rear UART0
//! (and USB Serial/JTAG for direct work), receiving into the idle OTA slot.
//!
//! - [`store::EspStore`]: the agent's `SlotStore` (OTA partitions, detached
//!   headers in the dagon partition, otadata)
//! - [`trial::TrialWatchdog`]: bounded self-feed while on trial
//! - `node` (Xtensa): board bring-up and the serving loop for the `app`
//!   (ota_0/ota_1) and `factory` binaries
//!
//! Build-time identity: `DAGON_PUBKEY` (64 hex digits, default: the RFC 8032
//! test key) and `DAGON_TARGET_TAG` (default `watch-s3`).
#![cfg_attr(not(test), no_std)]

pub mod store;
pub mod trial;

#[cfg(target_arch = "xtensa")]
pub mod node;

use dagon_agent::Config;
use dagon_format::header::{mcu_id, port_kind};
use dagon_proto::payload::role;

/// Release public key the agent accepts.
pub const PUBKEY: [u8; 32] = hex32(match option_env!("DAGON_PUBKEY") {
    Some(k) => k,
    None => "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
});

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
