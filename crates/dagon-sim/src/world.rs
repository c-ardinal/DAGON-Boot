//! The simulated wiring a topology describes, each node modelled on the
//! MCU definition the topology names for it in the definitions given (a
//! project's own, its overrides included).
//!
//! What the simulator models bounds it: the node the PC is connected to
//! relays over the links updating runs over (I2C and UART; a node that
//! relays as a node is on a UART link), to nodes whose definitions give
//! a kind S flash layout (`[layout]` with `stage0` and `app_log`) or are
//! the one the fixed vendor-stage2 layout models
//! ([`Layout::ESP32S3_WATCH`]). A topology outside that is refused by name,
//! never simulated as something else.

use dagon_agent::Config;
use dagon_format::header::target_tag;
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};
use dagon_topology::defs::{McuDef, Registry};
use dagon_topology::schema::{Link, LinkKind, Node, Topology};

use crate::{Layout, SimFlash, SimLink, SimNode, SimRelay};

/// The flash and layout the simulator models a node of `def` with.
fn modelled(def: &McuDef, seed: u64) -> Result<(SimFlash, Layout), String> {
    if def.watchdog.is_none() {
        return Err(format!(
            "MCU definition {} has no [watchdog] trial_attempts",
            def.id
        ));
    }
    if let Some(layout) = Layout::of_definition(def) {
        // One byte at a time: the finest any chip programs, so every write
        // of DAGON's (whole `write_unit`s) and of a ROM loader is in units.
        let flash = SimFlash::of_definition(def, 1, seed)
            .ok_or_else(|| format!("MCU definition {} has no [execution]", def.id))?;
        return Ok((flash, layout));
    }
    let fixed = Layout::ESP32S3_WATCH;
    if def.id == fixed.mcu {
        let def: &'static McuDef = Box::leak(Box::new(def.clone()));
        let layout = Layout {
            def: Some(def),
            ..fixed
        };
        return Ok((SimFlash::esp32s3_4m(seed), layout));
    }
    Err(format!(
        "the simulator models no flash layout for MCU {} (its definition has no [layout] stage0 and app_log)",
        def.id
    ))
}

/// Node `n` with an empty flash, answering frames of up to `payload` bytes.
fn node_of(
    reg: &Registry,
    n: &Node,
    hub: bool,
    payload: usize,
    pubkey: [u8; 32],
    seed: u64,
) -> Result<SimNode, String> {
    let def = reg
        .mcu(&n.mcu)
        .ok_or_else(|| format!("node {}: no MCU definition {}", n.id, n.mcu))?;
    let (flash, layout) = modelled(def, seed).map_err(|e| format!("node {}: {e}", n.id))?;
    let cfg = Config {
        mcu_id: def.mcu_id(),
        port_kind: def.port.wire(),
        target_tag: target_tag(&n.tag),
        role: if hub { role::HUB } else { role::NODE },
        mode: mode::APP,
        max_payload: payload as u16,
        trial_attempts: layout.trial_attempts(),
    };
    Ok(SimNode::new(cfg, pubkey, flash, layout))
}

/// What the building of a world carries from node to node.
struct Wiring<'a> {
    reg: &'a Registry,
    t: &'a Topology,
    pubkey: [u8; 32],
    seed: u64,
    /// The links updating runs over, by their place in `t.links`.
    used: Vec<bool>,
    /// The first node NRST and BOOT0 reach.
    wired: Option<String>,
    placed: Vec<String>,
}

impl Wiring<'_> {
    /// Node `at` as a relaying node, reached as `node`, with every node its
    /// links join to it but the one at the other end of link `up`.
    fn relay(&mut self, at: &Node, node: SimNode, up: Option<usize>) -> Result<SimRelay, String> {
        let t = self.t;
        let def = self
            .reg
            .mcu(&at.mcu)
            .ok_or_else(|| format!("node {}: no MCU definition {}", at.id, at.mcu))?;
        let mut relay = SimRelay::with_definition(node, def);
        let down: Vec<usize> = t
            .links_at(&at.id)
            .map(|(i, _)| i)
            .filter(|i| self.used[*i] && Some(*i) != up)
            .collect();
        for i in down {
            let l = &t.links[i];
            let (Some(near), Some(far)) = (l.end(&at.id), l.other(&at.id)) else {
                continue;
            };
            if def.port(&near.port).is_none() {
                return Err(format!(
                    "MCU definition {} has no port {}",
                    def.id, near.port
                ));
            }
            let n = t
                .node(&far.node)
                .ok_or_else(|| format!("the topology has no node {}", far.node))?;
            self.placed.push(n.id.clone());
            let seed = self.seed ^ (i as u64 + 1);
            let relays = t.links_at(&n.id).any(|(j, _)| self.used[j] && j != i);
            match l.kind {
                LinkKind::I2c if relays => {
                    return Err(format!(
                    "the simulator puts a relaying node on a uart link: node {} is on an i2c link",
                    n.id
                ))
                }
                LinkKind::I2c => {
                    let mut node =
                        node_of(self.reg, n, false, max_payload::I2C, self.pubkey, seed)?;
                    node.i2c_addr = Some(
                        far.addr
                            .ok_or_else(|| format!("node {}: its I2C end has no address", n.id))?,
                    );
                    let wire = up.is_none()
                        && self.wired.is_none()
                        && near.drives_reset()
                        && node.rom_addr.is_some();
                    if wire {
                        self.wired = Some(n.id.clone());
                    }
                    relay.add_i2c(&near.port, node, wire);
                }
                LinkKind::Uart => {
                    let node = node_of(self.reg, n, false, max_payload::UART, self.pubkey, seed)?;
                    if relays {
                        let behind = self.relay(n, node, Some(i))?;
                        relay.set_uart_relay(&near.port, SimLink::new(seed), behind);
                    } else {
                        relay.set_uart(&near.port, SimLink::new(seed), node);
                    }
                }
                k => {
                    return Err(format!(
                        "the simulator carries i2c and uart links: node {} is on a {} link",
                        n.id,
                        k.name()
                    ))
                }
            }
        }
        Ok(relay)
    }
}

/// The wiring of `t` with every flash empty: the node the PC is connected
/// to (the first coordinator), each node a link joins to it, and behind a
/// node whose MCU relays as a node the nodes its links join to it. NRST and
/// BOOT0 reach the first I2C node whose link gives the connected node both
/// lines and whose MCU has an I2C ROM loader; its id is the second value.
/// Provision each node as [`crate::demo_hub`] says.
pub fn build(
    reg: &Registry,
    t: &Topology,
    pubkey: [u8; 32],
    seed: u64,
) -> Result<(SimRelay, Option<String>), String> {
    let root = dagon_topology::reach_root(t).ok_or("the topology has no coordinator")?;
    let root_node = t.node(root).ok_or("the topology has no coordinator")?;
    let hub = node_of(reg, root_node, true, max_payload::USB, pubkey, seed)?;
    let (part, _) = dagon_topology::updatable_with(reg, t);
    let same = |a: &Link, b: &Link| {
        (0..2).all(|k| a.ends[k].node == b.ends[k].node && a.ends[k].port == b.ends[k].port)
    };
    let used: Vec<bool> = t
        .links
        .iter()
        .map(|l| part.links.iter().any(|p| same(p, l)))
        .collect();
    let reached = |id: &String| part.nodes.iter().any(|n| n.id == *id);
    if let Some(l) = t
        .links
        .iter()
        .zip(&used)
        .find_map(|(l, used)| (!used && l.ends.iter().all(|e| reached(&e.node))).then_some(l))
    {
        return Err(format!(
            "the simulator relays over the links updating runs over: the link of {} and {} is not one",
            l.ends[0].node, l.ends[1].node
        ));
    }
    let mut w = Wiring {
        reg,
        t,
        pubkey,
        seed,
        used,
        wired: None,
        placed: vec![root.to_string()],
    };
    let relay = w.relay(root_node, hub, None)?;
    if let Some(n) = t.nodes.iter().find(|n| !w.placed.contains(&n.id)) {
        return Err(format!("no link joins node {} to {root}", n.id));
    }
    Ok((relay, w.wired))
}
