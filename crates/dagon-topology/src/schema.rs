//! Topology YAML schema (schema version 2).

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
    /// A coordinator's hop towards the PC (or whatever drives it); omitted: USB CDC
    /// to dagon on a PC, as before. Not part of the compiled topology.
    #[serde(default)]
    pub uplink: Option<Uplink>,
}

/// A coordinator's uplink: the kind of hop, and what is at its other end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Uplink {
    pub kind: LinkKind,
    #[serde(default)]
    pub host: UplinkHost,
}

impl Default for Uplink {
    fn default() -> Self {
        Uplink {
            kind: LinkKind::UsbCdc,
            host: UplinkHost::PcCli,
        }
    }
}

/// What drives a coordinator through its uplink.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UplinkHost {
    /// dagon on a PC.
    #[default]
    PcCli,
    /// DAGON Console in a browser (a PC or a phone).
    Browser,
    /// A custom device of the developer's own.
    Embedded,
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

/// The I2C bus speed a 構成 gets when its bus leaves `speed_hz` out.
pub const DEFAULT_I2C_HZ: u32 = 400_000;

impl Bus {
    /// `speed_hz`, or [`DEFAULT_I2C_HZ`] when left out.
    pub fn i2c_hz(&self) -> u32 {
        self.speed_hz.unwrap_or(DEFAULT_I2C_HZ)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BusKind {
    I2c,
}

/// A link between two nodes. It has no direction: which node reaches which over
/// it is decided by where the PC is connected, never by the order of `ends`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// The medium: `i2c`, `uart`, `can`, `ethernet`, `usb-cdc`, `ble`, `wifi-tcp`.
    pub kind: LinkKind,
    /// The two nodes it joins, each with the port it uses there (V-04, V-27).
    pub ends: [End; 2],
    /// An I2C link's bus: an `id` of `buses` (V-01).
    pub bus: Option<String>,
    /// A UART link's UART: `hw-uart0` or `pio-uart` (V-01).
    pub iface: Option<UartIface>,
    /// A UART link's bit rate; 115200 when left out (V-26).
    pub baud: Option<u32>,
    /// How a node is put into its ROM loader over this link: `kind` (`full`, `cooperative`) and
    /// `rom_entry` (`force-download-boot`, `m5pm1`, `stm32-i2c-rom`). An I2C or UART link needs
    /// `rom_entry` when it is cooperative, the default (V-11).
    #[serde(default)]
    pub control: Control,
}

/// One end of a link: a node and what the link uses on it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct End {
    /// A node `id`.
    pub node: String,
    /// A `[[link_port]]` name of the node's MCU definition, of the link's kind (V-27).
    pub port: String,
    /// The 7-bit address (0x08 to 0x77) the other end reaches this node at, on a kind with
    /// addresses (I2C); refused on any other kind (V-18). No two ends on one bus share one, and
    /// none takes the address of a ROM loader reached over that bus (V-18).
    pub addr: Option<u8>,
    /// This node's I2C pins by MCU pin name (`{ sda: PB4, scl: PA8 }`), on an end with `addr`
    /// (V-01): one of its board's I2C pairs (V-07).
    pub node_pins: Option<NodePins>,
    /// This node's UART pins as GPIO numbers (`{ tx: 26, rx: 27 }`); a UART link names them on at
    /// least one end, and a link of any other kind on none (V-01).
    pub pins: Option<UartPins>,
    /// This node's GPIO driving the other node's reset, for `control.kind: full`.
    pub nrst: Option<u8>,
    /// This node's GPIO driving the other node's boot pin, for `control.kind: full`.
    pub boot: Option<u8>,
}

impl End {
    /// Whether this end names the GPIOs that reset the other node into its ROM loader.
    pub fn drives_reset(&self) -> bool {
        self.nrst.is_some() && self.boot.is_some()
    }
}

/// The kind of one hop. A 構成 may name any; which ones an MCU's firmware
/// implements is in [`crate::links`] (the rest are "not available").
/// Generates [`LinkKind`] from the one list of link kinds
/// (`dagon_format::with_link_kinds!`).
macro_rules! link_kind_enum {
    (() $( $v:ident = $c:literal, $n:literal; )*) => {
        /// A hop's medium, by the name a 構成 uses.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
        pub enum LinkKind {
            $( #[serde(rename = $n)] $v, )*
        }

        impl LinkKind {
            /// Every kind, in the list's order.
            pub const ALL: &'static [LinkKind] = &[$(LinkKind::$v),*];

            /// The name a 構成 uses.
            pub fn name(self) -> &'static str {
                match self {
                    $( LinkKind::$v => $n, )*
                }
            }

            /// The variant of the firmware's `links::Kind` (the same list).
            pub fn variant(self) -> &'static str {
                match self {
                    $( LinkKind::$v => stringify!($v), )*
                }
            }
        }
    };
}

dagon_format::with_link_kinds!([link_kind_enum]());

impl LinkKind {
    /// Kinds the compiled topology can carry between a coordinator and a node linked to it.
    pub fn compiles(self) -> bool {
        matches!(self, LinkKind::I2c | LinkKind::Uart)
    }

    /// Kinds whose nodes are reached at a 7-bit address an end gives (`addr`).
    pub fn addressed(self) -> bool {
        matches!(self, LinkKind::I2c)
    }

    /// Kinds the other end enumerates before they carry a frame (USB): a
    /// link of such a kind cannot take part in a boot stage's listen
    /// window, which ends long before an enumeration.
    pub fn enumerated(self) -> bool {
        matches!(self, LinkKind::UsbCdc)
    }
}

/// A node's I2C pin names (MCU pin names such as `PB4`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

/// A node's UART GPIOs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UartPins {
    pub tx: u8,
    pub rx: u8,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    #[serde(default = "cooperative")]
    pub kind: ControlKind,
    pub rom_entry: Option<RomEntry>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlKind {
    Full,
    #[default]
    Cooperative,
}

fn cooperative() -> ControlKind {
    ControlKind::Cooperative
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

    /// A coordinator's uplink as used (the default when it names none).
    pub fn uplink(n: &Node) -> Uplink {
        n.uplink.unwrap_or_default()
    }

    /// The links with an end at node `id`, with their positions.
    pub fn links_at<'a>(&'a self, id: &'a str) -> impl Iterator<Item = (usize, &'a Link)> + 'a {
        self.links
            .iter()
            .enumerate()
            .filter(move |(_, l)| l.end(id).is_some())
    }

    /// The link joining `a` and `b`, if one does.
    pub fn link_between(&self, a: &str, b: &str) -> Option<&Link> {
        self.links
            .iter()
            .find(|l| l.other(a).is_some_and(|e| e.node == b))
    }

    /// The link a node `id` with `role: node` is reached over: the one joining it to a coordinator
    /// (the first listed when several do), else its first link; `None` for a
    /// coordinator or an unlinked node. Only a coordinator relays, so the firmware of a node with `role: node` is
    /// built for this link and any other link at it is one it would relay over.
    pub fn reached_over(&self, id: &str) -> Option<&Link> {
        if self.node(id)?.role == Role::Coordinator {
            return None;
        }
        let at = || self.links.iter().filter(|l| l.end(id).is_some());
        let to_hub = |l: &&Link| {
            l.other(id)
                .and_then(|e| self.node(&e.node))
                .is_some_and(|n| n.role == Role::Coordinator)
        };
        at().find(to_hub).or_else(|| at().next())
    }

    /// The links node `id` would relay over: every link at a coordinator, every link
    /// at a node with `role: node` but [`Topology::reached_over`].
    pub fn relayed_from<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Link> + 'a {
        let up = self.reached_over(id);
        self.links_at(id)
            .map(|(_, l)| l)
            .filter(move |l| !up.is_some_and(|u| std::ptr::eq(u, *l)))
    }

    /// The bit rate node `id`'s firmware runs its UART links at (`None` when
    /// it has none). Both ends of a link are built with this value, so they
    /// always agree. UART links at different rates on one node are refused:
    /// a node's firmware drives one UART link speed.
    pub fn uart_baud(&self, id: &str) -> Result<Option<u32>, String> {
        let mut rates = self
            .links_at(id)
            .filter(|(_, l)| l.kind == LinkKind::Uart)
            .map(|(_, l)| l.uart_baud());
        let first = rates.next();
        match rates.find(|b| Some(*b) != first) {
            Some(other) => Err(format!(
                "{id}: its UART links run at {} and {other} bit/s; give them one baud",
                first.unwrap_or_default()
            )),
            None => Ok(first),
        }
    }
}

/// A UART link's bit rate when the topology gives no `baud`.
pub const DEFAULT_BAUD: u32 = 115_200;

impl Link {
    /// `baud`, or [`DEFAULT_BAUD`] when left out.
    pub fn uart_baud(&self) -> u32 {
        self.baud.unwrap_or(DEFAULT_BAUD)
    }

    /// The end at node `id`, if the link has one there.
    pub fn end(&self, id: &str) -> Option<&End> {
        self.ends.iter().find(|e| e.node == id)
    }

    /// The end that is not at node `id`, when `id` is one of the ends.
    pub fn other(&self, id: &str) -> Option<&End> {
        let [a, b] = &self.ends;
        if a.node == id {
            Some(b)
        } else if b.node == id {
            Some(a)
        } else {
            None
        }
    }

    /// The end that names the GPIOs resetting the other node, if one does.
    pub fn resetting_end(&self) -> Option<&End> {
        self.ends.iter().find(|e| e.drives_reset())
    }

    /// `between 'a' and 'b'`: the link by its nodes, for messages.
    pub fn named(&self) -> String {
        format!(
            "between '{}' and '{}'",
            self.ends[0].node, self.ends[1].node
        )
    }
}
