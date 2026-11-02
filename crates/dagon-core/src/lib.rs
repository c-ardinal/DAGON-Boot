//! Board-independent DAGON logic over the port contract: the APP state log
//! (fold, load, append) and the kind S boot decision (verify, trial,
//! rollback).
#![no_std]
#![forbid(unsafe_code)]

pub mod boot;
pub mod log;
