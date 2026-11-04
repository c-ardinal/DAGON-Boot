//! DAGON MCU package for the RP2040.
//!
//! - Always (host-testable): the flash [`layout`], inline [`image`] checks,
//!   the stage0 view of the boot core log ([`bl`]), hub command rules
//!   ([`cmd`]), and the MCU-independent
//!   parts re-exported from `dagon-mcu-common` ([`liveness`], [`frames`],
//!   [`uart`], [`mapped`]).
//! - Target only: XIP reader, image start and raw backlight blink ([`arm`]).
//! - `hal` (embassy-rp): the board definition and node start-up
//!   ([`node`], [`BoardDef`]), the flash port
//!   ([`flash`]), the liveness-gated watchdog ([`watchdog`]), the APP slot
//!   store behind dagon-agent ([`store`]), link drivers ([`links`]: an I2C
//!   controller with the DAGON mailbox, a PIO UART stream) and sub-MCU
//!   control lines ([`sub`]); the verifying key embedded at build time
//!   ([`PUBKEY`], [`TEST_KEY`]).
//! - `usb`: DAGON over USB CDC ([`usb`]), DAGON's high-priority executor
//!   ([`exec`]) and `node::start`.
//! - `relay`: the 親機 relay ([`relay`]: PC link, relaying to sub nodes,
//!   ROM recovery path, sub-node polling).
//! - Linker memory map: an APP slot body by default (slot B with
//!   `slot-b`), so apps need no build.rs; `image-stage0` / `image-bootcore`
//!   for the boot images in `stage0/` and `bootcore/`.
//!
//! Board pins, flash size and extras live in a board package.
#![no_std]

pub use dagon_mcu_common::{frames, liveness, mapped, tag, uart};

// The constants this package's MCU definition states (`dagon-mcu.toml`):
// `TRIAL_ATTEMPTS`, `MASKED_MAX_US` (`None` while unmeasured), `ERASE_UNIT`,
// `ERASE_UNIT_MAX_MS`, the `_KNOWN` flags (checked against their source),
// `CONFORM_SCRATCH`. The boot stage and the app layer read these.
include!(concat!(env!("OUT_DIR"), "/definition.rs"));

pub mod bl;
pub mod cmd;
pub mod image;
pub mod kinds;
pub mod layout;
pub mod nodes;
pub mod view;

#[cfg(target_os = "none")]
pub mod arm;

#[cfg(feature = "usb")]
pub mod exec;
#[cfg(feature = "hal")]
pub mod flash;
#[cfg(feature = "hal")]
pub mod links;
#[cfg(feature = "hal")]
pub mod node;
#[cfg(feature = "relay")]
pub mod relay;
#[cfg(feature = "hal")]
pub mod store;
#[cfg(feature = "hal")]
pub mod sub;
#[cfg(feature = "usb")]
pub mod usb;
#[cfg(feature = "hal")]
pub mod watchdog;

#[cfg(feature = "hal")]
pub use node::BoardDef;

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
pub const TOPOLOGY_MCU: &str = "rp2040";
pub const FLASH_MAP: &str = "rp2040-4m";

#[cfg(feature = "hal")]
include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));
