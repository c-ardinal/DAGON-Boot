//! The link kinds this package offers, by direction, for RP2040 (always a coordinator):
//!
//! README.md, "Links", shows the matrix (generated from `LINK_KINDS`,
//! which the MCU definition's `[links]` gives).
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Reconnect, Support};

/// The MCU definition's `[links]`: the kinds it lists are `Yes`
/// (`dagon_mcu_common::link::kinds_of_definition`).
pub const LINK_KINDS: &[KindInfo] =
    &dagon_mcu_common::link::kinds_of_definition(crate::LINKS_UP, crate::LINKS_DOWN);

dagon_mcu_common::link_requirements!("rp2040", LINK_KINDS);
