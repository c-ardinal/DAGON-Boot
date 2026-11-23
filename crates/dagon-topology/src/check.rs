//! Validation rules for `dagon topo check`.
//!
//! Errors V-01..V-21 (no V-19), V-25, V-26 and V-27; warnings V-22 and V-24
//! (V-24 is an error for an uplink on a node with `role: node`); information V-23 (a valid part
//! that updating skips). Errors block compiling the topology; the others
//! do not.
//!
//! A link has two ends and no direction: every rule reads it the same
//! whichever end is listed first.

use crate::links::{self, Dir};
use std::collections::{BTreeMap, BTreeSet};

use crate::defs::{BoardDef, Coordinator, McuDef, PortKind, Registry};
use crate::report::{Issue, Report, Severity};
use crate::schema::{ControlKind, End, Link, LinkKind, Node, Role, Tier, Topology, UartIface};

pub const SCHEMA_VERSION: u32 = 2;
/// Nodes a 構成 may describe (and the editor draw).
use dagon_format::topology::MAX_NODES;
const MAX_ID_LEN: usize = 15;
const MAX_TAG_LEN: usize = 16;
/// Standard and fast mode; each ROM I2C bootloader's `max_hz` caps it further.
const I2C_ADDR_MIN: u8 = 0x08;
const I2C_ADDR_MAX: u8 = 0x77;

/// Parse YAML and run every rule against the MCUs and boards of `reg`. A
/// document that does not parse yields a single V-01 error.
pub fn check_str_with(reg: &Registry, yaml: &str) -> Report {
    match yaml_serde::from_str::<Topology>(yaml) {
        Ok(t) => check_with(reg, &t),
        Err(e) => Report {
            issues: vec![Issue {
                rule: "V-01",
                severity: Severity::Error,
                path: String::new(),
                message: format!("does not match the topology schema: {e}"),
            }],
        },
    }
}

/// Every rule against the MCUs and boards of `reg`.
pub fn check_with(reg: &Registry, t: &Topology) -> Report {
    let mut c = Ctx {
        t,
        reg,
        issues: Vec::new(),
    };
    c.v01();
    c.v02();
    c.v03();
    c.v04();
    c.v05();
    c.v06();
    c.v07_v08();
    c.v07_every_end();
    c.v09();
    c.v10();
    c.v11();
    c.v12();
    c.v13();
    c.v14();
    c.v15();
    c.v16();
    c.v17();
    c.v18();
    c.v20();
    c.v21();
    c.v22();
    c.v23();
    c.v24();
    c.v25();
    c.v26();
    c.v27();
    Report { issues: c.issues }
}

/// True when node `id` is an end of a full-control link whose other end
/// names no NRST and BOOT GPIOs (V-22): the node is then treated as having
/// no `rom-forced` tier.
pub fn rom_forced_unavailable(link: &Link, id: &str) -> bool {
    link.control.kind == ControlKind::Full && !link.other(id).is_some_and(End::drives_reset)
}

/// Recovery tiers for `id` as they will be used, i.e. without `rom-forced`
/// when V-22 applies.
pub fn effective_tiers(t: &Topology, id: &str) -> Vec<Tier> {
    let dropped = t.links_at(id).any(|(_, l)| rom_forced_unavailable(l, id));
    t.recovery
        .get(id)
        .map(|tiers| {
            tiers
                .iter()
                .copied()
                .filter(|&tier| !(dropped && tier == Tier::RomForced))
                .collect()
        })
        .unwrap_or_default()
}

/// The board a node names. Pin and flash-size checks use only this (P1): a
/// node chosen by MCU is checked against the MCU's GPIO range, and its flash
/// size against the project's `[board.<id>]`, not against a stand-in board.
fn named_board<'r>(reg: &'r Registry, n: &Node) -> Option<&'r BoardDef> {
    n.board.as_deref().and_then(|id| reg.board(id))
}

/// What V-07 holds against `gpio` on `n`'s own end of a link: nothing when the node's
/// definition defines it. A named board judges by its header (one naming no numbered GPIO
/// judges nothing); a node named by MCU alone judges by that MCU's GPIO range, never by a
/// stand-in board.
pub fn pin_fault(reg: &Registry, mcu: &str, board: Option<&str>, gpio: u8) -> Option<String> {
    let name = format!("GPIO{gpio}");
    if let Some(b) = board.and_then(|id| reg.board(id)) {
        if !b.header.iter().any(|h| h.pin.starts_with("GPIO")) {
            return None;
        }
        if b.header.iter().any(|h| h.pin == name) {
            return None;
        }
        let avail: Vec<_> = b.header.iter().map(|h| h.pin.as_str()).collect();
        return Some(format!(
            "{name} is not on the {} header {avail:?}",
            b.product
        ));
    }
    let m = reg.mcu(mcu).filter(|m| m.gpio_count > 0)?;
    (gpio >= m.gpio_count)
        .then(|| format!("{name} is not a {} GPIO (0-{})", m.id, m.gpio_count - 1))
}

/// The pins a hw-uart0 link from `n` may use: its MCU's UART0 TX pins and RX pins (V-09),
/// each one its own definition defines (V-07). The Console offers exactly these.
pub fn uart0_choices(reg: &Registry, mcu: &str, board: Option<&str>) -> (Vec<u8>, Vec<u8>) {
    let Some(c) = reg.mcu(mcu).and_then(|m| m.coordinator.as_ref()) else {
        return (Vec::new(), Vec::new());
    };
    let ok = |pins: &[u8]| {
        pins.iter()
            .copied()
            .filter(|&g| pin_fault(reg, mcu, board, g).is_none())
            .collect()
    };
    (ok(&c.uart0_tx), ok(&c.uart0_rx))
}

/// The `[[link_port]]` a new end of a `kind` link on a node of `mcu` (and
/// `board`, else the MCU's default board) starts at, from its definitions: the
/// port of the board's own DAGON bus of that kind, else of the coordinator's I2C pins
/// the board names, else the port named like the link's UART `iface` (`hw-`
/// and the port, or the port itself), else the MCU's first port of the kind.
/// The Console offers this one; the check (V-27) judges whatever is written.
pub fn default_port<'r>(
    reg: &'r Registry,
    mcu: &str,
    board: Option<&str>,
    kind: LinkKind,
    iface: Option<UartIface>,
) -> Option<&'r str> {
    use crate::defs::board_boot::BusKind;
    let m = reg.mcu(mcu)?;
    let of_kind = |name: &str| {
        m.port(name)
            .filter(|p| p.kind == kind)
            .map(|p| p.name.as_str())
    };
    let b = board
        .and_then(|id| reg.board(id))
        .or_else(|| reg.default_board(mcu));
    let bus_kind = match kind {
        LinkKind::I2c => Some(BusKind::I2c),
        LinkKind::Uart => Some(BusKind::Uart),
        LinkKind::Can => Some(BusKind::Can),
        _ => None,
    };
    let own_bus = b.into_iter().flat_map(|b| &b.buses).find_map(|x| {
        (x.dagon && Some(x.kind) == bus_kind)
            .then(|| of_kind(&x.controller))
            .flatten()
    });
    let hub_i2c = b
        .and_then(|b| b.coordinator.as_ref())
        .filter(|_| kind == LinkKind::I2c)
        .and_then(|c| of_kind(&c.i2c.controller));
    let iface = iface.and_then(|i| {
        let name = match i {
            UartIface::HwUart0 => "hw-uart0",
            UartIface::PioUart => "pio-uart",
        };
        of_kind(name).or_else(|| name.strip_prefix("hw-").and_then(of_kind))
    });
    own_bus.or(hub_i2c).or(iface).or_else(|| {
        m.ports
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| p.name.as_str())
    })
}

/// Board for a node's capabilities (I2C target, ROM loader pins): the named
/// one, else its MCU's default board (`Registry::default_board`).
fn board_for<'r>(reg: &'r Registry, n: &Node) -> Option<&'r BoardDef> {
    match &n.board {
        Some(id) => reg.board(id),
        None => reg.default_board(&n.mcu),
    }
}

struct Ctx<'a> {
    t: &'a Topology,
    reg: &'a Registry,
    issues: Vec<Issue>,
}

/// What a clock's tolerance adds to a rate `error` off, as a check message
/// states it after the error ("" on an exact clock).
fn with_tolerance(error: f64, tolerance: f64) -> String {
    if tolerance > 0.0 {
        format!(
            ", up to {:.2} % including its clock's {:.2} %",
            (error.abs() + tolerance) * 100.0,
            tolerance * 100.0
        )
    } else {
        String::new()
    }
}

fn link_path(i: usize) -> String {
    format!("links[{i}]")
}

fn end_path(i: usize, k: usize) -> String {
    format!("links[{i}].ends[{k}]")
}

/// `'a' and 'b'`, in name order: a pair of nodes said the same whichever end comes first.
fn pair(a: &str, b: &str) -> String {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    format!("'{a}' and '{b}'")
}

impl<'a> Ctx<'a> {
    fn err(&mut self, rule: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            rule,
            severity: Severity::Error,
            path: path.into(),
            message: message.into(),
        });
    }

    fn warn(&mut self, rule: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            rule,
            severity: Severity::Warning,
            path: path.into(),
            message: message.into(),
        });
    }

    /// Hops whose kind the MCU firmware at either end does not run
    /// (未対応のリンク): accepted and drawn, reported by name;
    /// `dagon gen` refuses them. An uplink on a node with `role: node` is an error.
    fn v24(&mut self) {
        for (i, n) in self.t.nodes.iter().enumerate() {
            let Some(up) = n.uplink else { continue };
            if n.role != Role::Coordinator {
                self.err(
                    "V-24",
                    format!("nodes[{i}].uplink"),
                    format!(
                        "'{}' has `role: node`; only a node with `role: coordinator` has an uplink",
                        n.id
                    ),
                );
                continue;
            }
            let reg = self.reg;
            if links::known_in(reg, &n.mcu)
                && !links::available_in(reg, &n.mcu, Dir::Up).contains(&up.kind)
            {
                self.warn(
                    "V-24",
                    format!("nodes[{i}].uplink.kind"),
                    format!(
                        "uplink of '{}' over {}: the firmware of {} does not run this link kind",
                        n.id,
                        up.kind.name(),
                        n.mcu
                    ),
                );
            }
        }
        for (i, l) in self.links() {
            let [Some(a), Some(b)] = l.ends.each_ref().map(|e| self.t.node(&e.node)) else {
                continue;
            };
            // The firmware's kinds are by direction; the link has none, so it
            // is available when one end runs it down and the other up.
            let missing = |down: &'a Node, up: &'a Node| -> Vec<&'a str> {
                [(down, Dir::Down), (up, Dir::Up)]
                    .into_iter()
                    .filter(|(n, d)| {
                        links::known_in(self.reg, &n.mcu)
                            && !links::available_in(self.reg, &n.mcu, *d).contains(&l.kind)
                    })
                    .map(|(n, _)| n.mcu.as_str())
                    .collect()
            };
            let (ab, ba) = (missing(a, b), missing(b, a));
            let mut missing = if ba.len() < ab.len() { ba } else { ab };
            missing.sort_unstable();
            if !missing.is_empty() {
                self.warn(
                    "V-24",
                    format!("{}.kind", link_path(i)),
                    format!(
                        "link between {} over {}: the firmware of {} does not run this link kind",
                        pair(&a.id, &b.id),
                        l.kind.name(),
                        missing.join(", ")
                    ),
                );
            }
        }
    }

    fn coordinator(&self) -> Option<&'a Node> {
        self.t.nodes.iter().find(|n| n.role == Role::Coordinator)
    }

    fn coordinator_board(&self) -> Option<&'a BoardDef> {
        self.coordinator().and_then(|c| named_board(self.reg, c))
    }

    fn coordinator_mcu(&self) -> Option<&'a McuDef> {
        self.coordinator().and_then(|c| self.reg.mcu(&c.mcu))
    }

    /// What the coordinator's MCU offers its links; `None` when its
    /// definition has no `[coordinator]` data (then those checks are skipped).
    fn coordinator_data(&self) -> Option<&'a Coordinator> {
        self.coordinator_mcu().and_then(|m| m.coordinator.as_ref())
    }

    fn node_board(&self, id: &str) -> Option<&'a BoardDef> {
        self.t.node(id).and_then(|n| board_for(self.reg, n))
    }

    /// The ROM I2C bootloader of the node `id` when it is reached through
    /// its ROM (`rom-forced`).
    fn forced_rom(&self, id: &str) -> Option<&'a crate::defs::RomI2c> {
        let n = self.t.node(id)?;
        if !effective_tiers(self.t, id).contains(&Tier::RomForced) {
            return None;
        }
        self.reg.mcu(&n.mcu)?.rom_i2c.as_ref()
    }

    fn links(&self) -> impl Iterator<Item = (usize, &'a Link)> + 'a {
        self.t.links.iter().enumerate()
    }

    /// Links with the first coordinator at one end, over I2C or UART, with the
    /// position and the end at the coordinator: the ones its own pins, controllers
    /// and PIO serve, whichever end it is listed as. Other links (between
    /// coordinator, deeper, CAN, Ethernet) are reported by V-23 instead.
    fn hub_links(&self) -> impl Iterator<Item = (usize, &'a Link, usize, &'a End)> + 'a {
        let hub = self.coordinator().map(|c| c.id.as_str());
        self.t.links.iter().enumerate().filter_map(move |(i, l)| {
            let k = l.ends.iter().position(|e| Some(e.node.as_str()) == hub)?;
            matches!(l.kind, LinkKind::I2c | LinkKind::Uart).then(|| (i, l, k, &l.ends[k]))
        })
    }

    /// Whether end `k` of link `i` is the first coordinator's end of one of its [`Self::hub_links`].
    fn is_hub_end(&self, i: usize, k: usize) -> bool {
        self.hub_links().any(|(j, _, e, _)| (j, e) == (i, k))
    }

    /// Every end of every link: link position, the link, end position, the end.
    fn ends(&self) -> impl Iterator<Item = (usize, &'a Link, usize, &'a End)> + 'a {
        self.links()
            .flat_map(|(i, l)| l.ends.iter().enumerate().map(move |(k, e)| (i, l, k, e)))
    }

    /// Schema version and the fields the schema requires only in some cases.
    fn v01(&mut self) {
        let t = self.t;
        if t.dagon != SCHEMA_VERSION {
            self.err(
                "V-01",
                "dagon",
                format!("schema version must be {SCHEMA_VERSION}, got {}", t.dagon),
            );
        }
        for (i, n) in t.nodes.iter().enumerate() {
            if n.tag.is_empty() || n.tag.len() > MAX_TAG_LEN {
                self.err(
                    "V-01",
                    format!("nodes[{i}].tag"),
                    format!("tag must be 1-{MAX_TAG_LEN} characters"),
                );
            }
            if n.max_attempts.is_some_and(|a| !(1..=5).contains(&a)) {
                self.err(
                    "V-01",
                    format!("nodes[{i}].max_attempts"),
                    "max_attempts must be 1-5",
                );
            }
            if n.listen_window_ms.is_some_and(|w| w > 200) {
                self.err(
                    "V-01",
                    format!("nodes[{i}].listen_window_ms"),
                    "listen_window_ms must be 0-200",
                );
            }
        }
        let mut bus_ids = BTreeSet::new();
        for (i, b) in t.buses.iter().enumerate() {
            if !bus_ids.insert(b.id.as_str()) {
                self.err(
                    "V-01",
                    format!("buses[{i}].id"),
                    format!("duplicate bus id '{}'", b.id),
                );
            }
        }
        for (i, l) in self.links() {
            let p = link_path(i);
            // An end's `pins` are UART pins: the compiled topology holds them on a UART link only.
            if l.kind != LinkKind::Uart {
                for (k, _) in l.ends.iter().enumerate().filter(|(_, e)| e.pins.is_some()) {
                    self.err(
                        "V-01",
                        format!("{}.pins", end_path(i, k)),
                        format!("'pins' are UART pins; this {} link has none", l.kind.name()),
                    );
                }
            }
            match l.kind {
                LinkKind::I2c => {
                    match &l.bus {
                        None => self.err("V-01", format!("{p}.bus"), "i2c link needs 'bus'"),
                        Some(b) if !bus_ids.contains(b.as_str()) => {
                            self.err("V-01", format!("{p}.bus"), format!("unknown bus '{b}'"))
                        }
                        Some(_) => {}
                    }
                    if l.ends.iter().all(|e| e.addr.is_none()) {
                        self.err(
                            "V-01",
                            format!("{p}.ends"),
                            "i2c link needs 'addr' on the end the other end reaches",
                        );
                    }
                    for (k, e) in l.ends.iter().enumerate() {
                        if e.addr.is_some() && e.node_pins.is_none() {
                            self.err(
                                "V-01",
                                format!("{}.node_pins", end_path(i, k)),
                                "an i2c end with 'addr' needs 'node_pins' (sda, scl)",
                            );
                        }
                    }
                }
                LinkKind::Uart => {
                    if l.iface.is_none() {
                        self.err("V-01", format!("{p}.iface"), "uart link needs 'iface'");
                    }
                    if l.ends.iter().all(|e| e.pins.is_none()) {
                        self.err(
                            "V-01",
                            format!("{p}.ends"),
                            "uart link needs 'pins' (tx, rx) on an end",
                        );
                    }
                }
                // Described only; V-23 says updating skips them.
                LinkKind::Can
                | LinkKind::Ethernet
                | LinkKind::UsbCdc
                | LinkKind::Ble
                | LinkKind::WifiTcp => {}
            }
        }
    }

    fn v02(&mut self) {
        let n = self.t.nodes.len();
        if !(1..=MAX_NODES).contains(&n) {
            self.err(
                "V-02",
                "nodes",
                format!("need 1-{MAX_NODES} nodes, got {n}"),
            );
        }
        let mut seen = BTreeSet::new();
        for (i, node) in self.t.nodes.iter().enumerate() {
            let id = node.id.as_str();
            let well_formed = !id.is_empty()
                && id.len() <= MAX_ID_LEN
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if !well_formed {
                self.err(
                    "V-02",
                    format!("nodes[{i}].id"),
                    format!("id '{id}' must be 1-{MAX_ID_LEN} of a-z, 0-9, '-'"),
                );
            }
            if !seen.insert(id) {
                self.err(
                    "V-02",
                    format!("nodes[{i}].id"),
                    format!("duplicate node id '{id}'"),
                );
            }
        }
    }

    fn v03(&mut self) {
        let count = self
            .t
            .nodes
            .iter()
            .filter(|n| n.role == Role::Coordinator)
            .count();
        // Updating skips the trees of the coordinators after the first (V-23).
        if count == 0 {
            self.err("V-03", "nodes", "need a coordinator, got none");
        }
    }

    fn v04(&mut self) {
        for (i, _, k, e) in self.ends() {
            if self.t.node(&e.node).is_none() {
                self.err(
                    "V-04",
                    format!("{}.node", end_path(i, k)),
                    format!("unknown node '{}'", e.node),
                );
            }
        }
    }

    fn info(&mut self, rule: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(Issue {
            rule,
            severity: Severity::Info,
            path: path.into(),
            message: message.into(),
        });
    }

    /// Valid shapes that updating does not reach, told as information
    /// (never errors): more than one coordinator and their trees, nodes
    /// linked through another node and links of a kind the compiled topology cannot carry.
    /// Updating covers one coordinator with the nodes linked straight to it
    /// and skips the rest (see [`crate::updatable`]).
    fn v23(&mut self) {
        let hubs: Vec<&str> = self
            .t
            .nodes
            .iter()
            .filter(|n| n.role == Role::Coordinator)
            .map(|n| n.id.as_str())
            .collect();
        if hubs.len() > 1 {
            self.info(
                "V-23",
                "nodes",
                format!(
                    "{} coordinators {hubs:?}; updating reaches '{}' and skips the others' trees",
                    hubs.len(),
                    hubs[0]
                ),
            );
        }
        let first = hubs.first().copied();
        for (i, l) in self.links() {
            let p = link_path(i);
            let [a, b] = l.ends.each_ref().map(|e| e.node.as_str());
            if !l.kind.compiles() {
                self.info(
                    "V-23",
                    format!("{p}.kind"),
                    format!(
                        "{} link between {}: updating uses I2C and UART links only and skips it",
                        l.kind.name(),
                        pair(a, b)
                    ),
                );
            }
            let role = |id: &str| self.t.node(id).map(|n| n.role);
            let hubs_at: Vec<&str> = [a, b]
                .into_iter()
                .filter(|id| role(id) == Some(Role::Coordinator))
                .collect();
            let message = match (first.filter(|f| [a, b].contains(f)), hubs_at.as_slice()) {
                // Unknown nodes are V-04's.
                _ if role(a).is_none() || role(b).is_none() => None,
                (Some(f), [_, _]) => Some(format!(
                    "'{}' is a coordinator linked to the coordinator '{f}': updating reaches the first coordinator's tree only and skips it",
                    if a == f { b } else { a }
                )),
                (Some(_), _) => None,
                (None, []) => Some(format!(
                    "the link between the nodes {} is one link further from the coordinator: updating reaches only the nodes linked straight to the coordinator and skips it",
                    pair(a, b)
                )),
                (None, [h, ..]) => Some(format!(
                    "the link between {} is under the second coordinator '{h}': updating reaches the first coordinator's tree only and skips it",
                    pair(a, b)
                )),
            };
            if let Some(m) = message {
                self.info("V-23", format!("{p}.ends"), m);
            }
        }
    }

    /// The coordinator's MCU must define what it offers as one (its
    /// `[coordinator]` section). Only the first coordinator, the one the hub
    /// rules check: updating skips the further ones (V-23).
    /// Unknown MCUs are V-06's.
    fn v25(&mut self) {
        let first = self
            .t
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.role == Role::Coordinator);
        if let Some((i, n)) = first {
            let Some(m) = self.reg.mcu(&n.mcu) else {
                return;
            };
            if m.coordinator.is_none() {
                self.err(
                    "V-25",
                    format!("nodes[{i}].mcu"),
                    format!(
                        "{} cannot be the coordinator: its MCU definition has no [coordinator] section",
                        m.id
                    ),
                );
            }
        }
    }

    fn v05(&mut self) {
        for (i, n) in self
            .t
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.role == Role::Node)
        {
            if self.t.links_at(&n.id).next().is_none() {
                self.err(
                    "V-05",
                    format!("nodes[{i}]"),
                    format!("node '{}' needs a link, has none", n.id),
                );
            }
        }
        let links = &self.t.links;
        for (i, l) in links.iter().enumerate() {
            let [a, b] = l.ends.each_ref().map(|e| e.node.as_str());
            if a == b {
                self.err(
                    "V-05",
                    format!("{}.ends", link_path(i)),
                    format!("a link joins two nodes; both ends are '{a}'"),
                );
            } else if let Some(j) = links[..i]
                .iter()
                .position(|o| o.other(a).is_some_and(|e| e.node == b))
            {
                self.err(
                    "V-05",
                    format!("{}.ends", link_path(i)),
                    format!(
                        "links[{j}] and links[{i}] both join {}: one link per pair of nodes",
                        pair(a, b)
                    ),
                );
            }
        }
    }

    fn v06(&mut self) {
        for (i, n) in self.t.nodes.iter().enumerate() {
            match self.reg.mcu(&n.mcu) {
                None => {
                    let known: Vec<_> = self.reg.mcus().iter().map(|m| m.id.as_str()).collect();
                    self.err(
                        "V-06",
                        format!("nodes[{i}].mcu"),
                        format!("mcu '{}' is not in the port registry {known:?}", n.mcu),
                    );
                }
                Some(m) if m.port.as_str() != n.port => {
                    self.err(
                        "V-06",
                        format!("nodes[{i}].port"),
                        format!("{} uses port '{}', not '{}'", m.id, m.port.as_str(), n.port),
                    );
                }
                Some(_) => {}
            }
            if let Some(b) = &n.board {
                match self.reg.board(b) {
                    None => self.err(
                        "V-06",
                        format!("nodes[{i}].board"),
                        format!("unknown board '{b}'"),
                    ),
                    Some(board) if board.mcu != n.mcu => self.err(
                        "V-06",
                        format!("nodes[{i}].board"),
                        format!("board '{b}' carries {}, not {}", board.mcu, n.mcu),
                    ),
                    Some(_) => {}
                }
            }
        }
    }

    /// Every coordinator GPIO the topology uses, with its YAML path.
    fn coordinator_pins(&self) -> Vec<(String, u8)> {
        let mut pins = Vec::new();
        for (i, b) in self.t.buses.iter().enumerate() {
            pins.push((format!("buses[{i}].sda"), b.sda));
            pins.push((format!("buses[{i}].scl"), b.scl));
        }
        for (i, _, k, e) in self.hub_links() {
            pins.extend(own_gpios(&end_path(i, k), e, true));
        }
        pins
    }

    /// Every pin on every link end is one its own node's definition defines:
    /// its `pins`, `nrst` and `boot` on its board's header (or its MCU's GPIO
    /// range when it names only an MCU), its `node_pins` as one of its board's
    /// I2C pairs. The first coordinator's ends of its links are covered by `v07_v08`.
    fn v07_every_end(&mut self) {
        for (i, l, k, e) in self.ends() {
            let ep = end_path(i, k);
            if !self.is_hub_end(i, k) {
                // A hw-uart0 link runs on the MCU's own UART0 pair, judged below (V-09), not on the header.
                let pins = own_gpios(&ep, e, l.iface != Some(UartIface::HwUart0));
                let node = self.t.node(&e.node);
                if let Some(n) = node {
                    for (path, gpio) in pins {
                        if let Some(msg) = pin_fault(self.reg, &n.mcu, n.board.as_deref(), gpio) {
                            self.err("V-07", path, msg);
                        }
                    }
                }
                if l.iface == Some(UartIface::HwUart0) {
                    let uart0 = node
                        .and_then(|n| self.reg.mcu(&n.mcu))
                        .and_then(|m| m.coordinator.as_ref());
                    if let (Some(c), Some(u)) = (uart0, e.pins) {
                        if !c.uart0_tx.contains(&u.tx) || !c.uart0_rx.contains(&u.rx) {
                            let msg = format!("GPIO{}/{} is not a UART0 TX/RX pair", u.tx, u.rx);
                            self.err("V-09", format!("{ep}.pins"), msg);
                        }
                    }
                }
            }
            if let (Some(np), Some(b)) = (e.node_pins.as_ref(), self.node_board(&e.node)) {
                if !b
                    .i2c_pairs
                    .iter()
                    .any(|r| r.sda == np.sda && r.scl == np.scl)
                {
                    let pairs: Vec<_> = b
                        .i2c_pairs
                        .iter()
                        .map(|r| format!("{}/{}", r.sda, r.scl))
                        .collect();
                    let msg = format!(
                        "{}/{} is not an I2C pair of {}: {pairs:?}",
                        np.sda, np.scl, b.product
                    );
                    self.err("V-07", format!("{ep}.node_pins"), msg);
                }
            }
        }
    }

    fn v07_v08(&mut self) {
        let pins = self.coordinator_pins();
        if let Some(board) = self.coordinator_board() {
            for (path, gpio) in &pins {
                let name = format!("GPIO{gpio}");
                if !board.header.iter().any(|h| h.pin == name) {
                    let avail: Vec<_> = board.header.iter().map(|h| h.pin.as_str()).collect();
                    self.err(
                        "V-07",
                        path.clone(),
                        format!("{name} is not on the {} header {avail:?}", board.product),
                    );
                }
            }
        } else if let Some(mcu) = self.coordinator_mcu().filter(|m| m.gpio_count > 0) {
            // P1: a coordinator chosen by MCU is checked against the MCU's GPIOs.
            for (path, gpio) in &pins {
                if *gpio >= mcu.gpio_count {
                    let msg = format!(
                        "GPIO{gpio} is not a {} GPIO (0-{})",
                        mcu.id,
                        mcu.gpio_count - 1
                    );
                    self.err("V-07", path.clone(), msg);
                }
            }
        }
        let mut first: BTreeMap<u8, &str> = BTreeMap::new();
        for (path, gpio) in &pins {
            match first.get(gpio) {
                Some(prev) => {
                    let msg = format!("GPIO{gpio} is already used by {prev}");
                    self.err("V-08", path.clone(), msg);
                }
                None => {
                    first.insert(*gpio, path);
                }
            }
        }
    }

    fn v09(&mut self) {
        let bus_pins: BTreeSet<u8> = self.t.buses.iter().flat_map(|b| [b.sda, b.scl]).collect();
        let hub = self.coordinator_data();
        for (i, l, k, e) in self.hub_links() {
            let (Some(UartIface::HwUart0), Some(u)) = (l.iface, e.pins) else {
                continue;
            };
            let p = end_path(i, k);
            let not_uart0 =
                |c: &Coordinator| !c.uart0_tx.contains(&u.tx) || !c.uart0_rx.contains(&u.rx);
            if hub.is_some_and(not_uart0) {
                self.err(
                    "V-09",
                    format!("{p}.pins"),
                    format!("GPIO{}/{} is not a UART0 TX/RX pair", u.tx, u.rx),
                );
            }
            for g in [u.tx, u.rx].into_iter().filter(|g| bus_pins.contains(g)) {
                self.err(
                    "V-09",
                    format!("{p}.pins"),
                    format!("GPIO{g} is taken by I2C; use pio-uart"),
                );
            }
        }
    }

    fn v10(&mut self) {
        let pio_links = self
            .hub_links()
            .filter(|(_, l, _, _)| l.iface == Some(UartIface::PioUart))
            .count();
        let Some(hub) = self.coordinator_data() else {
            return;
        };
        let need = pio_links * usize::from(hub.pio_uart_state_machines);
        let have = usize::from(hub.pio_state_machines);
        if need > have {
            self.err(
                "V-10",
                "links",
                format!("{pio_links} pio-uart links need {need} PIO state machines, {have} exist"),
            );
        }
    }

    fn v11(&mut self) {
        for (i, l) in self
            .links()
            .filter(|(_, l)| matches!(l.kind, LinkKind::I2c | LinkKind::Uart))
        {
            if l.control.kind == ControlKind::Cooperative && l.control.rom_entry.is_none() {
                self.err(
                    "V-11",
                    format!("{}.control.rom_entry", link_path(i)),
                    "cooperative control needs 'rom_entry'",
                );
            }
        }
    }

    fn v12(&mut self) {
        let t = self.t;
        for id in t.recovery.keys() {
            let path = format!("recovery.{id}");
            let Some(node) = t.node(id) else {
                self.err("V-12", path, format!("recovery for unknown node '{id}'"));
                continue;
            };
            let controls: Vec<ControlKind> = t.links_at(id).map(|(_, l)| l.control.kind).collect();
            let control = |c: ControlKind| controls.contains(&c);
            for tier in effective_tiers(t, id) {
                match tier {
                    Tier::Local | Tier::Redistribute => {}
                    Tier::RomForced if !control(ControlKind::Full) => {
                        self.err(
                            "V-12",
                            path.clone(),
                            "rom-forced needs a link with full control",
                        );
                    }
                    Tier::RomCoop
                        if !control(ControlKind::Cooperative)
                            || PortKind::from_name(&node.port) != Some(PortKind::VendorStage2) =>
                    {
                        self.err(
                            "V-12",
                            path.clone(),
                            "rom-coop needs a vendor-stage2 node on a cooperative link",
                        );
                    }
                    Tier::RomForced | Tier::RomCoop => {}
                }
            }
        }
    }

    /// A node reached through its ROM loader (`rom-forced`) is on an I2C link
    /// whose end at it is on a ROM loader pin pair.
    fn v13(&mut self) {
        let t = self.t;
        for n in &t.nodes {
            if !effective_tiers(t, &n.id).contains(&Tier::RomForced) {
                continue;
            }
            let Some(board) = self.node_board(&n.id) else {
                continue;
            };
            // The ROM pairs are the board MCU's (the board brings them out).
            let rom_pairs = self
                .reg
                .mcu(&board.mcu)
                .and_then(|m| m.rom_i2c.as_ref())
                .map_or(&[][..], |r| &r.pairs[..]);
            let on_rom_pins = |l: &Link| {
                l.kind == LinkKind::I2c
                    && l.end(&n.id)
                        .and_then(|e| e.node_pins.as_ref())
                        .is_some_and(|np| {
                            rom_pairs.iter().any(|r| r.sda == np.sda && r.scl == np.scl)
                        })
            };
            let at: Vec<(usize, &Link)> = t.links_at(&n.id).collect();
            let Some(&(i, first)) = at.first() else {
                continue;
            };
            if !at.iter().any(|(_, l)| on_rom_pins(l)) {
                let k = first.ends.iter().position(|e| e.node == n.id).unwrap_or(0);
                let pairs: Vec<_> = rom_pairs
                    .iter()
                    .map(|r| format!("{}/{}", r.sda, r.scl))
                    .collect();
                self.err(
                    "V-13",
                    format!("{}.node_pins", end_path(i, k)),
                    format!("rom-forced needs an I2C link on a ROM bootloader pin pair (sda/scl) of {}: {pairs:?}", board.product),
                );
            }
        }
    }

    fn v14(&mut self) {
        let t = self.t;
        let order = &t.update.order;
        for n in &t.nodes {
            let count = order.iter().filter(|id| **id == n.id).count();
            if count != 1 {
                self.err(
                    "V-14",
                    "update.order",
                    format!("'{}' appears {count} times; every node exactly once", n.id),
                );
            }
        }
        for (i, id) in order.iter().enumerate() {
            if t.node(id).is_none() {
                self.err(
                    "V-14",
                    format!("update.order[{i}]"),
                    format!("unknown node '{id}'"),
                );
            }
        }
        if let Some(c) = self.coordinator() {
            if order.last() != Some(&c.id) {
                self.err(
                    "V-14",
                    "update.order",
                    format!("coordinator '{}' must come last", c.id),
                );
            }
        }
    }

    fn v15(&mut self) {
        for (i, c) in self.t.constraints.iter().enumerate() {
            for id in c.node_refs() {
                if self.t.node(id).is_none() {
                    self.err(
                        "V-15",
                        format!("constraints[{i}]"),
                        format!("unknown node '{id}'"),
                    );
                }
            }
        }
    }

    fn v16(&mut self) {
        for (i, n) in self.t.nodes.iter().enumerate() {
            let Some(name) = &n.flash_map else { continue };
            let path = format!("nodes[{i}].flash_map");
            match self.reg.flash_map(name) {
                None => self.err("V-16", path, format!("unknown flash map '{name}'")),
                Some((mcu, _)) if mcu.id != n.mcu => self.err(
                    "V-16",
                    path,
                    format!("flash map '{name}' is for {}", mcu.id),
                ),
                Some((_, m)) => {
                    // P1: a node chosen by MCU has its flash size in dagon.toml, checked there.
                    if let Some(b) = named_board(self.reg, n).filter(|b| m.bytes > b.flash_bytes) {
                        self.err(
                            "V-16",
                            path,
                            format!(
                                "'{name}' needs {} bytes, {} has {}",
                                m.bytes, b.product, b.flash_bytes
                            ),
                        );
                    }
                }
            }
        }
    }

    fn v17(&mut self) {
        let Some(board) = self.coordinator_board() else {
            return;
        };
        for (i, b) in self.t.buses.iter().enumerate() {
            let (sda, scl) = (format!("GPIO{}", b.sda), format!("GPIO{}", b.scl));
            let ok = board
                .i2c_pairs
                .iter()
                .any(|p| p.controller == b.controller && p.sda == sda && p.scl == scl);
            if !ok {
                let pairs: Vec<_> = board
                    .i2c_pairs
                    .iter()
                    .map(|p| format!("{} {}/{}", p.controller, p.sda, p.scl))
                    .collect();
                self.err(
                    "V-17",
                    format!("buses[{i}]"),
                    format!(
                        "{} {sda}/{scl} is not a usable I2C pair on {}: {pairs:?}",
                        b.controller, board.product
                    ),
                );
            }
        }
    }

    /// An `addr` is on an end of a kind with addresses and is a 7-bit address;
    /// no two ends on one bus share one, and none takes the address a ROM
    /// loader reached over that bus answers on.
    fn v18(&mut self) {
        let t = self.t;
        for (i, l, k, e) in self.ends() {
            let Some(addr) = e.addr else { continue };
            let path = format!("{}.addr", end_path(i, k));
            if !l.kind.addressed() {
                self.err(
                    "V-18",
                    path,
                    format!(
                        "a {} link has no addresses; 'addr' is for i2c",
                        l.kind.name()
                    ),
                );
            } else if !(I2C_ADDR_MIN..=I2C_ADDR_MAX).contains(&addr) {
                self.err(
                    "V-18",
                    path,
                    format!("address {addr:#04x} is outside 0x08-0x77"),
                );
            }
        }
        for bus in &t.buses {
            let on_bus: Vec<(usize, &Link)> = self
                .links()
                .filter(|(_, l)| l.kind == LinkKind::I2c && l.bus.as_ref() == Some(&bus.id))
                .collect();
            // Addresses the ROM loaders of nodes reached through them answer on.
            let reserved: BTreeMap<u8, &str> = on_bus
                .iter()
                .flat_map(|(_, l)| &l.ends)
                .filter_map(|e| self.forced_rom(&e.node).map(|r| (r.addr, r.name.as_str())))
                .collect();
            let mut seen = BTreeSet::new();
            let addressed = on_bus.iter().flat_map(|&(i, l)| {
                l.ends
                    .iter()
                    .enumerate()
                    .filter_map(move |(k, e)| e.addr.map(|a| (end_path(i, k), a)))
            });
            for (ep, addr) in addressed.collect::<Vec<_>>() {
                let path = format!("{ep}.addr");
                if !seen.insert(addr) {
                    self.err(
                        "V-18",
                        path.clone(),
                        format!("address {addr:#04x} is already used on bus '{}'", bus.id),
                    );
                }
                if let Some(rom) = reserved.get(&addr) {
                    self.err(
                        "V-18",
                        path,
                        format!("{addr:#04x} is reserved for the {rom} on this bus"),
                    );
                }
            }
        }
    }

    /// A bus's speed is one every node on it runs its I2C link at, from
    /// their MCUs' `[link_i2c]`; a node whose MCU states none is refused (no
    /// speed is known to work on it). The slowest ROM loader reached over
    /// the bus caps it.
    fn v20(&mut self) {
        for (i, b) in self.t.buses.iter().enumerate() {
            let on_bus: Vec<&Link> = self
                .links()
                .filter(|(_, l)| l.kind == LinkKind::I2c && l.bus.as_ref() == Some(&b.id))
                .map(|(_, l)| l)
                .collect();
            let rom_cap = on_bus
                .iter()
                .flat_map(|l| &l.ends)
                .filter_map(|e| self.forced_rom(&e.node))
                .min_by_key(|r| r.max_hz);
            let speed = b.i2c_hz();
            let path = format!("buses[{i}].speed_hz");
            // The speeds every node's firmware runs the bus at; a node whose firmware runs no I2C link here is V-23's.
            let mut named: Option<Vec<u32>> = None;
            for l in &on_bus {
                for id in l.ends.iter().map(|e| &e.node) {
                    let Some(m) = self.t.node(id).and_then(|n| self.reg.mcu(&n.mcu)) else {
                        continue;
                    };
                    if !links::runs_in(self.reg, &m.id, LinkKind::I2c) {
                        continue;
                    }
                    let Some(u) = &m.link_i2c else {
                        self.err(
                            "V-20",
                            path.clone(),
                            format!(
                                "speed_hz {speed}: {id} ({}) cannot be checked: its MCU definition has no [link_i2c]",
                                m.name
                            ),
                        );
                        continue;
                    };
                    named = Some(match named {
                        None => u.hzs.clone(),
                        Some(n) => n.into_iter().filter(|h| u.hzs.contains(h)).collect(),
                    });
                }
            }
            // On a speed every node runs, each node that drives the bus
            // makes SCL from its clock: the end the other reaches at an
            // address is the target, the other end the controller.
            if named.as_ref().is_none_or(|n| n.contains(&speed)) {
                let mut seen: Vec<&str> = Vec::new();
                for l in &on_bus {
                    if !l.ends.iter().any(|e| e.addr.is_some()) {
                        continue;
                    }
                    for e in l.ends.iter().filter(|e| e.addr.is_none()) {
                        if seen.contains(&e.node.as_str()) {
                            continue;
                        }
                        seen.push(&e.node);
                        if let Some(what) = self.scl_fault(&e.node, speed) {
                            self.err("V-20", path.clone(), format!("speed_hz {speed}: {what}"));
                        }
                    }
                }
            }
            let over_cap = rom_cap.is_some_and(|r| speed > r.max_hz);
            if named.as_ref().is_some_and(|n| !n.contains(&speed)) || over_cap {
                let limit = rom_cap.map_or(String::new(), |r| {
                    format!(" ({} limit {})", r.name, r.max_hz)
                });
                self.err(
                    "V-20",
                    path,
                    format!(
                        "speed_hz must be one of {:?}{limit}",
                        named.unwrap_or_default()
                    ),
                );
            }
            if !matches!(b.pullups.as_deref(), Some("external" | "board")) {
                self.err(
                    "V-20",
                    format!("buses[{i}].pullups"),
                    "pullups must be 'external' or 'board'",
                );
            }
        }
    }

    /// Why the controller `id` cannot drive its bus at `speed` (from its
    /// MCU's `[link_i2c.controller]` on the clock its node's app runs on, as
    /// V-26 takes a UART's), or `None` when it can. A node whose firmware
    /// runs no I2C link is V-23's.
    fn scl_fault(&self, id: &str, speed: u32) -> Option<String> {
        let n = self.t.node(id)?;
        let m = self.reg.mcu(&n.mcu)?;
        if !links::runs_in(self.reg, &m.id, LinkKind::I2c) {
            return None;
        }
        let c = m.link_i2c.as_ref()?.controller.as_ref();
        let board = n.board.as_deref().and_then(|b| self.reg.board(b));
        let what = match m.i2c_controller_clock(board) {
            Err(why) => why,
            Ok(clock) => {
                let c = c?;
                match c
                    .actual_hz(clock.hz, speed)
                    .zip(c.hz_error(clock.hz, speed))
                {
                    Some((_, e)) if c.limit().admits(e, clock.tolerance) => return None,
                    Some((a, e)) => format!(
                        "runs SCL at {a:.0} Hz, {:+.2} %{}; the bus controller must run it {}",
                        e * 100.0,
                        with_tolerance(e, clock.tolerance),
                        c.limit().describe()
                    ),
                    None => "cannot make it as the bus controller".into(),
                }
            }
        };
        Some(format!("{id} ({}) {what}", m.name))
    }

    /// A UART link's speed on each end whose firmware runs it, from its
    /// MCU's `[link_uart]` on the clock its node's UART runs on (for
    /// `clock = "app"`, the `[[app.clock]]` setup the board the node names
    /// picks, its tolerance added to the error); such an end whose MCU
    /// states no `[link_uart]`, or whose clock no tolerance bounds, is
    /// refused (no speed is known to work on it).
    fn v26(&mut self) {
        for (i, l) in self.links() {
            if l.kind != LinkKind::Uart {
                continue;
            }
            let baud = l.uart_baud();
            for id in l.ends.iter().map(|e| &e.node) {
                let Some(n) = self.t.node(id) else {
                    continue;
                };
                let Some(m) = self.reg.mcu(&n.mcu) else {
                    continue;
                };
                // An end whose firmware runs no UART link runs no speed:
                // V-24 reports the link instead.
                if !links::runs_in(self.reg, &m.id, LinkKind::Uart) {
                    continue;
                }
                // The board the node names, as its build takes it: none
                // runs the app on the internal oscillator.
                let board = n.board.as_deref().and_then(|b| self.reg.board(b));
                let clock = m.uart_clock(board);
                let u = m.link_uart.as_ref().zip(clock.as_ref().ok());
                let at = |u: &crate::defs::LinkUart, c: &crate::defs::LinkClockOn| {
                    (
                        u.runs(baud),
                        u.actual_baud(c.hz, baud),
                        u.baud_error(c.hz, baud),
                    )
                };
                let tolerance = clock.as_ref().map_or(0.0, |c| c.tolerance);
                let limit = m.link_uart.as_ref().map(crate::defs::LinkUart::limit);
                let what = match u.map(|(u, c)| at(u, c)) {
                    None => clock.err().unwrap_or_default(),
                    Some((_, Some(a), Some(e)))
                        if !limit.is_some_and(|r| r.admits(e, tolerance)) =>
                    {
                        format!(
                            "runs at {a:.0} bit/s, {:+.2} %{}; each end of a UART link must be {}",
                            e * 100.0,
                            with_tolerance(e, tolerance),
                            limit.map(|r| r.describe()).unwrap_or_default()
                        )
                    }
                    Some((false, Some(_), Some(_))) => format!(
                        "is not a speed its firmware runs the link at; its definition names {:?}",
                        u.map(|(u, _)| u.bauds.as_slice()).unwrap_or(&[])
                    ),
                    Some((true, Some(_), Some(_))) => continue,
                    Some(_) => "cannot make it".into(),
                };
                self.err(
                    "V-26",
                    format!("{}.baud", link_path(i)),
                    format!("baud {baud}: {id} ({}) {what}", m.name),
                );
            }
        }
    }

    fn v21(&mut self) {
        for (i, l) in self.links() {
            if l.kind != LinkKind::I2c {
                continue;
            }
            // An end the other reaches at an address is an I2C target.
            let targets = l.ends.iter().filter(|e| e.addr.is_some());
            let boards: Vec<&BoardDef> = targets
                .filter_map(|e| self.node_board(&e.node).filter(|b| !b.i2c_target))
                .collect();
            for b in boards {
                let mcu_name = self.reg.mcu(&b.mcu).map_or(b.mcu.as_str(), |m| &m.name);
                self.err(
                    "V-21",
                    format!("{}.kind", link_path(i)),
                    format!(
                        "{} · {mcu_name} cannot be an I2C target; use uart",
                        b.product
                    ),
                );
            }
        }
    }

    fn v22(&mut self) {
        for (i, l) in self.links() {
            if l.control.kind == ControlKind::Full && l.resetting_end().is_none() {
                let [a, b] = l.ends.each_ref().map(|e| e.node.as_str());
                self.issues.push(Issue {
                    rule: "V-22",
                    severity: Severity::Warning,
                    path: format!("{}.control", link_path(i)),
                    message: format!(
                        "no end of the link between {} sets both nrst and boot; recovery over it runs without rom-forced",
                        pair(a, b)
                    ),
                });
            }
        }
    }

    /// Each end's port is a `[[link_port]]` of its node's MCU definition, of the
    /// link's kind. Unknown nodes and MCUs are V-04's and V-06's.
    fn v27(&mut self) {
        for (i, l, k, e) in self.ends() {
            let Some(m) = self.t.node(&e.node).and_then(|n| self.reg.mcu(&n.mcu)) else {
                continue;
            };
            let path = format!("{}.port", end_path(i, k));
            match m.port(&e.port) {
                None => {
                    let known: Vec<&str> = m.ports.iter().map(|p| p.name.as_str()).collect();
                    self.err(
                        "V-27",
                        path,
                        format!("{} has no port '{}'; its ports: {known:?}", m.id, e.port),
                    );
                }
                Some(p) if p.kind != l.kind => self.err(
                    "V-27",
                    path,
                    format!(
                        "port '{}' of {} is {}, not {}",
                        e.port,
                        m.id,
                        p.kind.name(),
                        l.kind.name()
                    ),
                ),
                Some(_) => {}
            }
        }
    }
}

/// The GPIOs an end names on its own node, with their paths: `pins` (when
/// `with_pins`), `nrst`, `boot`.
fn own_gpios(ep: &str, e: &End, with_pins: bool) -> Vec<(String, u8)> {
    let mut out = Vec::new();
    if let (Some(u), true) = (e.pins, with_pins) {
        out.push((format!("{ep}.pins.tx"), u.tx));
        out.push((format!("{ep}.pins.rx"), u.rx));
    }
    out.extend(e.nrst.map(|g| (format!("{ep}.nrst"), g)));
    out.extend(e.boot.map(|g| (format!("{ep}.boot"), g)));
    out
}
