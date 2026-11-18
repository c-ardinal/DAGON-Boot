//! Executable model of DAGON's retained and persistent formats.
//!
//! The MCU identity, the compiled topology and the boot core trial cell are
//! specified by the shapes this model proves
//! (`site/src/content/docs/explanation/retained-formats.md`). The boot
//! record, the lockup fuse and the keyed records are models the design no
//! longer has (the limitations page lists them). Each format's codec is a trait
//! ([`record::RecordCodec`], [`fuse::FuseCodec`]) so a production codec
//! plugs into the same harness and stays checked.
//!
//! Worlds ([`world_record`], [`world_fuse`], [`world_trial`]: the boot core
//! trial cell) generate
//! seeded scenarios with faults after any byte, run them against an oracle
//! and check their properties after every operation; a failure
//! shrinks to a minimal case ([`harness`]). [`identity`] and [`boot`] hold
//! the MCU identity rules and the sandbox contract the fuse world drives.
//!
//! [`journal`] is the state log with keyed records.
#![forbid(unsafe_code)]

pub mod boot;
pub mod fuse;
pub mod harness;
pub mod identity;
pub mod journal;
pub mod mem;
pub mod record;
pub mod rng;
pub mod skew;
pub mod world_fuse;
pub mod world_record;
pub mod world_trial;
