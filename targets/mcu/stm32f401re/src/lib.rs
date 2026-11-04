//! DAGON MCU package for the STM32F401RE.
//!
//! - Always (host-testable): the flash [`layout`], the kind S slot store
//!   behind dagon-agent ([`KindSStore`]), the I2C target [`mailbox`], and
//!   the conformance scratch.
//! - `hal` (embassy-stm32): clocks ([`clocks`]), the flash port
//!   ([`flash`]), the liveness-gated IWDG ([`watchdog`]); the verifying key
//!   embedded at build time ([`PUBKEY`], [`TEST_KEY`]).
//! - `node`: the board definition and node start-up ([`node`],
//!   [`BoardDef`]) with DAGON's high-priority executor ([`exec`]); logs
//!   through defmt.
//! - `conform`: the conformance build's CONF_RUN serving.
//! - Target only: the boot core trial cell and stage0's watchdog start
//!   ([`trial`]).
//! - Linker memory map: an APP slot body by default (slot B with
//!   `slot-b`), so apps need no build.rs; `image-stage0` / `image-bootcore`
//!   for the boot images in `stage0/` and `bootcore/` (the boot core in
//!   slot B with `slot-b`).
//!
//! Board pins, the link's address and extras live in a board package.
#![no_std]

pub mod layout;
pub mod links;
pub mod mailbox;
mod store;
pub use store::KindSStore;
#[cfg(target_os = "none")]
pub mod trial;

/// The slot this image is linked for (A, or B with `slot-b`): an app's
/// APP slot, or the boot core's boot core slot.
pub const LINKED_SLOT: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };

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

// The constants this package's MCU definition states (`dagon-mcu.toml`):
// `TRIAL_ATTEMPTS`, `MASKED_MAX_US` (`None` while unmeasured), `ERASE_UNIT`,
// `ERASE_UNIT_MAX_MS`, the `_KNOWN` flags (checked against their source),
// `CONFORM_SCRATCH`. The boot stage and the app layer read these.
include!(concat!(env!("OUT_DIR"), "/definition.rs"));

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

/// Scratch address when the definition names none: outside the flash, so
/// the conformance erase/write tests fail ("outside flash") without erasing.
pub const NO_SCRATCH: u32 = 0xFFFF_FFFF;

/// Start of the flash a conformance build erases and writes: the
/// definition's `[conformance] scratch` (no image, header or log is there),
/// or [`NO_SCRATCH`].
pub const fn conform_scratch() -> u32 {
    match CONFORM_SCRATCH {
        Some((at, _)) => at,
        None => NO_SCRATCH,
    }
}
