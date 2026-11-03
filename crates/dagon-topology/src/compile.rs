//! `topo compile`: validated YAML model -> topology canonical binary
//! (layout in `dagon_format::topology`, shared with the hub).
//!
//! Mapping: node index = position in `nodes[]`; links sorted by `to`;
//! `bus_id` = position in `buses[]`; every flash map is its MCU's default
//! (`map_id` 0); omitted `listen_window_ms` = 0, `max_attempts` = 3, bus speed
//! = 400 kHz, baud = 115200. Node ids, the topology name, positions and
//! nicknames are not in the binary.

use std::fmt;

use dagon_format::header::target_tag;
use dagon_format::manifest::{constraint_kind, Constraint as BinConstraint};
use dagon_format::topology::{self as tp, TopoLink, TopoNode};
use dagon_format::Version;

use crate::defs::{PortKind, Registry};
use crate::schema::{Constraint, ControlKind, Link, LinkKind, Role, RomEntry, Tier, UartIface};
use crate::{check_with, effective_tiers, Report, Topology};

const DEFAULT_MAX_ATTEMPTS: u8 = 3;
const DEFAULT_I2C_HZ: u32 = 400_000;
const DEFAULT_BAUD: u32 = 115_200;
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
/// against the builtin definitions.
pub fn compile(t: &Topology) -> Result<Vec<u8>, CompileError> {
    compile_with(Registry::builtin(), t)
}

/// [`compile`] against the MCUs and boards of `reg`.
pub fn compile_with(reg: &Registry, t: &Topology) -> Result<Vec<u8>, CompileError> {
    let report = check_with(reg, t);
    if !report.is_pushable() {
        return Err(CompileError::Invalid(report));
    }
    encode(reg, t)
}

pub fn parse_str(yaml: &str) -> Result<Topology, CompileError> {
    yaml_serde::from_str(yaml).map_err(|e| CompileError::Parse(e.to_string()))
}

pub fn compile_str(yaml: &str) -> Result<Vec<u8>, CompileError> {
    compile(&parse_str(yaml)?)
}

/// Image header `mcu_id` for a topology MCU id (builtin definitions).
pub fn mcu_id_of(mcu: &str) -> Option<u16> {
    Registry::builtin().mcu(mcu).map(|m| m.wire_id)
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
                    .map(|m| m.wire_id)
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

fn link(t: &Topology, l: &Link) -> Result<TopoLink, CompileError> {
    let missing = |what: &str| bad(format!("link to '{}' has no {what}", l.to));
    let pin = |p: Option<u8>| p.unwrap_or(tp::NONE);
    let (iface, tx_pin, rx_pin, baud, i2c_addr, bus_id) = match l.kind {
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
                bus.speed_hz.unwrap_or(DEFAULT_I2C_HZ),
                l.addr.ok_or_else(|| missing("addr"))?,
                bus_id as u8,
            )
        }
        LinkKind::Uart => {
            let pins = l.pins.ok_or_else(|| missing("pins"))?;
            let iface = match l.iface.ok_or_else(|| missing("iface"))? {
                UartIface::HwUart0 => tp::iface::HW_UART0,
                UartIface::PioUart => tp::iface::PIO_UART,
            };
            let baud = l.baud.unwrap_or(DEFAULT_BAUD);
            (iface, pins.tx, pins.rx, baud, tp::NONE, tp::NONE)
        }
        LinkKind::Can | LinkKind::Ethernet => {
            return Err(bad(format!(
                "link to '{}' is not updatable yet (V-23)",
                l.to
            )));
        }
    };
    Ok(TopoLink {
        from: index_of(t, &l.from)?,
        to: index_of(t, &l.to)?,
        iface,
        control: match l.control.kind {
            ControlKind::Full => tp::control::FULL,
            ControlKind::Cooperative => tp::control::COOPERATIVE,
        },
        tx_pin,
        rx_pin,
        nrst_pin: pin(l.control.nrst),
        boot_pin: pin(l.control.boot),
        baud,
        rom_entry: match l.control.rom_entry {
            None => tp::rom_entry::NONE,
            Some(RomEntry::ForceDownloadBoot) => tp::rom_entry::FORCE_DOWNLOAD_BOOT,
            Some(RomEntry::M5pm1) => tp::rom_entry::M5PM1,
            Some(RomEntry::Stm32I2cRom) => tp::rom_entry::STM32_I2C_ROM,
        },
        i2c_addr,
        bus_id,
    })
}

fn constraint(t: &Topology, c: &Constraint) -> Result<BinConstraint, CompileError> {
    let version = |s: &str| parse_version(s).ok_or_else(|| bad(format!("bad version '{s}'")));
    Ok(match c {
        Constraint::Lockstep(ids) => BinConstraint {
            kind: constraint_kind::LOCKSTEP_PROTO,
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
        .map(|l| link(t, l))
        .collect::<Result<Vec<_>, _>>()?;
    links.sort_by_key(|l| l.to);
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

    const DESK_DEMO: &str = include_str!("../../../topology/desk-demo.yaml");

    #[test]
    fn desk_demo_matches_golden_vector() {
        let golden = include_bytes!("../../dagon-format/golden/topology_desk_demo.bin");
        assert_eq!(compile_str(DESK_DEMO).unwrap(), golden);
    }

    #[test]
    fn link_order_in_yaml_does_not_change_bytes() {
        let mut t = parse_str(DESK_DEMO).unwrap();
        t.links.reverse();
        assert_eq!(compile(&t).unwrap(), compile_str(DESK_DEMO).unwrap());
    }

    #[test]
    fn constraints_are_carried() {
        let yaml = format!("{DESK_DEMO}\nconstraints:\n  - lockstep: [hub, seg]\n");
        let bin = compile_str(&yaml).unwrap();
        let v = tp::TopologyView::parse(&bin).unwrap();
        assert_eq!(v.constraint_count(), 1);
        assert_eq!(v.constraint(0).node_mask, 0b011);
    }

    #[test]
    fn invalid_topology_is_rejected_before_encoding() {
        let yaml = DESK_DEMO.replace("dagon: 1", "dagon: 2");
        assert!(matches!(compile_str(&yaml), Err(CompileError::Invalid(_))));
    }
}
