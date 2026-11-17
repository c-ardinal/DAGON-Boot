//! Argument rules of the relay commands, kept identical to the simulated
//! relay (`dagon-sim`) so tools tested against it behave the same on a
//! node.

use dagon_proto::payload::I2cRaw;
use dagon_proto::Status;

use crate::RelayLinkDef;

/// RELAY targets a node accepts: one of its `links`, an address that link
/// accepts, and a frame within that link's limit. Anything else answers
/// E_ARG.
pub fn relay_fits(links: &[RelayLinkDef], l: u8, addr: u8, inner_len: usize) -> bool {
    links
        .iter()
        .find(|k| k.id == l)
        .is_some_and(|k| k.fits(addr, inner_len))
}

/// I2C_RAW over the recovery link `link`: parse errors as parsed, E_STATE
/// unless BOOT0 is high, E_ARG for data to write or a read longer than the
/// link's buffer ([`RelayLinkDef::buffer_len`]).
pub fn i2c_raw<'a>(
    p: &'a [u8],
    boot0_high: bool,
    link: &RelayLinkDef,
) -> Result<I2cRaw<'a>, Status> {
    let raw = I2cRaw::parse(p)?;
    if !boot0_high {
        return Err(Status::State);
    }
    let limit = link.buffer_len();
    if usize::from(raw.read_len) > limit || raw.data.len() > limit {
        return Err(Status::Arg);
    }
    Ok(raw)
}
