//! The link kinds this package offers, by direction, for STM32F401RE (a 子機: no downlinks):
//!
//! | Kind | Up | Down |
//! |---|---|---|
//! | `usb-cdc` | NotYet | No |
//! | `uart` | NotYet | No |
//! | `i2c` | Yes | No |
//! | `ble` | NotYet | No |
//! | `wifi-tcp` | NotYet | No |
//! | `ethernet` | NotYet | No |
//! | `can` | NotYet | No |
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Link, LinkError, Reconnect, Support};

pub const LINK_KINDS: &[KindInfo] = &[
    KindInfo {
        kind: Kind::UsbCdc,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Uart,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::I2c,
        up: Support::Yes,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Ble,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::WifiTcp,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Ethernet,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Can,
        up: Support::NotYet,
        down: Support::No,
    },
];

dagon_mcu_common::link_requirements!("stm32f401re", LINK_KINDS);
