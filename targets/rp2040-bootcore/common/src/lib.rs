//! Hub (RP2040) flash layout, state log folding/appending, STATUS mapping and
//! stream frame reassembly, shared by stage0, the boot core and the hub app.
#![no_std]

pub mod frames;
pub mod image;
pub mod layout;
pub mod log;
pub mod status;

#[cfg(target_os = "none")]
pub mod arm;
#[cfg(feature = "rp")]
pub mod rp;
