//! Board-independent DAGON logic over the port contract: the APP state log
//! (fold, load, append, compaction), the kind S boot decision (verify,
//! trial, rollback) and build-time key helpers.
#![no_std]
#![forbid(unsafe_code)]

pub mod boot;
pub mod key;
pub mod log;
