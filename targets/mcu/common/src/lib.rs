//! MCU-independent building blocks of the DAGON MCU packages:
//! - [`guard`]: the liveness-gated watchdog guard apps declare;
//! - [`hooks`]: the app layer's chain, with the app's CONFIRM self-check;
//! - [`tag`]: the node's image target tag, checked;
//! - [`link`]: the one frame-link interface of every hop, link kinds;
//! - [`liveness`]: deadline bookkeeping that gates watchdog feeding, with
//!   preemption accounting for two-level task layouts;
//! - [`frames`]: DAGON frames cut out of a byte stream (USB CDC, UART);
//! - [`uart`]: request/response over a byte stream (bounded, resyncing);
//! - [`serve`]: the agent's serving loop over any link;
//! - [`reattach`]: when a USB device drops off the bus and attaches again
//!   because the host never reset it;
//! - [`mapped`]: read-only `Flash` over memory-mapped flash;
//! - [`shared`]: several handles on one flash driver, no interrupt masking;
//! - [`vector`]: the entry point of an image that starts with its vector
//!   table;
//! - [`verify_time`] (feature `verify-time`): a stage0 signature check
//!   timed in core clock cycles, for a stage0 built to measure it.
//! - [`boot_time`] (feature `boot-time`): the records of a boot core built
//!   to measure its listen window or its boot decision.
//! - [`masked_time`] (feature `masked-time`): the longest time DAGON's own
//!   code masks interrupts, for an app built to measure it.
#![no_std]

#[doc(hidden)]
pub use dagon_format as __format;

#[cfg(any(test, feature = "boot-time"))]
pub mod boot_time;
pub mod frames;
pub mod guard;
pub mod hooks;
pub mod link;
pub mod liveness;
pub mod mapped;
#[cfg(any(test, feature = "masked-time"))]
pub mod masked_time;
pub mod reattach;
pub mod serve;
pub mod shared;
pub mod tag;
pub mod uart;
pub mod vector;
#[cfg(any(test, feature = "verify-time"))]
pub mod verify_time;
