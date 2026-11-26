//! DAGON MCU package for the RP2040.
//!
//! - Always (host-testable): the flash [`layout`], inline [`image`] checks,
//!   the stage0 view of the boot core log ([`bl`]), the links the relay
//!   reaches nodes over ([`RELAY_LINKS`]), and the MCU-independent
//!   parts re-exported from `dagon-mcu-common` ([`liveness`], [`frames`],
//!   [`uart`], [`mapped`]).
//! - Always: the clock setup a boot loader or an app runs on, from the
//!   definitions' constants ([`clock`]); with `hal`, its embassy-rp
//!   configuration.
//! - Target only: XIP reader, image start and raw backlight blink ([`arm`]).
//! - `hal` (embassy-rp): the board definition and node start-up
//!   ([`node`], [`BoardDef`]), the flash port
//!   ([`flash`]), the liveness-gated watchdog ([`watchdog`]), the APP slot
//!   store behind dagon-agent ([`store`]), link drivers ([`links`]: an I2C
//!   controller with the DAGON mailbox, a PIO UART stream) and sub-MCU
//!   control lines ([`sub`]).
//! - `hal` and `image-stage0`: the verifying key embedded at build time
//!   ([`PUBKEY`], [`TEST_KEY`]); stage0 holds it in its own flash
//!   ([`STAGE0_KEY_PLACE`]).
//! - `usb`: DAGON over USB CDC ([`usb`]), DAGON's high-priority executor
//!   ([`exec`]) and `node::start`.
//! - `relay`: the relay of `dagon-relay` on this MCU ([`relay`]: its
//!   links built from RP2040 peripherals, its tasks and clock).
//! - Linker memory map: an APP slot body by default (slot B with
//!   `slot-b`), so apps need no build.rs; `image-stage0` / `image-bootcore`
//!   for the boot images in `stage0/` and `bootcore/`.
//!
//! Board pins, flash size and extras live in a board package.
#![no_std]

pub use dagon_mcu_common::{frames, liveness, mapped, tag, uart};

// The constants this package's MCU definition states (`dagon-mcu.toml`):
// `TRIAL_ATTEMPTS`, `MASKED_MAX_US` (`None` while unmeasured), `ERASE_REGIONS` (the MCU's
// default erase regions; a node's own come from its board's generated rows),
// `SLOT_LAYOUTS`,
// `ERASE_UNIT_MAX_MS` (the longest single erase), the `_KNOWN` flags (checked against their source),
// `CONFORM_SCRATCH`, and `RELAY_LINKS` with one `RELAY_LINK_<PORT>` id per
// relay port and each link's buffer size in `RELAY_LINK_BUFFERS`
// (`[[relay_link]]`), and `RECOVERY_TARGETS`, the relay link, ROM loader
// address and lines of each node this node recovers (from the list in the
// node's boot folder). The boot stage and the app layer read these.
include!(concat!(env!("OUT_DIR"), "/definition.rs"));

/// This MCU's identity in image headers and answers, taken from its
/// definition's id.
pub const MCU_ID: dagon_format::header::McuId = dagon_format::header::McuId(MCU_ID_BYTES);

/// An erase region, for a board's `BoardDef` (`EraseRegion::table` turns its
/// generated `ERASE_REGIONS` rows into them).
pub use dagon_port::timeout::EraseRegion;

pub mod bl;
pub mod boot_receive;
pub mod clock;
pub mod image;
pub mod kinds;
pub mod layout;
pub mod regions;
pub mod reset;

#[cfg(target_os = "none")]
pub mod arm;

#[cfg(feature = "hal")]
pub mod boot_links;
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
pub mod store;
#[cfg(feature = "hal")]
pub mod sub;
#[cfg(feature = "usb")]
pub mod usb;
#[cfg(feature = "hal")]
pub mod watchdog;

#[cfg(feature = "hal")]
pub use node::{BoardCore, BoardDef};

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

/// The slot this image is linked for (A, or B with `slot-b`): an app's
/// APP slot, or the boot core's boot core slot.
pub const LINKED_SLOT: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };

/// The topology's MCU id and flash map this package implements.
pub const TOPOLOGY_MCU: &str = "rp2040";
pub const FLASH_MAP: &str = "rp2040-4m";

#[cfg(any(feature = "hal", feature = "image-stage0"))]
include!(concat!(env!("OUT_DIR"), "/pubkey.rs"));
