//! DAGON MCU package for the STM32F401RE.
//!
//! - Always (host-testable): the flash [`layout`], the kind S slot store
//!   behind dagon-agent ([`KindSStore`]), the I2C target [`mailbox`], and
//!   the conformance scratch slot rule.
//! - `hal` (embassy-stm32): clocks ([`clocks`]), the flash port
//!   ([`flash`]), the liveness-gated IWDG ([`watchdog`]); the verifying key
//!   embedded at build time ([`PUBKEY`], [`TEST_KEY`]).
//! - `node`: the board definition and node start-up ([`node`],
//!   [`BoardDef`]) with DAGON's high-priority executor ([`exec`]); logs
//!   through defmt.
//! - `conform`: the conformance build's CONF_RUN serving.
//! - Linker memory map: an APP slot body by default (slot B with
//!   `slot-b`), so apps need no build.rs; `image-bootcore` for the boot
//!   core in `bootcore/`.
//!
//! Board pins, the link's address and extras live in a board package.
#![no_std]

pub mod layout;
pub mod mailbox;
mod store;
pub use store::KindSStore;

#[cfg(feature = "hal")]
mod clocks;
#[cfg(feature = "hal")]
pub use clocks::{clocks, SYSCLK_HZ};
#[cfg(feature = "node")]
pub mod exec;
#[cfg(feature = "hal")]
pub mod flash;
#[cfg(feature = "node")]
pub mod node;
#[cfg(feature = "hal")]
pub mod watchdog;
#[cfg(feature = "node")]
pub use node::BoardDef;

pub use dagon_mcu_common::{liveness, tag};

/// This node's image target tag, from `DAGON_TARGET_TAG` at build time
/// (empty if unset). The boot core accepts only images with this tag, and
/// the node's agent announces it; a generated project sets it in its
/// `.cargo/config.toml`. Up to 16 ASCII characters.
pub const TARGET_TAG: &str = match option_env!("DAGON_TARGET_TAG") {
    Some(t) => t,
    None => "",
};

/// This node's [`TARGET_TAG`], checked at compile time in the crate that
/// uses it: an app built without a valid `DAGON_TARGET_TAG` (empty, longer
/// than 16 characters, not printable ASCII) fails to build, like its boot
/// core does.
#[macro_export]
macro_rules! target_tag {
    () => {{
        const _: [u8; 16] = $crate::tag::checked_tag($crate::TARGET_TAG);
        $crate::TARGET_TAG
    }};
}

/// The topology's MCU id and flash map this package implements.
pub const TOPOLOGY_MCU: &str = "stm32f401re";
pub const FLASH_MAP: &str = "f401-default";

#[cfg(feature = "hal")]
include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));

use dagon_core::boot::SlotLayout;
use dagon_core::log::LogState;

/// Scratch address when no slot may be destroyed: outside the flash, so the
/// conformance erase/write tests fail ("outside flash") without erasing.
pub const NO_SCRATCH: u32 = 0xFFFF_FFFF;

/// The APP slot a conformance build may destroy as scratch: neither running,
/// nor confirmed, nor holding the pending (staged or on trial) image. `None`
/// on trial and while an update waits for its first boot.
pub fn scratch_slot(state: &LogState, running: u8) -> Option<u8> {
    let held = [state.confirmed, state.pending].map(|r| r.map(|r| r.slot));
    (0..2u8).find(|&s| s != running && !held.contains(&Some(s)))
}

/// Start of the scratch slot, or [`NO_SCRATCH`].
pub fn scratch_addr(state: &LogState, running: u8, slots: &SlotLayout) -> u32 {
    scratch_slot(state, running).map_or(NO_SCRATCH, |s| slots.slots[s as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_core::log::SlotRef;
    use dagon_format::Version;

    fn at(slot: u8) -> Option<SlotRef> {
        Some(SlotRef {
            slot,
            version: Version::default(),
            digest16: [0; 16],
        })
    }

    #[test]
    fn scratch_is_never_running_or_confirmed() {
        let confirmed_a = LogState {
            confirmed: at(0),
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&confirmed_a, 0), Some(1));
        let confirmed_b = LogState {
            confirmed: at(1),
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&confirmed_b, 1), Some(0));
        // Trial of B with A confirmed: no slot to spare.
        let trial = LogState {
            confirmed: at(0),
            pending: at(1),
            attempts: 1,
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&trial, 1), None);
        // B staged as pending, not started yet: it must survive too.
        let staged = LogState {
            confirmed: at(0),
            pending: at(1),
            ..LogState::default()
        };
        assert_eq!(scratch_slot(&staged, 0), None);
    }
}
