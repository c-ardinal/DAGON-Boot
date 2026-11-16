//! Executable model of DAGON's retained and persistent formats.
//!
//! The boot record, the lockup fuse and the MCU identity are specified by
//! the shapes this model proves
//! (`site/src/content/docs/explanation/retained-formats.md`). Each format's codec is a trait
//! ([`record::RecordCodec`], [`fuse::FuseCodec`]) so a production codec
//! plugs into the same harness and stays checked.
//!
//! Worlds ([`world_record`], [`world_fuse`], [`world_trial`]: the boot core
//! trial cell) generate
//! seeded scenarios with faults after any byte, run them against an oracle
//! and check the properties P1..P26 (P13 aside) after every operation; a failure
//! shrinks to a minimal case ([`harness`]). [`identity`] and [`boot`] hold
//! the MCU identity rules and the sandbox contract the fuse world drives.
#![forbid(unsafe_code)]

pub mod boot;
pub mod fuse;
pub mod harness;
pub mod identity;
pub mod mem;
pub mod record;
pub mod rng;
pub mod skew;
pub mod world_fuse;
pub mod world_record;
pub mod world_trial;
