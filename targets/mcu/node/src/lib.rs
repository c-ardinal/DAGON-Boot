//! The node's services the DAGON MCU packages run, MCU-independent. They
//! build on the driver helpers of `dagon-mcu-common`, which never use
//! them:
//! - [`guard`]: the liveness-gated watchdog guard apps declare;
//! - [`handed`]: the start of a boot core whose stage0 starts the app on
//!   its request (listen window, hand-over, recovery, then the request);
//! - [`hooks`]: the app layer's chain, with the app's CONFIRM self-check;
//! - [`serve`]: the agent's serving loop over any link;
//! - [`log_store`]: the slot store of an MCU whose boot stage keeps a
//!   DAGON state log, inline or detached headers;
//! - [`masked_time`] (feature `masked-time`): the longest time DAGON's own
//!   code masks interrupts, for an app built to measure it.
#![no_std]

pub mod guard;
pub mod handed;
pub mod hooks;
pub mod log_store;
#[cfg(any(test, feature = "masked-time"))]
pub mod masked_time;
pub mod serve;
