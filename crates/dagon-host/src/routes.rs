//! Which target a request to a node names: the node the PC is connected to
//! itself, or a node reached over the topology link joining the two, in
//! either orientation, by the relay table entry of the connected node's
//! MCU for the port of the connected node's end of it.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use dagon_format::topology::{TopoLink, NONE};
use dagon_port::{Addressing, RelayLinkDef};

/// One RELAY: the link id and the address the relaying node names the next
/// node by, and the largest payload that link carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hop {
    pub link: u8,
    pub addr: u8,
    pub max_payload: u16,
}

/// The most RELAYs between the PC and a node: each one takes its frame and
/// fields out of what the next may carry and adds its own wait.
pub const MAX_HOPS: usize = 4;

/// The hops to a node behind another relaying node, nearest first: two to
/// [`MAX_HOPS`]. Equal routes are equal values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    hops: [Hop; MAX_HOPS],
    len: u8,
}

impl Route {
    pub fn hops(&self) -> &[Hop] {
        &self.hops[..usize::from(self.len)]
    }
}

/// Where a request goes: the node the PC is connected to, or a node RELAY
/// reaches over one of its links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The node the PC is connected to.
    Local,
    /// A node RELAY reaches: the link id and the address RELAY names it by,
    /// and the largest payload that link carries.
    Linked {
        link: u8,
        addr: u8,
        max_payload: u16,
    },
    /// A node behind a linked node that relays: a RELAY inside a RELAY for
    /// each hop ([`Target::through`]).
    Deep(Route),
}

impl Target {
    /// The target `hops` lead to, nearest first: none is [`Target::Local`],
    /// one [`Target::Linked`]. `None` past [`MAX_HOPS`].
    pub fn through(hops: &[Hop]) -> Option<Target> {
        match *hops {
            [] => Some(Target::Local),
            [Hop {
                link,
                addr,
                max_payload,
            }] => Some(Target::Linked {
                link,
                addr,
                max_payload,
            }),
            _ if hops.len() <= MAX_HOPS => {
                let mut all = [Hop::default(); MAX_HOPS];
                all[..hops.len()].copy_from_slice(hops);
                Some(Target::Deep(Route {
                    hops: all,
                    len: hops.len() as u8,
                }))
            }
            _ => None,
        }
    }

    /// The hops to this target, nearest first (none for [`Target::Local`]).
    pub fn hops(&self) -> Vec<Hop> {
        match *self {
            Target::Local => Vec::new(),
            Target::Linked {
                link,
                addr,
                max_payload,
            } => alloc::vec![Hop {
                link,
                addr,
                max_payload,
            }],
            Target::Deep(r) => r.hops().to_vec(),
        }
    }

    /// The relaying node that reaches this target and the hop it takes:
    /// who knows the target as a peer. `None` for [`Target::Local`].
    pub fn parent(&self) -> Option<(Target, Hop)> {
        let hops = self.hops();
        let (last, before) = hops.split_last()?;
        Some((Target::through(before)?, *last))
    }
}

/// Each node by id with its target, or why it has none.
pub type NodeTargets = Vec<(String, Result<Target, RouteError>)>;

/// The node the PC is connected to: its topology index and the relay table
/// of its MCU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalNode {
    pub index: u8,
    pub links: Vec<RelayLinkDef>,
}

/// Why a request to a node has no target; nothing was sent. Nodes are
/// named by their topology index (the compiled topology holds no ids).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteError {
    /// No configuration was given: only the connected node is reached.
    NoConfiguration,
    /// The connected node's MCU has no definition.
    NoMcu { local: String, mcu: String },
    /// The configuration's updatable part has no node of this id.
    Unknown { node: String },
    /// The configuration does not compile.
    Topology(String),
    /// No topology link joins the two nodes.
    NoLink { local: u8, node: u8 },
    /// Several topology links join the two nodes.
    SeveralLinks { local: u8, node: u8 },
    /// The connected node's end names a port its MCU relays over no link
    /// on (no `[[relay_link]]` entry for it).
    NoEntry { local: u8, node: u8, port: u8 },
    /// A seven-bit link whose end at the node gives no address.
    NoAddress { local: u8, node: u8 },
    /// The connected node's MCU names a way of addressing this host does not know.
    Addressing { local: u8, node: u8 },
    /// The configuration names no node the PC is connected to.
    Connected { nodes: Vec<String> },
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::NoConfiguration => {
                f.write_str("no configuration: only the connected node is reached")
            }
            RouteError::NoMcu { local, mcu } => {
                write!(f, "'{local}': MCU '{mcu}' has no definition, so no relay link is known")
            }
            RouteError::Unknown { node } => {
                write!(f, "the configuration's updatable part has no node '{node}'")
            }
            RouteError::Topology(e) => write!(f, "{e}"),
            RouteError::NoLink { local, node } => {
                write!(f, "no link joins node {local} and node {node}")
            }
            RouteError::SeveralLinks { local, node } => {
                write!(f, "several links join node {local} and node {node}")
            }
            RouteError::NoEntry { local, node, port } => write!(
                f,
                "node {local} relays over no link on its port {port} (its link to node {node}): no [[relay_link]] for it"
            ),
            RouteError::NoAddress { local, node } => {
                write!(f, "the link from node {local} gives node {node} no address")
            }
            RouteError::Addressing { local, node } => write!(
                f,
                "node {local} addresses node {node} in a way this version does not know"
            ),
            RouteError::Connected { nodes } if nodes.is_empty() => {
                f.write_str("the configuration has no node the PC connects to")
            }
            RouteError::Connected { nodes } => write!(
                f,
                "the configuration has several nodes the PC may be connected to: {}",
                nodes.join(", ")
            ),
        }
    }
}

impl Target {
    /// The target of topology node `node` from node `local`, the node the
    /// PC is connected to, whose MCU relays over `links`. `topology` is the
    /// topology's links. `Local` when the two are one node; otherwise the
    /// one link joining them (either end may be `local`), the entry of
    /// `links` for the port of `local`'s end, and the address: the other
    /// end's for a seven-bit entry, the fixed one for a fixed entry.
    pub fn of_node(
        topology: impl IntoIterator<Item = TopoLink>,
        local: u8,
        node: u8,
        links: &[RelayLinkDef],
    ) -> Result<Target, RouteError> {
        if local == node {
            return Ok(Target::Local);
        }
        let joining: Vec<TopoLink> = topology
            .into_iter()
            .filter(|l| l.end(local).is_some() && l.end(node).is_some())
            .collect();
        let l = match joining.as_slice() {
            [l] => l,
            [] => return Err(RouteError::NoLink { local, node }),
            _ => return Err(RouteError::SeveralLinks { local, node }),
        };
        let (near, far) = (l.end(local).expect("joins"), l.end(node).expect("joins"));
        let def = links
            .iter()
            .find(|d| d.port == near.port)
            .ok_or(RouteError::NoEntry {
                local,
                node,
                port: near.port,
            })?;
        let addr = match def.addressing {
            Addressing::SevenBit if far.addr != NONE => far.addr,
            Addressing::SevenBit => return Err(RouteError::NoAddress { local, node }),
            Addressing::Fixed(a) => a,
            _ => return Err(RouteError::Addressing { local, node }),
        };
        Ok(Target::Linked {
            link: def.id,
            addr,
            max_payload: def.max_payload,
        })
    }
}

/// The links of a compiled topology.
pub(crate) fn links_of<'a>(
    topo: &'a dagon_format::topology::TopologyView<'_>,
) -> impl Iterator<Item = TopoLink> + 'a {
    (0..topo.link_count()).map(|i| topo.link(i))
}

/// Node `local` of `topo` as the node the PC is connected to: its index in
/// the compiled topology (the updatable part) and the `[[relay_link]]`
/// table of its MCU in `reg`.
#[cfg(feature = "defs")]
pub fn local_node(
    reg: &dagon_topology::defs::Registry,
    topo: &dagon_topology::Topology,
    local: &str,
) -> Result<LocalNode, RouteError> {
    let unknown = || RouteError::Unknown { node: local.into() };
    let n = topo.node(local).ok_or_else(unknown)?;
    let m = reg.mcu(&n.mcu).ok_or_else(|| RouteError::NoMcu {
        local: local.into(),
        mcu: n.mcu.clone(),
    })?;
    let index = updatable_ids(reg, topo)?
        .iter()
        .position(|id| id == local)
        .and_then(|i| u8::try_from(i).ok())
        .ok_or_else(unknown)?;
    Ok(LocalNode {
        index,
        links: m.relay_link_defs(),
    })
}

/// The ids of the compiled topology's nodes, in index order.
#[cfg(feature = "defs")]
fn updatable_ids(
    reg: &dagon_topology::defs::Registry,
    topo: &dagon_topology::Topology,
) -> Result<Vec<String>, RouteError> {
    let cut = dagon_topology::cut_with(reg, topo);
    match cut.topology {
        Some(_) => Ok(cut.reach),
        None => Err(RouteError::Topology(
            cut.report
                .errors()
                .map(|i| alloc::format!("{}: {}", i.path, i.message))
                .collect::<Vec<_>>()
                .join("; "),
        )),
    }
}

/// Each node of the compiled `topo` (its updatable part), by id in index
/// order, with its target from node `local` ([`Target::of_node`] with
/// [`local_node`]).
#[cfg(feature = "defs")]
pub fn node_targets(
    reg: &dagon_topology::defs::Registry,
    topo: &dagon_topology::Topology,
    local: &str,
) -> Result<NodeTargets, RouteError> {
    let me = local_node(reg, topo, local)?;
    let bin = dagon_topology::compile_with(reg, topo)
        .map_err(|e| RouteError::Topology(alloc::format!("{e}")))?;
    let view = dagon_format::topology::TopologyView::parse(&bin)
        .map_err(|e| RouteError::Topology(alloc::format!("{e:?}")))?;
    Ok(updatable_ids(reg, topo)?
        .into_iter()
        .zip(0u8..)
        .map(|(id, i)| (id, Target::of_node(links_of(&view), me.index, i, &me.links)))
        .collect())
}
