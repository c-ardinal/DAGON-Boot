//! Per-stage time limits of the conformance run, and the per-link limits
//! they depend on. Fixed: a port cannot change them.
//!
//! | Stage | Limit |
//! |---|---|
//! | plain request (HELLO, STATUS, XFER_BEGIN, XFER_DATA, non-final XFER_HDR, REBOOT) | [`REQUEST_MS`] |
//! | final XFER_HDR (erases the slot) | [`ERASE_F401_MS`] on STM32F401, else [`ERASE_MS`] |
//! | XFER_END, SET_PENDING, CONFIRM (verify or write state) | [`VERIFY_MS`] |
//! | CONF_RUN (one node-side test) | [`CONF_RUN_MS`] |
//! | whole image transfer | [`transfer_ms`]: size / link floor ([`link_limits`]) + [`TRANSFER_MARGIN_MS`] |
//! | REBOOT until the expected HELLO (also covers a full rollback) | [`REBOOT_WAIT_MS`] |

use dagon_port::LinkKind;

pub const REQUEST_MS: u32 = 300;
pub const VERIFY_MS: u32 = 5_000;
/// One 128 KiB sector takes at most 4 s.
pub const ERASE_F401_MS: u32 = 5_000;
pub const ERASE_MS: u32 = 30_000;
pub const CONF_RUN_MS: u32 = 30_000;
pub const REBOOT_WAIT_MS: u32 = 30_000;
pub const TRANSFER_MARGIN_MS: u32 = 30_000;
/// Lowest accepted effective transfer speed on I2C, bytes per second.
pub const I2C_FLOOR_BPS: u32 = 10_000;
/// Lowest accepted effective transfer speed on UART (and direct USB), bytes
/// per second.
pub const UART_FLOOR_BPS: u32 = 5_000;
/// Pause between HELLO polls while waiting for a node to come back.
pub const POLL_MS: u32 = 500;

/// What the run judges a link kind by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkLimits {
    /// The payload limit an agent on the link announces in HELLO.
    pub max_payload: usize,
    /// Lowest accepted effective transfer speed, bytes per second.
    pub floor_bps: u32,
}

/// The limits of `link`; `None` for a kind the run has no limits for, which
/// it refuses ([`SetupError::UnsupportedLink`](crate::pc::SetupError))
/// rather than judge it by another kind's.
pub const fn link_limits(link: LinkKind) -> Option<LinkLimits> {
    use dagon_proto::max_payload;
    let (max_payload, floor_bps) = match link {
        LinkKind::I2c => (max_payload::I2C, I2C_FLOOR_BPS),
        LinkKind::Uart => (max_payload::UART, UART_FLOOR_BPS),
        LinkKind::UsbCdc => (max_payload::USB, UART_FLOOR_BPS),
        _ => return None,
    };
    Some(LinkLimits {
        max_payload,
        floor_bps,
    })
}

/// Time allowed for transferring an image of `len` bytes (header included)
/// over a link with `limits`.
pub const fn transfer_ms(len: u32, limits: LinkLimits) -> u32 {
    (len as u64 * 1000 / limits.floor_bps as u64) as u32 + TRANSFER_MARGIN_MS
}

/// Limit for the request that erases the slot.
pub const fn erase_ms(mcu_id: u16) -> u32 {
    if mcu_id == dagon_format::header::mcu_id::STM32F401XE {
        ERASE_F401_MS
    } else {
        ERASE_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_limit_follows_link_floor() {
        let limits = |k| link_limits(k).unwrap();
        assert_eq!(
            transfer_ms(64 * 1024 + 512, limits(LinkKind::I2c)),
            6_604 + 30_000
        );
        assert_eq!(transfer_ms(10_000, limits(LinkKind::Uart)), 32_000);
        assert_eq!(erase_ms(0x0002), 5_000);
        assert_eq!(erase_ms(0x0003), 30_000);
    }
}
