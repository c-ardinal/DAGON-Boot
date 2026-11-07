//! Board-independent DAGON logic over the port contract: the state log
//! (fold, load, append, compaction), the kind S boot decision (verify,
//! trial, rollback), the boot core A/B rules of a stage0 MCU and
//! build-time key helpers.
#![no_std]
#![forbid(unsafe_code)]

pub mod boot;
pub mod bootcore;
pub mod key;
pub mod log;
