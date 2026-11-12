//! Each node's own time limits ([`NodeTimings`]) as the caller injects them
//! into [`Host::timings`](crate::Host::timings), built from the MCU
//! definitions under the `defs` feature.

use alloc::vec::Vec;
use dagon_port::timeout::{EraseLayout, EraseRegion, NodeTimings, SlotLayout};

/// Node timings that own their erase layout (built at run time).
pub type HostTimings = NodeTimings<EraseLayout<Vec<EraseRegion>, Vec<SlotLayout>>>;

/// An MCU definition whose timings break a [`NodeTimings::check`] rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeTimingError {
    /// The MCU's id (`[mcu] id`).
    pub mcu: alloc::string::String,
    pub rule: dagon_port::timeout::TimingError,
}

impl core::fmt::Display for NodeTimingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MCU '{}': {}", self.mcu, self.rule)
    }
}

/// The timings of a node of MCU `m` on `board`: its effective erase layout
/// (the board's `[flash]` regions when it states them, else the MCU's) and
/// the MCU's `[watchdog]` trial ([`NodeTimings::FALLBACK`] without one),
/// refused when they break a [`NodeTimings::check`] rule.
#[cfg(feature = "defs")]
pub fn mcu_timings(
    m: &dagon_topology::defs::McuDef,
    board: Option<&dagon_topology::defs::BoardDef>,
) -> Result<HostTimings, NodeTimingError> {
    let t = m.timings_on(board);
    t.check().map_err(|rule| NodeTimingError {
        mcu: m.id.clone(),
        rule,
    })?;
    Ok(t)
}

/// Each node's timings by topology index, from its MCU's definition in
/// `reg` on its board (the board it names, else its MCU's default board);
/// an MCU missing from `reg` gets [`NodeTimings::FALLBACK`].
#[cfg(feature = "defs")]
pub fn node_timings(
    reg: &dagon_topology::defs::Registry,
    topo: &dagon_topology::Topology,
) -> Result<Vec<(u8, HostTimings)>, NodeTimingError> {
    topo.nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let board = reg.node_board(n);
            let t = match reg.mcu(&n.mcu) {
                Some(m) => mcu_timings(m, board)?,
                None => NodeTimings::FALLBACK,
            };
            Ok((u8::try_from(i).unwrap_or(u8::MAX), t))
        })
        .collect()
}
