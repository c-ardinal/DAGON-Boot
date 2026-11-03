//! Validation rules for `dagon topo check`.
//!
//! 20 errors (V-01..V-21 without V-19) and one warning (V-22). Errors block
//! `topo push`; warnings do not.

use std::collections::{BTreeMap, BTreeSet};

use crate::boards::{self, Board};
use crate::report::{Issue, Report, Severity};
use crate::schema::{ControlKind, Link, LinkKind, Node, Role, Tier, Topology, UartIface};

pub const SCHEMA_VERSION: u32 = 1;
/// Nodes a 構成 may describe (and the editor draw).
const MAX_NODES: usize = 32;
/// Nodes updating reaches today; more is a shape not updatable yet (V-23).
const MAX_UPDATABLE_NODES: usize = 8;
const MAX_ID_LEN: usize = 15;
const MAX_TAG_LEN: usize = 16;
const BAUD_RATES: &[u32] = &[9_600, 57_600, 115_200, 460_800];
pub(crate) const I2C_SPEEDS: &[u32] = &[100_000, 400_000];
const I2C_ADDR_MIN: u8 = 0x08;
const I2C_ADDR_MAX: u8 = 0x77;
/// Address the STM32 ROM I2C bootloader answers on.
pub(crate) const STM32_ROM_ADDR: u8 = 0x39;
// ponytail: the coordinator is always an RP2040 today; move these into the
// port registry when a second coordinator MCU appears.
pub(crate) const RP2040_PIO_STATE_MACHINES: usize = 8;
pub(crate) const PIO_UART_STATE_MACHINES: usize = 2;
pub(crate) const RP2040_UART0_TX: &[u8] = &[0, 12, 16, 28];
pub(crate) const RP2040_UART0_RX: &[u8] = &[1, 13, 17, 29];

/// Parse YAML and run every rule. A document that does not parse yields a
/// single V-01 error.
pub fn check_str(yaml: &str) -> Report {
    match yaml_serde::from_str::<Topology>(yaml) {
        Ok(t) => check(&t),
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

pub fn check(t: &Topology) -> Report {
    let mut c = Ctx {
        t,
        issues: Vec::new(),
    };
    c.v01();
    c.v02();
    c.v03();
    c.v04();
    c.v05();
    c.v06();
    c.v07_v08();
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
    Report { issues: c.issues }
}

/// True when a full-control link lacks NRST or BOOT wiring (V-22): the node
/// is then treated as having no `rom-forced` tier.
pub fn rom_forced_unavailable(link: &Link) -> bool {
    link.control.kind == ControlKind::Full
        && (link.control.nrst.is_none() || link.control.boot.is_none())
}

/// Recovery tiers for `id` as they will be used, i.e. without `rom-forced`
/// when V-22 applies.
pub fn effective_tiers(t: &Topology, id: &str) -> Vec<Tier> {
    let dropped = t.link_to(id).is_some_and(rom_forced_unavailable);
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
pub fn named_board(n: &Node) -> Option<&'static Board> {
    n.board.as_deref().and_then(boards::board)
}

/// Board for a node's capabilities (I2C target, ROM loader pins): the named
/// one, else the first table entry for its MCU.
pub fn node_board(n: &Node) -> Option<&'static Board> {
    match &n.board {
        Some(id) => boards::board(id),
        None => boards::default_board(&n.mcu),
    }
}

struct Ctx<'a> {
    t: &'a Topology,
    issues: Vec<Issue>,
}

fn link_path(i: usize) -> String {
    format!("links[{i}]")
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

    fn coordinator(&self) -> Option<&'a Node> {
        self.t.nodes.iter().find(|n| n.role == Role::Coordinator)
    }

    fn coordinator_board(&self) -> Option<&'static Board> {
        self.coordinator().and_then(named_board)
    }

    fn links(&self) -> impl Iterator<Item = (usize, &'a Link)> + 'a {
        self.t.links.iter().enumerate()
    }

    /// Links from the 親機 to its 子機 over I2C or UART: the ones the hub's
    /// own pins, controllers and PIO serve. Other links (between 親機,
    /// deeper, CAN, Ethernet) are reported by V-23 instead.
    fn hub_links(&self) -> impl Iterator<Item = (usize, &'a Link)> + 'a {
        let hub = self.coordinator().map(|c| c.id.clone());
        self.t.links.iter().enumerate().filter(move |(_, l)| {
            Some(&l.from) == hub.as_ref() && matches!(l.kind, LinkKind::I2c | LinkKind::Uart)
        })
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
            match l.kind {
                LinkKind::I2c => {
                    match &l.bus {
                        None => self.err("V-01", format!("{p}.bus"), "i2c link needs 'bus'"),
                        Some(b) if !bus_ids.contains(b.as_str()) => {
                            self.err("V-01", format!("{p}.bus"), format!("unknown bus '{b}'"))
                        }
                        Some(_) => {}
                    }
                    if l.addr.is_none() {
                        self.err("V-01", format!("{p}.addr"), "i2c link needs 'addr'");
                    }
                    if l.node_pins.is_none() {
                        self.err(
                            "V-01",
                            format!("{p}.node_pins"),
                            "i2c link needs 'node_pins' (sda, scl)",
                        );
                    }
                }
                LinkKind::Uart => {
                    if l.iface.is_none() {
                        self.err("V-01", format!("{p}.iface"), "uart link needs 'iface'");
                    }
                    if l.pins.is_none() {
                        self.err(
                            "V-01",
                            format!("{p}.pins"),
                            "uart link needs 'pins' (tx, rx)",
                        );
                    }
                    if l.baud.is_some_and(|b| !BAUD_RATES.contains(&b)) {
                        self.err(
                            "V-01",
                            format!("{p}.baud"),
                            format!("baud must be one of {BAUD_RATES:?}"),
                        );
                    }
                }
                // Described only; V-23 says they are not updatable yet.
                LinkKind::Can | LinkKind::Ethernet => {}
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
        // Several coordinators are a shape that is not updatable yet (V-23).
        if count == 0 {
            self.err("V-03", "nodes", "need a coordinator, got none");
        }
    }

    fn v04(&mut self) {
        for (i, l) in self.links() {
            let p = link_path(i);
            for (end, id) in [("from", &l.from), ("to", &l.to)] {
                if self.t.node(id).is_none() {
                    self.err("V-04", format!("{p}.{end}"), format!("unknown node '{id}'"));
                }
            }
        }
    }

    /// Shapes a 構成 may describe but that cannot be updated yet: more than
    /// one coordinator, links that do not run from the coordinator to a node
    /// (between coordinators, to grandchildren and deeper), and CAN or
    /// Ethernet links. Updating covers one 親機 with its direct 子機 today.
    fn v23(&mut self) {
        let hubs: Vec<&str> = self
            .t
            .nodes
            .iter()
            .filter(|n| n.role == Role::Coordinator)
            .map(|n| n.id.as_str())
            .collect();
        let n = self.t.nodes.len();
        if n > MAX_UPDATABLE_NODES {
            self.err(
                "V-23",
                "nodes",
                format!("{n} nodes; updating covers up to {MAX_UPDATABLE_NODES}"),
            );
        }
        if hubs.len() > 1 {
            self.err(
                "V-23",
                "nodes",
                format!("{} coordinators {hubs:?}; updating covers one", hubs.len()),
            );
        }
        for (i, l) in self.links() {
            let p = link_path(i);
            if matches!(l.kind, LinkKind::Can | LinkKind::Ethernet) {
                self.err(
                    "V-23",
                    format!("{p}.kind"),
                    "CAN and Ethernet links are not updatable yet",
                );
            }
            match self.t.node(&l.from) {
                Some(n) if n.role != Role::Coordinator => self.err(
                    "V-23",
                    format!("{p}.from"),
                    format!(
                        "'{}' is not a coordinator; updating reaches direct nodes only",
                        l.from
                    ),
                ),
                _ => {}
            }
            if let Some(n) = self.t.node(&l.to).filter(|n| n.role == Role::Coordinator) {
                self.err(
                    "V-23",
                    format!("{p}.to"),
                    format!(
                        "'{}' is a coordinator; links between coordinators are not updatable yet",
                        n.id
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
            let count = self.t.links.iter().filter(|l| l.to == n.id).count();
            if count != 1 {
                self.err(
                    "V-05",
                    format!("nodes[{i}]"),
                    format!("node '{}' needs exactly one link, has {count}", n.id),
                );
            }
        }
    }

    fn v06(&mut self) {
        for (i, n) in self.t.nodes.iter().enumerate() {
            match boards::mcu(&n.mcu) {
                None => {
                    let known: Vec<_> = boards::MCUS.iter().map(|m| m.id).collect();
                    self.err(
                        "V-06",
                        format!("nodes[{i}].mcu"),
                        format!("mcu '{}' is not in the port registry {known:?}", n.mcu),
                    );
                }
                Some(m) if m.port != n.port => {
                    self.err(
                        "V-06",
                        format!("nodes[{i}].port"),
                        format!("{} uses port '{}', not '{}'", m.id, m.port, n.port),
                    );
                }
                Some(_) => {}
            }
            if let Some(b) = &n.board {
                match boards::board(b) {
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
        for (i, l) in self.hub_links() {
            let p = link_path(i);
            if let Some(u) = l.pins {
                pins.push((format!("{p}.pins.tx"), u.tx));
                pins.push((format!("{p}.pins.rx"), u.rx));
            }
            if let Some(g) = l.control.nrst {
                pins.push((format!("{p}.control.nrst"), g));
            }
            if let Some(g) = l.control.boot {
                pins.push((format!("{p}.control.boot"), g));
            }
        }
        pins
    }

    fn v07_v08(&mut self) {
        let pins = self.coordinator_pins();
        if let Some(board) = self.coordinator_board() {
            for (path, gpio) in &pins {
                let name = format!("GPIO{gpio}");
                if !board.header.iter().any(|h| h.pin == name) {
                    let avail: Vec<_> = board.header.iter().map(|h| h.pin).collect();
                    self.err(
                        "V-07",
                        path.clone(),
                        format!("{name} is not on the {} header {avail:?}", board.product),
                    );
                }
            }
        } else if let Some(mcu) = self
            .coordinator()
            .and_then(|c| boards::mcu(&c.mcu))
            .filter(|m| m.gpio_count > 0)
        {
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
        for (i, l) in self.hub_links() {
            let (Some(UartIface::HwUart0), Some(u)) = (l.iface, l.pins) else {
                continue;
            };
            let p = link_path(i);
            if !RP2040_UART0_TX.contains(&u.tx) || !RP2040_UART0_RX.contains(&u.rx) {
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
            .filter(|(_, l)| l.iface == Some(UartIface::PioUart))
            .count();
        if pio_links * PIO_UART_STATE_MACHINES > RP2040_PIO_STATE_MACHINES {
            self.err(
                "V-10",
                "links",
                format!(
                    "{pio_links} pio-uart links need {} PIO state machines, {RP2040_PIO_STATE_MACHINES} exist",
                    pio_links * PIO_UART_STATE_MACHINES
                ),
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
            let control = t.link_to(id).map(|l| l.control.kind);
            for tier in effective_tiers(t, id) {
                match tier {
                    Tier::Local | Tier::Redistribute => {}
                    Tier::RomForced if control != Some(ControlKind::Full) => {
                        self.err(
                            "V-12",
                            path.clone(),
                            "rom-forced needs a link with full control",
                        );
                    }
                    Tier::RomCoop
                        if control != Some(ControlKind::Cooperative)
                            || node.port != "vendor-stage2" =>
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

    fn v13(&mut self) {
        for (i, l) in self.links() {
            if !effective_tiers(self.t, &l.to).contains(&Tier::RomForced) {
                continue;
            }
            let Some(board) = self.t.node(&l.to).and_then(node_board) else {
                continue;
            };
            let on_rom_pins = l.kind == LinkKind::I2c
                && l.node_pins.as_ref().is_some_and(|np| {
                    board
                        .rom_i2c_pairs
                        .iter()
                        .any(|r| r.sda == np.sda && r.scl == np.scl)
                });
            if !on_rom_pins {
                let pairs: Vec<_> = board
                    .rom_i2c_pairs
                    .iter()
                    .map(|r| format!("{}/{}", r.sda, r.scl))
                    .collect();
                self.err(
                    "V-13",
                    format!("{}.node_pins", link_path(i)),
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
            match boards::flash_map(name) {
                None => self.err("V-16", path, format!("unknown flash map '{name}'")),
                Some(m) if m.mcu != n.mcu => {
                    self.err("V-16", path, format!("flash map '{name}' is for {}", m.mcu))
                }
                Some(m) => {
                    // P1: a node chosen by MCU has its flash size in dagon.toml, checked there.
                    if let Some(b) = named_board(n).filter(|b| m.bytes > b.flash_bytes) {
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

    fn v18(&mut self) {
        let t = self.t;
        for bus in &t.buses {
            let on_bus: Vec<(usize, &Link)> = self
                .links()
                .filter(|(_, l)| l.kind == LinkKind::I2c && l.bus.as_ref() == Some(&bus.id))
                .collect();
            let stm32_rom = on_bus.iter().any(|(_, l)| {
                t.node(&l.to).is_some_and(|n| n.mcu.starts_with("stm32"))
                    && effective_tiers(t, &l.to).contains(&Tier::RomForced)
            });
            let mut seen = BTreeSet::new();
            for (i, l) in on_bus {
                let Some(addr) = l.addr else { continue };
                let path = format!("{}.addr", link_path(i));
                if !(I2C_ADDR_MIN..=I2C_ADDR_MAX).contains(&addr) {
                    self.err(
                        "V-18",
                        path.clone(),
                        format!("address {addr:#04x} is outside 0x08-0x77"),
                    );
                }
                if !seen.insert(addr) {
                    self.err(
                        "V-18",
                        path.clone(),
                        format!("address {addr:#04x} is already used on bus '{}'", bus.id),
                    );
                }
                if stm32_rom && addr == STM32_ROM_ADDR {
                    self.err("V-18", path, format!("{STM32_ROM_ADDR:#04x} is reserved for the STM32 ROM bootloader on this bus"));
                }
            }
        }
    }

    fn v20(&mut self) {
        for (i, b) in self.t.buses.iter().enumerate() {
            if b.speed_hz.is_some_and(|s| !I2C_SPEEDS.contains(&s)) {
                self.err(
                    "V-20",
                    format!("buses[{i}].speed_hz"),
                    format!("speed_hz must be one of {I2C_SPEEDS:?} (STM32 ROM limit 400000)"),
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

    fn v21(&mut self) {
        for (i, l) in self.links() {
            if l.kind != LinkKind::I2c {
                continue;
            }
            if let Some(b) = self
                .t
                .node(&l.to)
                .and_then(node_board)
                .filter(|b| !b.i2c_target)
            {
                self.err(
                    "V-21",
                    format!("{}.kind", link_path(i)),
                    format!(
                        "{} · {} cannot be an I2C target; use uart",
                        b.product, b.mcu_name
                    ),
                );
            }
        }
    }

    fn v22(&mut self) {
        for (i, l) in self.links() {
            if rom_forced_unavailable(l) {
                self.issues.push(Issue {
                    rule: "V-22",
                    severity: Severity::Warning,
                    path: format!("{}.control", link_path(i)),
                    message: format!(
                        "reset or boot pin for '{}' is not set; recovery for it runs without rom-forced",
                        l.to
                    ),
                });
            }
        }
    }
}
