//! `topo compile`: validated YAML model -> topology canonical binary
//! (layout in `dagon_format::topology`, shared with the hub).
//!
//! Mapping: node index = position in `nodes[]`; a link's ends in node order,
//! each with its port's `[[link_port]]` index and its address; links sorted by
//! their ends' nodes; `bus_id` = position in `buses[]`; every flash map is its MCU's default
//! (`map_id` 0); omitted `listen_window_ms` = 0, `max_attempts` = 3, bus speed
//! = 400 kHz, baud = [`crate::schema::DEFAULT_BAUD`]. Node ids, the topology name, positions and
//! nicknames are not in the binary.

use std::fmt;

use crate::defs::McuDef;
use dagon_format::header::{target_tag, McuId};
use dagon_format::manifest::{constraint_kind, Constraint as BinConstraint};
use dagon_format::topology::{self as tp, TopoEnd, TopoLink, TopoNode};
use dagon_format::Version;

use crate::defs::{PortKind, Registry};
use crate::schema::{Constraint, ControlKind, End, Link, LinkKind, Role, Tier, UartIface};
use crate::{check_with, effective_tiers, Report, Topology};

const DEFAULT_MAX_ATTEMPTS: u8 = 3;
const MAX_TIERS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The YAML did not parse.
    Parse(String),
    /// `topo check` reported errors.
    Invalid(Report),
    /// Valid YAML that the binary cannot represent.
    Unrepresentable(String),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "topology does not parse: {e}"),
            Self::Invalid(r) => write!(
                f,
                "topology has {} error(s); run `dagon topo check`",
                r.errors().count()
            ),
            Self::Unrepresentable(e) => write!(f, "topology cannot be compiled: {e}"),
        }
    }
}

impl std::error::Error for CompileError {}

fn bad(msg: impl Into<String>) -> CompileError {
    CompileError::Unrepresentable(msg.into())
}

/// Canonical binary of a topology that passes `check` (warnings allowed),
/// against the builtin definitions. Only its updatable part is encoded
/// ([`crate::cut_with`]), after it passes `check` on its own; for a 構成
/// updating reaches whole that is all of it.
pub fn compile(t: &Topology) -> Result<Vec<u8>, CompileError> {
    compile_with(Registry::builtin(), t)
}

/// [`compile`] against the MCUs and boards of `reg`.
pub fn compile_with(reg: &Registry, t: &Topology) -> Result<Vec<u8>, CompileError> {
    let report = check_with(reg, t);
    if !report.is_pushable() {
        return Err(CompileError::Invalid(report));
    }
    // The part updating reaches is checked on its own before it is encoded.
    let cut = crate::cut_with(reg, t);
    match cut.topology {
        Some(part) => encode(reg, &part),
        None => Err(CompileError::Invalid(cut.report)),
    }
}

/// The MCU of the node on node `id`'s ROM recovery link: its
/// [`TopologyView::rom_peer`](tp::TopologyView::rom_peer) in the binary of
/// the part of `t` updating reaches, `reg` saying which MCUs have a ROM I2C
/// loader. The relay at `id` is built for that loader's address (`dagon
/// gen`); the host takes the ROM path to the same node. `None`: `id` has
/// no such link, or that part does not compile.
pub fn recovery_peer_mcu(reg: &Registry, t: &Topology, id: &str) -> Option<String> {
    let part = crate::cut_with(reg, t).topology?;
    let bin = encode(reg, &part).ok()?;
    let view = tp::TopologyView::parse(&bin).ok()?;
    let at = u8::try_from(part.nodes.iter().position(|n| n.id == id)?).ok()?;
    let has_loader = |w: McuId| reg.mcu_by_id(w).is_some_and(McuDef::has_rom_loader);
    let peer = view.rom_peer(at, has_loader)?;
    Some(part.nodes.get(usize::from(peer))?.mcu.clone())
}

pub fn parse_str(yaml: &str) -> Result<Topology, CompileError> {
    yaml_serde::from_str(yaml).map_err(|e| CompileError::Parse(e.to_string()))
}

pub fn compile_str(yaml: &str) -> Result<Vec<u8>, CompileError> {
    compile(&parse_str(yaml)?)
}

/// Image header `mcu_id` for a topology MCU id (builtin definitions).
pub fn mcu_id_of(mcu: &str) -> Option<McuId> {
    Registry::builtin().mcu(mcu).map(McuDef::mcu_id)
}

/// Image header `port_kind` for a topology port name.
pub fn port_kind_of(port: &str) -> Option<u8> {
    PortKind::from_name(port).map(PortKind::wire)
}

/// `"major.minor.patch"` (as shown by `Version`'s Display).
pub fn parse_version(s: &str) -> Option<Version> {
    let mut it = s.trim().split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    it.next()
        .is_none()
        .then(|| Version::new(major, minor, patch))
}

fn index_of(t: &Topology, id: &str) -> Result<u8, CompileError> {
    t.nodes
        .iter()
        .position(|n| n.id == id)
        .map(|i| i as u8)
        .ok_or_else(|| bad(format!("unknown node '{id}'")))
}

fn tier_code(tier: Tier) -> u8 {
    match tier {
        Tier::Local => tp::tier::LOCAL,
        Tier::Redistribute => tp::tier::REDISTRIBUTE,
        Tier::RomForced => tp::tier::ROM_FORCED,
        Tier::RomCoop => tp::tier::ROM_COOP,
    }
}

fn nodes(reg: &Registry, t: &Topology) -> Result<Vec<TopoNode>, CompileError> {
    t.nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let tiers = effective_tiers(t, &n.id);
            if tiers.len() > MAX_TIERS {
                return Err(bad(format!(
                    "node '{}' has more than {MAX_TIERS} recovery tiers",
                    n.id
                )));
            }
            let mut recovery_tiers = [tp::tier::NONE; MAX_TIERS];
            tiers
                .iter()
                .zip(recovery_tiers.iter_mut())
                .for_each(|(&tier, slot)| *slot = tier_code(tier));
            let order_rank = t
                .update
                .order
                .iter()
                .position(|id| *id == n.id)
                .ok_or_else(|| bad(format!("node '{}' is not in update.order", n.id)))?;
            Ok(TopoNode {
                index: i as u8,
                role: match n.role {
                    Role::Coordinator => tp::role::COORDINATOR,
                    Role::Node => tp::role::NODE,
                },
                mcu_id: reg
                    .mcu(&n.mcu)
                    .map(McuDef::mcu_id)
                    .ok_or_else(|| bad(format!("unknown MCU '{}'", n.mcu)))?,
                port_kind: port_kind_of(&n.port)
                    .ok_or_else(|| bad(format!("unknown port '{}'", n.port)))?,
                max_attempts: n.max_attempts.unwrap_or(DEFAULT_MAX_ATTEMPTS),
                listen_window_ms: n.listen_window_ms.unwrap_or(0),
                map_id: tp::map_id::DEFAULT,
                recovery_tiers,
                order_rank: order_rank as u8,
                target_tag: target_tag(&n.tag),
            })
        })
        .collect()
}

/// The end whose node a link's GPIOs (a UART's `pins`, `nrst`, `boot`) are
/// on; the compiled link holds one end's.
fn gpio_end(l: &Link) -> Result<Option<&End>, CompileError> {
    let names = |e: &End| {
        e.nrst.is_some() || e.boot.is_some() || (l.kind == LinkKind::Uart && e.pins.is_some())
    };
    match l.ends.each_ref().map(names) {
        [true, true] => Err(bad(format!(
            "link {}: both ends name GPIOs; the compiled topology holds one end's",
            l.named()
        ))),
        [true, false] => Ok(Some(&l.ends[0])),
        [false, true] => Ok(Some(&l.ends[1])),
        [false, false] => Ok(None),
    }
}

fn end(reg: &Registry, t: &Topology, e: &End) -> Result<TopoEnd, CompileError> {
    let node = index_of(t, &e.node)?;
    let port = t
        .node(&e.node)
        .and_then(|n| reg.mcu(&n.mcu))
        .and_then(|m| m.port(&e.port))
        .ok_or_else(|| bad(format!("'{}' has no port '{}'", e.node, e.port)))?;
    Ok(TopoEnd {
        node,
        port: port.index,
        addr: e.addr.unwrap_or(tp::NONE),
    })
}

fn link(reg: &Registry, t: &Topology, l: &Link) -> Result<TopoLink, CompileError> {
    let named = l.named();
    let missing = |what: &str| bad(format!("link {named} has no {what}"));
    let pin = |p: Option<u8>| p.unwrap_or(tp::NONE);
    let gpios = gpio_end(l)?;
    let (iface, tx_pin, rx_pin, baud, bus_id) = match l.kind {
        LinkKind::I2c => {
            let name = l.bus.as_deref().ok_or_else(|| missing("bus"))?;
            let bus_id = t
                .buses
                .iter()
                .position(|b| b.id == name)
                .ok_or_else(|| missing("known bus"))?;
            let bus = &t.buses[bus_id];
            (
                tp::iface::I2C_BUS,
                bus.sda,
                bus.scl,
                bus.i2c_hz(),
                bus_id as u8,
            )
        }
        LinkKind::Uart => {
            let pins = gpios.and_then(|e| e.pins).ok_or_else(|| missing("pins"))?;
            let iface = match l.iface.ok_or_else(|| missing("iface"))? {
                UartIface::HwUart0 => tp::iface::HW_UART0,
                UartIface::PioUart => tp::iface::PIO_UART,
            };
            let baud = l.uart_baud();
            (iface, pins.tx, pins.rx, baud, tp::NONE)
        }
        LinkKind::Can
        | LinkKind::Ethernet
        | LinkKind::UsbCdc
        | LinkKind::Ble
        | LinkKind::WifiTcp => {
            // updatable_with() leaves these out; reaching here is a caller's bug.
            return Err(bad(format!(
                "link {named}: updating uses I2C and UART links only (V-23)"
            )));
        }
    };
    let mut ends = [end(reg, t, &l.ends[0])?, end(reg, t, &l.ends[1])?];
    ends.sort_by_key(|e| e.node);
    let gpio_end = match gpios {
        Some(g) => {
            let at = index_of(t, &g.node)?;
            ends.iter().position(|e| e.node == at).unwrap_or(0) as u8
        }
        None => tp::NONE,
    };
    Ok(TopoLink {
        ends,
        iface,
        control: match l.control.kind {
            ControlKind::Full => tp::control::FULL,
            ControlKind::Cooperative => tp::control::COOPERATIVE,
        },
        tx_pin,
        rx_pin,
        nrst_pin: pin(gpios.and_then(|e| e.nrst)),
        boot_pin: pin(gpios.and_then(|e| e.boot)),
        baud,
        rom_entry: match &l.control.rom_entry {
            None => tp::rom_entry::NONE,
            Some(r) => reg.rom_entry_code(r.name()).ok_or_else(|| {
                bad(format!(
                    "link {named}: no definition names the ROM entry '{}' (V-01)",
                    r.name()
                ))
            })?,
        },
        bus_id,
        gpio_end,
    })
}

fn constraint(t: &Topology, c: &Constraint) -> Result<BinConstraint, CompileError> {
    let version = |s: &str| parse_version(s).ok_or_else(|| bad(format!("bad version '{s}'")));
    Ok(match c {
        Constraint::Lockstep(ids) => BinConstraint {
            kind: constraint_kind::LOCKSTEP_PROTO,
            // `MAX_NODES` is held to the mask's 32 bits where it is defined.
            node_mask: ids
                .iter()
                .map(|id| index_of(t, id).map(|i| 1u32 << i))
                .sum::<Result<u32, _>>()?,
            ..Default::default()
        },
        Constraint::Requires { node, needs, min } => BinConstraint {
            kind: constraint_kind::REQUIRES,
            node_a: index_of(t, node)?,
            node_b: index_of(t, needs)?,
            min_version: version(min)?,
            ..Default::default()
        },
        Constraint::MinBootcore { node, min } => BinConstraint {
            kind: constraint_kind::MIN_BOOTCORE,
            node_a: index_of(t, node)?,
            min_version: version(min)?,
            ..Default::default()
        },
    })
}

fn encode(reg: &Registry, t: &Topology) -> Result<Vec<u8>, CompileError> {
    let nodes = nodes(reg, t)?;
    let mut links = t
        .links
        .iter()
        .map(|l| link(reg, t, l))
        .collect::<Result<Vec<_>, _>>()?;
    links.sort_by_key(|l| (l.ends[0].node, l.ends[1].node));
    let constraints = t
        .constraints
        .iter()
        .map(|c| constraint(t, c))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = vec![0u8; tp::MAX_SIZE];
    let n =
        tp::encode(&nodes, &links, &constraints, &mut out).map_err(|e| bad(format!("{e:?}")))?;
    out.truncate(n);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_YAML: &str = include_str!("../../../topology/sample.yaml");

    #[test]
    fn sample_matches_golden_vector() {
        let golden = include_bytes!("../../dagon-format/golden/topology_sample.bin");
        assert_eq!(compile_str(SAMPLE_YAML).unwrap(), golden);
    }

    #[test]
    fn link_and_end_order_in_yaml_does_not_change_bytes() {
        let mut t = parse_str(SAMPLE_YAML).unwrap();
        t.links.reverse();
        assert_eq!(compile(&t).unwrap(), compile_str(SAMPLE_YAML).unwrap());
        t.links.iter_mut().for_each(|l| l.ends.reverse());
        assert_eq!(compile(&t).unwrap(), compile_str(SAMPLE_YAML).unwrap());
    }

    #[test]
    fn each_end_keeps_its_port_index_and_address() {
        let bin = compile_str(SAMPLE_YAML).unwrap();
        let v = tp::TopologyView::parse(&bin).unwrap();
        let seg = v.link(0);
        // hub: rp2040 port i2c0 (index 1); seg: stm32f401re i2c3 (index 3) at 0x42.
        assert_eq!(seg.end(0).map(|e| (e.port, e.addr)), Some((1, tp::NONE)));
        assert_eq!(seg.end(1).map(|e| (e.port, e.addr)), Some((3, 0x42)));
        let watch = v.link(1);
        assert_eq!(watch.end(2).map(|e| (e.port, e.addr)), Some((1, tp::NONE)));
        // The coordinator (end 0) names the reset lines and the UART pins.
        assert_eq!((seg.gpio_end, watch.gpio_end), (0, 0));
        let flipped = SAMPLE_YAML
            .replace(
                "{ node: watch, port: uart0 }",
                "{ node: watch, port: uart0, pins: { tx: 43, rx: 44 } }",
            )
            .replace(
                "{ node: hub, port: pio-uart, pins: { tx: 26, rx: 27 } }",
                "{ node: hub, port: pio-uart }",
            );
        let v2 = compile_str(&flipped).unwrap();
        let w = tp::TopologyView::parse(&v2).unwrap().link(1);
        assert_eq!((w.gpio_end, w.tx_pin, w.rx_pin), (1, 43, 44));
    }

    #[test]
    fn pins_named_on_both_ends_cannot_be_compiled() {
        let yaml = SAMPLE_YAML.replace(
            "{ node: watch, port: uart0 }",
            "{ node: watch, port: uart0, pins: { tx: 43, rx: 44 } }",
        );
        let e = compile_str(&yaml).unwrap_err().to_string();
        assert!(e.contains("both ends name GPIOs"), "{e}");
    }

    #[test]
    fn constraints_are_carried() {
        let yaml = format!("{SAMPLE_YAML}\nconstraints:\n  - lockstep: [hub, seg]\n");
        let bin = compile_str(&yaml).unwrap();
        let v = tp::TopologyView::parse(&bin).unwrap();
        assert_eq!(v.constraint_count(), 1);
        assert_eq!(v.constraint(0).node_mask, 0b011);
    }

    #[test]
    fn invalid_topology_is_rejected_before_encoding() {
        let yaml = SAMPLE_YAML.replace("dagon: 2", "dagon: 1");
        assert!(matches!(compile_str(&yaml), Err(CompileError::Invalid(_))));
    }
}
