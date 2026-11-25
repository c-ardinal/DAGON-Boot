//! Executable model of DAGON's retained and persistent formats.
//!
//! The MCU identity, the compiled topology and the boot core trial cell are
//! specified by the shapes this model proves
//! (`site/src/content/docs/explanation/retained-formats.md`).
//!
//! Worlds ([`world_trial`]: the boot core trial cell, [`world_hand_off`]:
//! the app hand-off through that cell) generate seeded scenarios with
//! faults, run them against an oracle and check their properties after
//! every operation; a failure shrinks to a minimal case ([`harness`]).
//! [`identity`] holds the MCU identity rules.
#![forbid(unsafe_code)]

pub mod harness;
pub mod identity;
pub mod rng;
pub mod world_hand_off;
pub mod world_trial;
