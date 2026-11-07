//! The link kinds this package offers, by direction, for RP2040 (always a 親機):
//!
//! README.md, "Links", shows the matrix (generated from `LINK_KINDS`;
//! a kind the table leaves out is `No`).
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Reconnect, Support};

pub const LINK_KINDS: &[KindInfo] = &[
    KindInfo {
        kind: Kind::UsbCdc,
        up: Support::Yes,
        down: Support::NotYet,
    },
    KindInfo {
        kind: Kind::Uart,
        up: Support::NotYet,
        down: Support::Yes,
    },
    KindInfo {
        kind: Kind::I2c,
        up: Support::NotYet,
        down: Support::Yes,
    },
    KindInfo {
        kind: Kind::Ble,
        up: Support::NotYet,
        down: Support::NotYet,
    },
    KindInfo {
        kind: Kind::WifiTcp,
        up: Support::NotYet,
        down: Support::NotYet,
    },
    KindInfo {
        kind: Kind::Ethernet,
        up: Support::NotYet,
        down: Support::NotYet,
    },
    KindInfo {
        kind: Kind::Can,
        up: Support::NotYet,
        down: Support::NotYet,
    },
];

dagon_mcu_common::link_requirements!("rp2040", LINK_KINDS);
