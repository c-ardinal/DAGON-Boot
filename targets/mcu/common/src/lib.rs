//! MCU-independent building blocks of the DAGON MCU packages:
//! - [`guard`]: the liveness-gated watchdog guard apps declare;
//! - [`hooks`]: the app's CONFIRM self-check as agent hooks;
//! - [`tag`]: the node's image target tag, checked;
//! - [`liveness`]: deadline bookkeeping that gates watchdog feeding, with
//!   preemption accounting for two-level task layouts;
//! - [`frames`]: DAGON frames cut out of a byte stream (USB CDC, UART);
//! - [`uart`]: request/response over a byte stream (bounded, resyncing);
//! - [`mapped`]: read-only `Flash` over memory-mapped flash.
#![no_std]

pub mod frames;
pub mod guard;
pub mod hooks;
pub mod liveness;
pub mod mapped;
pub mod tag;
pub mod uart;
