//! Topology YAML schema (schema version 1).

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    pub dagon: u32,
    pub name: String,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub buses: Vec<Bus>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub recovery: BTreeMap<String, Vec<Tier>>,
    pub update: Update,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    /// Checked against the port registry by V-06, so kept as text.
    pub mcu: String,
    pub board: Option<String>,
    pub role: Role,
    /// Checked against the MCU by V-06, so kept as text.
    pub port: String,
    pub flash_map: Option<String>,
    pub max_attempts: Option<u8>,
    pub listen_window_ms: Option<u16>,
    pub tag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Coordinator,
    Node,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bus {
    pub id: String,
    pub kind: BusKind,
    pub controller: String,
    pub sda: u8,
    pub scl: u8,
    pub speed_hz: Option<u32>,
    /// `external` or `board`; missing or `none` is reported by V-20.
    pub pullups: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BusKind {
    I2c,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub kind: LinkKind,
    pub bus: Option<String>,
    pub addr: Option<u8>,
    pub node_pins: Option<NodePins>,
    pub iface: Option<UartIface>,
    pub pins: Option<UartPins>,
    pub baud: Option<u32>,
    pub control: Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    I2c,
    Uart,
}

/// Node-side I2C pin names (MCU pin names such as `PB4`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePins {
    pub sda: String,
    pub scl: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UartIface {
    HwUart0,
    PioUart,
}

/// Coordinator-side UART GPIOs.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UartPins {
    pub tx: u8,
    pub rx: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub kind: ControlKind,
    pub nrst: Option<u8>,
    pub boot: Option<u8>,
    pub rom_entry: Option<RomEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlKind {
    Full,
    Cooperative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RomEntry {
    ForceDownloadBoot,
    M5pm1,
    Stm32I2cRom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Local,
    Redistribute,
    RomForced,
    RomCoop,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub order: Vec<String>,
}

/// One of `lockstep: [ids]`, `requires: {node, needs, min}` or
/// `min_bootcore: {node, min}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "ConstraintEntry")]
pub enum Constraint {
    /// These nodes must all run the same release.
    Lockstep(Vec<String>),
    /// `node` needs `needs` at version `min` or later.
    Requires {
        node: String,
        needs: String,
        min: String,
    },
    /// `node` needs a boot core at version `min` or later.
    MinBootcore { node: String, min: String },
}

/// Single-key mapping form of `Constraint` as written in YAML.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstraintEntry {
    lockstep: Option<Vec<String>>,
    requires: Option<RequiresEntry>,
    min_bootcore: Option<MinBootcoreEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequiresEntry {
    node: String,
    needs: String,
    min: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MinBootcoreEntry {
    node: String,
    min: String,
}

impl TryFrom<ConstraintEntry> for Constraint {
    type Error = &'static str;

    fn try_from(e: ConstraintEntry) -> Result<Self, Self::Error> {
        match (e.lockstep, e.requires, e.min_bootcore) {
            (Some(ids), None, None) => Ok(Self::Lockstep(ids)),
            (None, Some(r), None) => Ok(Self::Requires {
                node: r.node,
                needs: r.needs,
                min: r.min,
            }),
            (None, None, Some(m)) => Ok(Self::MinBootcore {
                node: m.node,
                min: m.min,
            }),
            _ => Err("a constraint has exactly one of lockstep, requires, min_bootcore"),
        }
    }
}

impl Constraint {
    pub fn node_refs(&self) -> Vec<&str> {
        match self {
            Self::Lockstep(ids) => ids.iter().map(String::as_str).collect(),
            Self::Requires { node, needs, .. } => vec![node, needs],
            Self::MinBootcore { node, .. } => vec![node],
        }
    }
}

impl Topology {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// The link that reaches node `id`, if any.
    pub fn link_to(&self, id: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.to == id)
    }
}
