//! Board-independent parts of the NUCLEO-F401RE app, kept apart from the
//! HAL so they run in host tests: the kind S slot store behind dagon-agent,
//! the I2C target mailbox and the 7-segment text.
#![no_std]
#![forbid(unsafe_code)]

pub mod mailbox;
pub mod seg;
mod store;

pub use store::KindSStore;
