//! Hub (RP2040) flash layout, the stage0 view of the boot core log, STATUS
//! mapping, inline image checks and stream frame reassembly, shared by
//! stage0, the boot core and the hub app. The APP state log and the boot
//! decision are dagon-core's.
#![no_std]

pub mod bl;
pub mod frames;
pub mod image;
pub mod layout;
pub mod status;

#[cfg(target_os = "none")]
pub mod arm;
#[cfg(feature = "rp")]
pub mod rp;
