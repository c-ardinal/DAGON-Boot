//! The simulated wiring a topology describes, each node modelled on the
//! MCU definition the topology names for it in the definitions given (a
//! project's own, its overrides included).
//!
//! What the simulator models bounds it: the node the PC is connected to
//! relays one hop, over I2C and UART links, to nodes whose definitions give
//! a kind S flash layout (`[layout]` with `stage0` and `app_log`) or are
//! the one the fixed vendor-stage2 layout models
//! ([`Layout::ESP32S3_WATCH`]). A topology outside that is refused by name,
//! never simulated as something else.

use dagon_agent::Config;
use dagon_format::header::target_tag;
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};
use dagon_topology::defs::{McuDef, Registry};
use dagon_topology::schema::{LinkKind, Node, Topology};

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
        mcu_id: def.wire_id,
        port_kind: def.port.wire(),
        target_tag: target_tag(&n.tag),
        role: if hub { role::HUB } else { role::NODE },
        mode: mode::APP,
        max_payload: payload as u16,
        trial_attempts: layout.trial_attempts(),
    };
    Ok(SimNode::new(cfg, pubkey, flash, layout))
}

/// The wiring of `t` with every flash empty: the node the PC is connected
/// to (the first coordinator) and each node a link joins to it. NRST and
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
    let root_def = reg
        .mcu(&root_node.mcu)
        .ok_or_else(|| format!("node {root}: no MCU definition {}", root_node.mcu))?;
    let hub = node_of(reg, root_node, true, max_payload::USB, pubkey, seed)?;
    let mut relay = SimRelay::with_definition(hub, root_def);
    let mut wired = None;
    let mut placed = vec![root.to_string()];
    for (i, l) in t.links.iter().enumerate() {
        let [a, b] = &l.ends;
        let (near, far) = match (a.node == root, b.node == root) {
            (true, false) => (a, b),
            (false, true) => (b, a),
            _ => {
                return Err(format!(
                    "the simulator relays one hop from {root}: the link of {} and {} does not start there",
                    a.node, b.node
                ))
            }
        };
        if root_def.port(&near.port).is_none() {
            return Err(format!(
                "MCU definition {} has no port {}",
                root_def.id, near.port
            ));
        }
        let n = t
            .node(&far.node)
            .ok_or_else(|| format!("the topology has no node {}", far.node))?;
        let seed = seed ^ (i as u64 + 1);
        match l.kind {
            LinkKind::I2c => {
                let mut node = node_of(reg, n, false, max_payload::I2C, pubkey, seed)?;
                node.i2c_addr = Some(
                    far.addr
                        .ok_or_else(|| format!("node {}: its I2C end has no address", n.id))?,
                );
                let wire = wired.is_none() && near.drives_reset() && node.rom_addr.is_some();
                if wire {
                    wired = Some(n.id.clone());
                }
                relay.add_i2c(&near.port, node, wire);
            }
            LinkKind::Uart => {
                let node = node_of(reg, n, false, max_payload::UART, pubkey, seed)?;
                relay.set_uart(&near.port, SimLink::new(seed), node);
            }
            k => {
                return Err(format!(
                    "the simulator carries i2c and uart links: node {} is on a {} link",
                    n.id,
                    k.name()
                ))
            }
        }
        placed.push(n.id.clone());
    }
    if let Some(n) = t.nodes.iter().find(|n| !placed.contains(&n.id)) {
        return Err(format!("no link joins node {} to {root}", n.id));
    }
    Ok((relay, wired))
}
