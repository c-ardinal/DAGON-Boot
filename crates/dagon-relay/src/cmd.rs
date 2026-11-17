//! Argument rules of the relay commands, kept identical to the simulated
//! relay (`dagon-sim`) so tools tested against it behave the same on a
//! node.

use dagon_proto::payload::{I2cRaw, Relay};
use dagon_proto::Status;

use crate::RelayLinkDef;

/// RELAY targets a node accepts from an uplink carrying `ingress` payload
/// bytes: one of its `links`, and what [`relay_admits`] admits on it.
/// Anything else answers E_ARG.
pub fn relay_fits(links: &[RelayLinkDef], ingress: usize, r: &Relay) -> bool {
    links
        .iter()
        .find(|k| k.id == r.link)
        .is_some_and(|k| relay_admits(k, r, ingress))
}

/// Whether `link` takes `r` from an uplink carrying `ingress` payload
/// bytes: an address the link accepts, and an inner frame the tunnel over
/// it admits ([`Relay::admits`]).
pub fn relay_admits(link: &RelayLinkDef, r: &Relay, ingress: usize) -> bool {
    link.addressing.accepts(r.addr) && r.admits(ingress, usize::from(link.max_payload))
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
