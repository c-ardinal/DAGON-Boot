//! Executable model of DAGON's retained and persistent formats.
//!
//! The boot record, the lockup fuse, the MCU identity and the compiled
//! topology's storage are specified by the shapes this model proves
//! (`docs/design/retained-formats.md`). Each format's codec is a trait
//! ([`record::RecordCodec`], [`fuse::FuseCodec`], [`topology::BodyCodec`])
//! so a production codec plugs into the same harness and stays checked.
//!
//! Worlds ([`world_record`], [`world_fuse`], [`world_topology`],
//! [`world_trial`]: the boot core trial cell) generate
//! seeded scenarios with faults after any byte, run them against an oracle
//! and check the properties P1..P26 after every operation; a failure
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
pub mod topology;
pub mod world_fuse;
pub mod world_record;
pub mod world_topology;
pub mod world_trial;
