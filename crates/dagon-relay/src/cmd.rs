//! Argument rules of the relay commands, kept identical to the simulated
//! relay (`dagon-sim`) so tools tested against it behave the same on a
//! node.

use dagon_proto::payload::I2cRaw;
use dagon_proto::{frame_len, max_payload, Status};

use crate::RelayLink;

/// RELAY targets a node accepts: one of its `links`, an address that link
/// accepts, and a frame within that link's limit. Anything else answers
/// E_ARG.
pub fn relay_fits(links: &[RelayLink], l: u8, addr: u8, inner_len: usize) -> bool {
    links
        .iter()
        .find(|k| k.id == l)
        .is_some_and(|k| k.addressing.accepts(addr) && inner_len <= frame_len(k.max_payload))
}

/// Longest I2C_RAW read that fits the USB response.
pub const RAW_READ_MAX: usize = max_payload::USB - 1;

/// I2C_RAW: parse errors as parsed, E_STATE unless BOOT0 is high, E_ARG for
/// a read longer than [`RAW_READ_MAX`].
pub fn i2c_raw(p: &[u8], boot0_high: bool) -> Result<I2cRaw<'_>, Status> {
    let raw = I2cRaw::parse(p)?;
    if !boot0_high {
        return Err(Status::State);
    }
    if raw.read_len as usize > RAW_READ_MAX {
        return Err(Status::Arg);
    }
    Ok(raw)
}
