//! Which relay link a request to a linked node goes over. The caller gives
//! the host the node the PC is connected to and the relay table of that
//! node's MCU ([`relay_routes`] under the `defs` feature); a node is reached
//! over the topology link joining the two nodes, in either orientation, by
//! the table entry for the port of the connected node's end of it.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use dagon_port::RelayLinkDef;

use crate::Target;

/// A RELAY to a node: the link and the address RELAY names it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub link: RelayLinkDef,
    pub addr: u8,
}

/// Why a request to a node has no route; nothing was sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteError {
    /// The connected node's MCU has no definition.
    NoMcu { local: String, mcu: String },
    /// No topology link joins the two nodes.
    NoLink { local: String, node: String },
    /// Several topology links join the two nodes.
    SeveralLinks { local: String, node: String },
    /// The connected node's end names a port its MCU relays over no link
    /// on (no `[[relay_link]]` entry for it).
    NoEntry {
        local: String,
        node: String,
        port: String,
    },
    /// A seven-bit link whose end at the node gives no address.
    NoAddress { local: String, node: String },
    /// The connected node's MCU names a way of addressing this host does not know.
    Addressing { local: String, node: String },
    /// No node of the configuration is this target (or no configuration was given).
    Unknown(Target),
    /// Several nodes of the configuration are this target.
    Ambiguous { local: String, nodes: Vec<String> },
    /// The configuration names no node or several the PC can be connected to.
    Connected { nodes: Vec<String> },
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::NoMcu { local, mcu } => {
                write!(f, "'{local}': MCU '{mcu}' has no definition, so no relay link is known")
            }
            RouteError::NoLink { local, node } => write!(f, "no link joins '{local}' and '{node}'"),
            RouteError::SeveralLinks { local, node } => {
                write!(f, "several links join '{local}' and '{node}'")
            }
            RouteError::NoEntry { local, node, port } => write!(
                f,
                "'{local}' relays over no link on port '{port}' (its link to '{node}'): no [[relay_link]] for it"
            ),
            RouteError::NoAddress { local, node } => {
                write!(f, "the link from '{local}' gives '{node}' no address")
            }
            RouteError::Addressing { local, node } => {
                write!(f, "'{local}' addresses '{node}' in a way this version does not know")
            }
            RouteError::Unknown(to) => write!(f, "no node of the configuration is {to:?}"),
            RouteError::Ambiguous { local, nodes } => {
                write!(f, "'{local}' reaches several nodes the same way: {}", nodes.join(", "))
            }
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

/// Each linked node's route from the connected node, as the caller resolved
/// them; empty (the default): only the connected node is reached.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Routes {
    local: String,
    nodes: Vec<(String, Target, Result<Route, RouteError>)>,
    /// Why no route is known at all (the configuration names no single
    /// connected node).
    failed: Option<RouteError>,
}

impl Routes {
    /// The routes from node `local` to `nodes`: each node's id, the target
    /// requests to it name, and its route or why it has none.
    pub fn new(local: String, nodes: Vec<(String, Target, Result<Route, RouteError>)>) -> Self {
        Routes {
            local,
            nodes,
            failed: None,
        }
    }

    /// No routes, every request to a linked node failing with `why`.
    pub fn failed(why: RouteError) -> Self {
        Routes {
            failed: Some(why),
            ..Routes::default()
        }
    }

    /// The route of a request to `to`. A node no link joins to the connected
    /// node gives way to one that is joined at the same target.
    pub fn to(&self, to: Target) -> Result<Route, RouteError> {
        if let Some(why) = &self.failed {
            return Err(why.clone());
        }
        let hits: Vec<_> = self.nodes.iter().filter(|(_, t, _)| *t == to).collect();
        let unlinked = |(_, _, r): &&(String, Target, Result<Route, RouteError>)| {
            matches!(r, Err(RouteError::NoLink { .. }))
        };
        let linked: Vec<_> = hits.iter().filter(|h| !unlinked(h)).collect();
        let pick = if linked.is_empty() {
            hits.iter().collect()
        } else {
            linked
        };
        let one_node = pick.windows(2).all(|w| w[0].0 == w[1].0);
        match pick.as_slice() {
            [] => Err(RouteError::Unknown(to)),
            // One node, listed once per link it has at this target.
            [(_, _, r), ..] if one_node => r.clone(),
            many => Err(RouteError::Ambiguous {
                local: self.local.clone(),
                nodes: many.iter().map(|(id, _, _)| id.clone()).collect(),
            }),
        }
    }
}

/// The routes from node `local` of `topo`: for each other node, the one
/// topology link joining the two (either orientation), the port of
/// `local`'s end of it, and the `[[relay_link]]` entry of `local`'s MCU in
/// `reg` for that port. Each node is listed under the target each of its
/// links gives it (its address on a link with addresses, else the one peer
/// of a stream), with its route or why it has none.
#[cfg(feature = "defs")]
pub fn relay_routes(
    reg: &dagon_topology::defs::Registry,
    topo: &dagon_topology::Topology,
    local: &str,
) -> Routes {
    let mcu = topo.node(local).map(|n| (n.mcu.as_str(), reg.mcu(&n.mcu)));
    let mut nodes = Vec::new();
    for n in topo.nodes.iter().filter(|n| n.id != local) {
        let (l, id) = (String::from(local), n.id.clone());
        let joining: Vec<_> = topo
            .links
            .iter()
            .filter(|k| k.end(local).is_some() && k.end(&n.id).is_some())
            .collect();
        let links: Vec<_> = if joining.is_empty() {
            topo.links_at(&n.id).map(|(_, k)| k).collect()
        } else {
            joining.clone()
        };
        for link in links {
            let Some(to) = target_at(link, &n.id) else {
                continue;
            };
            let r = match (&mcu, joining.as_slice()) {
                (_, []) => Err(RouteError::NoLink {
                    local: l.clone(),
                    node: id.clone(),
                }),
                (_, [_, _, ..]) => Err(RouteError::SeveralLinks {
                    local: l.clone(),
                    node: id.clone(),
                }),
                (Some((_, Some(m))), [_]) => route(m, link, local, &n.id),
                (Some((name, None)), [_]) => Err(RouteError::NoMcu {
                    local: l.clone(),
                    mcu: String::from(*name),
                }),
                (None, [_]) => continue,
            };
            nodes.push((id.clone(), to, r));
        }
    }
    Routes::new(local.into(), nodes)
}

/// The target a request to node `node` over `link` names.
#[cfg(feature = "defs")]
fn target_at(link: &dagon_topology::schema::Link, node: &str) -> Option<Target> {
    use dagon_topology::schema::LinkKind;
    match link.kind {
        LinkKind::I2c => link.end(node)?.addr.map(Target::I2c),
        LinkKind::Uart => Some(Target::Uart),
        // No target names a node over any other kind yet.
        _ => None,
    }
}

/// Node `node` over `link` (the one joining it to `local`), by the entry of
/// `local`'s MCU `m` for the port of `local`'s end.
#[cfg(feature = "defs")]
fn route(
    m: &dagon_topology::defs::McuDef,
    link: &dagon_topology::schema::Link,
    local: &str,
    node: &str,
) -> Result<Route, RouteError> {
    let (l, n) = (String::from(local), String::from(node));
    let (near, far) = (
        link.end(local).expect("joins"),
        link.end(node).expect("joins"),
    );
    let port = m.port(&near.port).map(|p| p.index);
    let Some(def) = m
        .relay_link_defs()
        .into_iter()
        .find(|d| Some(d.port) == port)
    else {
        return Err(RouteError::NoEntry {
            local: l,
            node: n,
            port: near.port.clone(),
        });
    };
    let addr = match def.addressing {
        dagon_port::Addressing::SevenBit => far.addr.ok_or(RouteError::NoAddress {
            local: l.clone(),
            node: n.clone(),
        })?,
        dagon_port::Addressing::Fixed(a) => a,
        _ => return Err(RouteError::Addressing { local: l, node: n }),
    };
    Ok(Route { link: def, addr })
}
