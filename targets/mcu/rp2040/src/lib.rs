//! DAGON MCU package for the RP2040.
//!
//! - Always (host-testable): the flash [`layout`], inline [`image`] checks,
//!   the stage0 view of the boot core log ([`bl`]), STATUS mapping
//!   ([`status`]), hub command rules ([`cmd`]), and the MCU-independent
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
//! - `usb`: DAGON over USB CDC ([`usb`]) and `node::start`.
//! - Linker memory map: an APP slot body by default (slot B with
//!   `slot-b`), so apps need no build.rs; `image-stage0` / `image-bootcore`
//!   for the boot images in `stage0/` and `bootcore/`.
//!
//! Board pins, flash size and extras live in a board package.
#![no_std]

pub use dagon_mcu_common::{frames, liveness, mapped, uart};

pub mod bl;
pub mod cmd;
pub mod image;
pub mod layout;
pub mod status;

#[cfg(target_os = "none")]
pub mod arm;

#[cfg(feature = "hal")]
pub mod flash;
#[cfg(feature = "hal")]
pub mod links;
#[cfg(feature = "hal")]
pub mod node;
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

/// The topology's MCU id and flash map this package implements.
pub const TOPOLOGY_MCU: &str = "rp2040";
pub const FLASH_MAP: &str = "rp2040-4m";

#[cfg(feature = "hal")]
include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));
