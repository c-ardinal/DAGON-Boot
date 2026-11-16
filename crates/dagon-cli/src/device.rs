//! `status`, `apply`, `rollback`: thin shells over
//! dagon-host on the hub's serial port.

use std::path::{Path, PathBuf};

use dagon_format::header::port_kind;
use dagon_format::Version;
use dagon_host::{
    Event, Host, HostClock, LocalNode, NodeState, Outcome, RouteError, Target, Transport, WritePath,
};
use dagon_proto::payload::{phase, SlotState, StatusResp, VersionResp, VersionSlots};
use dagon_proto::Status;
use dagon_sign::to_hex;
use dagon_sim::block_on;
use dagon_topology::defs::Registry;
use dagon_topology::Topology;

use crate::cli::{ApplyArgs, RollbackArgs, StatusArgs};
use crate::config::{load_key, read_file, read_topology, Config};
use crate::serial;

type Res = Result<(), String>;

fn topology_bin(reg: &Registry, t: &Topology) -> Result<Vec<u8>, String> {
    dagon_topology::compile_with(reg, t).map_err(|e| e.to_string())
}

/// The STATUS entries a node fills, from its MCU definition: the app's
/// (with the agent's factory slot) and the boot core's.
pub(crate) struct Rows {
    pub app: Vec<u8>,
    pub core: Vec<u8>,
    /// The app's image slots: `app` without the agent's factory slot.
    pub image: Vec<u8>,
}

impl Rows {
    /// The rows of MCU `mcu`; `None` when the definitions do not name it
    /// (every entry is shown then).
    pub(crate) fn of(reg: &Registry, mcu: &str) -> Option<Rows> {
        use dagon_topology::defs::TargetName;
        let m = reg.mcu(mcu)?;
        let app = m.status_rows(TargetName::App);
        let agent = m.status_rows(TargetName::Agent);
        Some(Rows {
            image: app.iter().copied().filter(|s| !agent.contains(s)).collect(),
            app,
            core: m.status_rows(TargetName::Bootcore),
        })
    }
}

/// A STATUS slot by name: A, B, then F for the factory slot.
fn slot_name(s: u8) -> String {
    match s {
        dagon_format::statelog::slot::FACTORY => "F".into(),
        s => char::from(b'A' + s).to_string(),
    }
}

/// `A:Confirmed 1.0.0, B:Empty 0.0.0` for the entries in `rows` (every
/// entry without).
fn slots_of(st: &StatusResp, rows: Option<&Vec<u8>>) -> String {
    let parts: Vec<String> = (0u8..)
        .zip(&st.slots)
        .filter(|(s, _)| rows.is_none_or(|r| r.contains(s)))
        .map(|(s, x)| format!("{}:{:?} {}", slot_name(s), x.state, x.version))
        .collect();
    parts.join(", ")
}

fn show_state(name: &str, s: &NodeState, rows: Option<&Rows>) {
    let st = &s.status;
    let slots = slots_of(st, rows.map(|r| &r.app));
    println!(
        "{name}: version {}{}, mcu {:#06x}, wire {}, active {}, confirmed {}, attempts {}, last reason {}, [{}]",
        s.hello.active_version,
        if s.hello.trial { " (on trial)" } else { "" },
        s.hello.mcu_id,
        s.hello.proto_ver,
        st.active,
        st.confirmed,
        st.attempts,
        st.last_reason,
        slots
    );
    if let Some(w) = crate::versions::wire_warning(name, s.hello.proto_ver) {
        println!("{w}");
    }
}

/// The boot core line of `dagon status`: the running core's version and
/// header digest, then each boot core slot; or why there is none.
pub(crate) fn core_line<E: std::fmt::Debug>(
    name: &str,
    r: Result<StatusResp, dagon_host::Error<E>>,
    rows: Option<&Rows>,
) -> String {
    match r {
        Ok(st) => {
            let image_slots = (0..StatusResp::IMAGE_SLOTS as u8).collect();
            let rows = rows.map_or(&image_slots, |r| &r.core);
            let slots = slots_of(&st, Some(rows));
            let running = rows
                .contains(&st.active)
                .then(|| st.slots.get(usize::from(st.active)))
                .flatten();
            match running {
                Some(r) => format!(
                    "{name}: boot core {} (slot {}, digest {}), [{slots}]",
                    r.version,
                    slot_name(st.active),
                    to_hex(&r.digest16[..8]),
                ),
                None => format!("{name}: boot core unknown, [{slots}]"),
            }
        }
        Err(dagon_host::Error::Rejected {
            status: Status::Unsupported | Status::Arg,
            ..
        }) => format!("{name}: boot core not written through releases on this node"),
        Err(e) => format!("{name}: boot core status unreadable ({e:?})"),
    }
}

/// The topology from `--topology` or `dagon.toml`, if any.
fn optional_topology(topology: Option<&str>, cfg: &Config) -> Result<Option<Topology>, String> {
    match topology.map(PathBuf::from).or(cfg.topology.clone()) {
        Some(p) => read_topology(cfg, &p).map(Some),
        None => Ok(None),
    }
}

pub fn status(a: &StatusArgs) -> Res {
    let cfg = Config::load(a.config.as_deref())?;
    let t = optional_topology(a.topology.as_deref(), &cfg)?;
    println!("{}", crate::versions::known_text());
    let mut host = serial::host(a.port.as_deref())?;
    let hub = block_on(host.status(Target::Local)).map_err(|e| format!("hub: {e:?}"))?;
    let reg = crate::defs::registry(&cfg)?;
    let hub_rows = t.as_ref().and_then(|t| hub_rows(&reg, t));
    let hub_rows = hub_rows.as_ref();
    show_state("hub", &hub, hub_rows);
    println!(
        "{}",
        core_line(
            "hub",
            block_on(host.boot_core_status(Target::Local)),
            hub_rows
        )
    );
    let port = Some(hub.hello.port_kind);
    println!(
        "{}",
        version_line("hub", block_on(host.version(Target::Local)), port, hub_rows)
    );
    let Some(t) = t else {
        return Ok(());
    };
    for (id, to, rows) in node_targets(&cfg, &t)? {
        let rows = rows.as_ref();
        match to {
            Ok(to) => match block_on(host.status(to)) {
                Ok(s) => {
                    show_state(&id, &s, rows);
                    let core = block_on(host.boot_core_status(to));
                    println!("{}", core_line(&id, core, rows));
                    let port = Some(s.hello.port_kind);
                    let v = block_on(host.version(to));
                    println!("{}", version_line(&id, v, port, rows));
                }
                Err(e) => println!("{id}: no answer ({e:?})"),
            },
            Err(why) => println!("{id}: not reachable ({why})"),
        }
    }
    Ok(())
}

/// A topology node other than the one the PC is connected to: its id, its
/// target (or why it has none) and the STATUS rows it fills.
type NodeRoute = (String, Result<Target, String>, Option<Rows>);

/// The topology's nodes other than the one the PC is connected to, and how
/// each is reached.
fn node_targets(cfg: &Config, t: &Topology) -> Result<Vec<NodeRoute>, String> {
    let reg = crate::defs::registry(cfg)?;
    let all = targets(&reg, t)?;
    let nodes = t
        .nodes
        .iter()
        .map(|n| {
            let to = match all.iter().find(|(id, _)| *id == n.id) {
                Some((_, to)) => to.clone().map_err(|e| e.to_string()),
                None => Err("updating does not reach it".to_string()),
            };
            (n.id.clone(), to, Rows::of(&reg, &n.mcu))
        })
        .filter(|(_, to, _)| *to != Ok(Target::Local))
        .collect();
    Ok(nodes)
}

/// The node the PC is connected to: the 親機 the reachable part starts from.
fn connected(t: &Topology) -> Result<&str, RouteError> {
    dagon_topology::reach_root(t).ok_or(RouteError::Connected { nodes: Vec::new() })
}

/// The node the PC is connected to and its MCU's relay table
/// ([`Host::local`]).
pub(crate) fn local(reg: &Registry, t: &Topology) -> Result<LocalNode, RouteError> {
    dagon_host::local_node(reg, t, connected(t)?)
}

/// Each node of the part of `t` updating reaches, by id, with its target
/// from the node the PC is connected to.
pub(crate) fn targets(reg: &Registry, t: &Topology) -> Result<dagon_host::NodeTargets, String> {
    let c = connected(t).map_err(|e| e.to_string())?;
    dagon_host::node_targets(reg, t, c).map_err(|e| e.to_string())
}

/// The target of node `node` of `t` from the node the PC is connected to.
pub(crate) fn target_of(reg: &Registry, t: &Topology, node: &str) -> Result<Target, String> {
    let (_, to) = targets(reg, t)?
        .into_iter()
        .find(|(id, _)| id == node)
        .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?;
    to.map_err(|e| format!("'{node}' is not reachable: {e}"))
}

/// The STATUS rows of the node on the PC's port.
fn hub_rows(reg: &Registry, t: &Topology) -> Option<Rows> {
    let id = dagon_topology::reach_root(t)?;
    Rows::of(reg, &t.node(id)?.mcu)
}

/// The VERSION line of `dagon status` and `dagon doctor`: stage0, boot
/// core slots, app slots, wire version and MCU id. A value the node left
/// out is "unknown", or "none" on an MCU booted by its vendor's 2nd stage
/// (HELLO `port_kind`); a node whose agent predates VERSION is "unknown".
pub(crate) fn version_line<E: std::fmt::Debug>(
    name: &str,
    r: Result<Option<VersionResp>, dagon_host::Error<E>>,
    port: Option<u8>,
    rows: Option<&Rows>,
) -> String {
    let v = match r {
        Ok(Some(v)) => v,
        Ok(None) => return format!("{name}: versions unknown (its agent predates VERSION)"),
        Err(e) => return format!("{name}: versions unreadable ({e:?})"),
    };
    let absent = || match port {
        Some(port_kind::VENDOR_STAGE2) => "none (vendor 2nd stage)".to_string(),
        _ => "unknown".to_string(),
    };
    let unknown = || "unknown".to_string();
    let stage0 = v.stage0.map_or_else(absent, |x| x.to_string());
    let core = v
        .core_slots
        .map_or_else(absent, |c| slots_text(&c, rows.map(|r| &r.core)));
    let app = v
        .app_slots
        .map_or_else(unknown, |a| slots_text(&a, rows.map(|r| &r.app)));
    let wire = v.wire.map_or_else(unknown, |w| w.to_string());
    let mcu = v.mcu_id.map_or_else(unknown, |m| format!("{m:#06x}"));
    format!(
        "{name}: versions: stage0 {stage0}; boot core {core}; app {app}; wire {wire}; mcu {mcu}"
    )
}

/// `A:Confirmed 1.0.0 (running), B:Empty`
fn slots_text<const N: usize>(s: &VersionSlots<N>, rows: Option<&Vec<u8>>) -> String {
    let parts: Vec<String> = (0u8..)
        .zip(&s.slots)
        .filter(|(i, _)| rows.is_none_or(|r| r.contains(i)))
        .map(|(i, x)| {
            let n = slot_name(i);
            let running = if i == s.active { " (running)" } else { "" };
            match x.state {
                SlotState::Empty => format!("{n}:Empty{running}"),
                st => format!("{n}:{st:?} {}{running}", x.version),
            }
        })
        .collect();
    parts.join(", ")
}

/// `dagon doctor`'s node section: each node's VERSION line when a hub
/// answers on `port` (or is found); otherwise why none were read. Never a
/// failure of the tool checks.
pub(crate) fn doctor_versions(
    port: Option<&str>,
    cfg: &Config,
    t: Option<&Topology>,
) -> Vec<String> {
    let nodes = match t.map(|t| node_targets(cfg, t)).transpose() {
        Ok(n) => n.unwrap_or_default(),
        Err(e) => return vec![format!("node versions not read: {e}")],
    };
    match serial::host(port) {
        Ok(mut host) => version_lines(&mut host, &nodes),
        Err(e) => vec![format!("node versions not read: {e}")],
    }
}

/// The hub's VERSION line, then each node's.
pub(crate) fn version_lines<T: Transport, C: HostClock>(
    host: &mut Host<T, C>,
    nodes: &[NodeRoute],
) -> Vec<String> {
    let mut one = |name: &str, to: Target, rows: Option<&Rows>| {
        let port = block_on(host.hello(to)).ok().map(|h| h.port_kind);
        version_line(name, block_on(host.version(to)), port, rows)
    };
    let mut out = vec![one("hub", Target::Local, None)];
    for (id, to, rows) in nodes {
        out.push(match to {
            Ok(to) => one(id, *to, rows.as_ref()),
            Err(why) => format!("{id}: not reachable ({why})"),
        });
    }
    out
}

/// Rolls topology node `node` back to the version in its other slot and
/// confirms it there; returns that version.
pub fn rollback_node<T: Transport, C: HostClock>(
    host: &mut Host<T, C>,
    reg: &Registry,
    t: &Topology,
    node: &str,
    on_event: &mut dyn FnMut(Event),
) -> Result<Version, String> {
    let i = t
        .nodes
        .iter()
        .position(|n| n.id == node)
        .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?;
    host.timings = node_timings(reg, t)?;
    let to = target_of(reg, t, node)?;
    let mcu = &t.nodes[i].mcu;
    let rows = Rows::of(reg, mcu).ok_or_else(|| format!("{node}: no MCU definition '{mcu}'"))?;
    block_on(host.rollback(to, i as u8, &rows.image, on_event)).map_err(|e| match e {
        dagon_host::Error::NoPreviousVersion => {
            format!("'{node}' has no previously confirmed version to go back to")
        }
        e => format!("rollback {node}: {e:?}"),
    })
}

pub fn rollback(a: &RollbackArgs) -> Res {
    let node = a.node.as_str();
    let cfg = Config::load(a.config.as_deref())?;
    let t = read_topology(&cfg, &cfg.topology_path(a.topology.as_deref())?)?;
    let mut host = serial::host(a.port.as_deref())?;
    let mut print = |e: Event| println!("{node}: {} ({})", phase_name(e.phase), e.version);
    let v = rollback_node(
        &mut host,
        &crate::defs::registry(&cfg)?,
        &t,
        node,
        &mut print,
    )?;
    println!("{node}: back on {v}, confirmed");
    Ok(())
}

/// Each node's own time limits from its MCU definition, for
/// [`Host::timings`].
pub(crate) fn node_timings(
    reg: &Registry,
    t: &Topology,
) -> Result<Vec<(u8, dagon_host::HostTimings)>, String> {
    dagon_host::node_timings(reg, t).map_err(|e| e.to_string())
}

/// How long a written image's erase, data and verify took, after its
/// verify line.
fn print_times(node: &str, e: &Event) {
    if let Some(t) = e.times {
        println!(
            "{node}: erase {} ms, data {} ms, verify {} ms",
            t.erase_ms, t.data_ms, t.verify_ms
        );
    }
}

fn phase_name(p: u8) -> &'static str {
    match p {
        phase::TRANSFER => "transfer",
        phase::VERIFY => "verify",
        phase::REBOOT => "reboot",
        phase::CONFIRM => "confirmed",
        phase::REJECTED => "REJECTED",
        phase::FAILED => "FAILED",
        phase::ROM => "rom",
        _ => "?",
    }
}

/// Verifying key: `--pubkey <file>`, `pubkey` in dagon.toml, else the
/// public half of the signing key.
pub(crate) fn pubkey(
    pubkey: Option<&str>,
    key: Option<&str>,
    cfg: &Config,
) -> Result<[u8; 32], String> {
    match pubkey.map(PathBuf::from).or(cfg.pubkey.clone()) {
        Some(p) => {
            let k = dagon_sign::load_public_key(&p).map_err(|e| e.to_string())?;
            crate::config::allow_key(&k, crate::config::test_key_allowed())?;
            Ok(k)
        }
        None => Ok(*load_key(key)?.pk),
    }
}

/// The silent nodes of a write, before any transfer. Everything is checked
/// and one question asked before anything is sent: a silent node without a
/// ROM entry goes to the direct write, a ROM write needs its boot core, and
/// `ask` answers for all of them. Then each is rewritten through its ROM.
/// Returns the indices of the nodes that answer, for the normal transfer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_silent_nodes<T: Transport, C: HostClock>(
    host: &mut Host<T, C>,
    t: &Topology,
    topology: &[u8],
    release: &[u8],
    key: &[u8; 32],
    nodes: &[String],
    roms: &[dagon_host::rom::RomMap],
    provision: &std::path::Path,
    ask: &mut dyn FnMut(&str) -> bool,
) -> Result<Vec<u8>, String>
where
    T::Error: std::fmt::Debug,
{
    // Every node's target is checked before anything is sent.
    let indices = nodes
        .iter()
        .map(|node| {
            let index = t
                .nodes
                .iter()
                .position(|n| &n.id == node)
                .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?
                as u8;
            host.target(topology, index)
                .map_err(|e| format!("{node}: {e:?}"))?;
            Ok(index)
        })
        .collect::<Result<Vec<u8>, String>>()?;
    block_on(host.hello(Target::Local)).map_err(|e| match e {
        dagon_host::Error::Timeout => {
            let hub = connected(t).unwrap_or("hub");
            format!("the hub does not answer; write it directly: dagon flash {hub}")
        }
        e => format!("{e:?}"),
    })?;
    let mut answering = Vec::new();
    let mut silent = Vec::new();
    for (node, &index) in nodes.iter().zip(&indices) {
        match block_on(host.write_path(index, topology, roms)).map_err(|e| format!("{e:?}"))? {
            WritePath::Transfer => answering.push(index),
            WritePath::Rom => {
                let boot_path = provision.join(node).join("bootcore.img");
                let boot = std::fs::read(&boot_path).map_err(|_| {
                    format!(
                        "{node} does not answer; its ROM write needs {}",
                        boot_path.display()
                    )
                })?;
                // Stage0 as the first write left it, for an MCU with a stage0.
                let stage0 =
                    std::fs::read(provision.join(node).join(crate::commands::STAGE0_FILE)).ok();
                silent.push((node, index, boot, stage0));
            }
            WritePath::Direct => {
                return Err(format!(
                    "{node} does not answer; write it directly: dagon flash {node}"
                ))
            }
        }
    }
    if silent.is_empty() {
        return Ok(answering);
    }
    let names = silent
        .iter()
        .map(|s| s.0.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if !ask(&format!(
        "No answer from {names}. Writing through the ROM loader rewrites all their contents. \
         A loose wire also stops an answer: check it first.\n"
    )) {
        return Err(format!(
            "stopped; nothing was written ({names} not touched)"
        ));
    }
    for (node, index, boot, stage0) in &silent {
        let assets = dagon_host::RecoverAssets {
            release,
            topology,
            pubkey: key,
            bootcore: Some(boot),
            stage0: stage0.as_deref(),
            roms,
            rom_confirmed: true,
        };
        let mut print = |e: Event| {
            println!(
                "{node}: {} {}% ({})",
                phase_name(e.phase),
                e.percent,
                e.version
            );
        };
        let (_, v) = block_on(host.recover(*index, &assets, &mut print))
            .map_err(|e| format!("{node}: {e:?}"))?;
        println!("{node}: rewritten through the ROM loader, {v}");
    }
    Ok(answering)
}

pub fn apply(a: &ApplyArgs) -> Res {
    let no_verify = a.no_host_verify;
    let cfg = Config::load(a.config.as_deref())?;
    let dgb = match &a.dgb {
        Some(p) => PathBuf::from(p),
        None => cfg
            .release
            .clone()
            .ok_or("no release: pass <dgb> or set release in dagon.toml")?,
    };
    let t = read_topology(&cfg, &cfg.topology_path(a.topology.as_deref())?)?;
    let ids: Vec<String> = t.nodes.iter().map(|n| n.id.clone()).collect();
    let picked = a
        .node
        .iter()
        .map(|id| {
            ids.iter()
                .position(|x| x == id)
                .map(|i| i as u8)
                .ok_or_else(|| format!("no updatable node '{id}' in the topology"))
        })
        .collect::<Result<Vec<u8>, String>>()?;
    let only = (!picked.is_empty()).then_some(picked.as_slice());
    let key = if no_verify {
        eprintln!(
            "warning: --no-host-verify: signatures and digests are left to the nodes; \
             the topology must still match"
        );
        None
    } else {
        Some(pubkey(a.pubkey.as_deref(), a.key.as_deref(), &cfg)?)
    };
    let bundle = read_file(Path::new(&dgb))?;
    crate::versions::check_number("bundle", &dgb.display().to_string(), &bundle)?;

    let mut host = serial::host(a.port.as_deref())?;
    let reg = crate::defs::registry(&cfg)?;
    // Every package written is kept (out/deploy), so it can be written back later.
    if let Some(k) = &key {
        match crate::kept::store(&cfg, &t, k, &bundle) {
            Ok(id) => println!("kept as {}/{id}.dgb", crate::kept::DIR),
            Err(e) => eprintln!("warning: the package is not kept: {e}"),
        }
    }
    apply_bundle(&mut host, &reg, &bundle, &t, key.as_ref(), only)
}

/// Applies `bundle` (verified under `key`, or unverified when `None`) to
/// the nodes of `t` through `host`, printing progress and one result line
/// per node.
pub fn apply_bundle<T: Transport, C: HostClock>(
    host: &mut Host<T, C>,
    reg: &Registry,
    bundle: &[u8],
    t: &Topology,
    key: Option<&[u8; 32]>,
    only: Option<&[u8]>,
) -> Res
where
    T::Error: std::fmt::Debug,
{
    host.timings = node_timings(reg, t)?;
    host.local = local(reg, t);
    let bin = topology_bin(reg, t)?;
    let ids: Vec<String> = t.nodes.iter().map(|n| n.id.clone()).collect();
    let name = |n: u8| ids.get(n as usize).map_or("?", String::as_str);
    let mut print = |e: Event| {
        let pct = if e.phase == phase::TRANSFER {
            format!(" {}%", e.percent)
        } else {
            String::new()
        };
        let step = if e.phase == phase::CONFIRM && !e.wrote {
            "not written, already on it"
        } else {
            phase_name(e.phase)
        };
        println!("{}: {step}{pct} ({})", name(e.node), e.version);
        print_times(name(e.node), &e);
    };
    let run = match key {
        Some(k) => block_on(host.apply(bundle, &bin, k, only, &mut print)),
        None => block_on(host.apply_unverified(bundle, &bin, only, &mut print)),
    };
    let report = run.map_err(|e| format!("apply refused: {e:?}"))?;
    let mut ok = true;
    for r in &report {
        let line = match &r.outcome {
            Outcome::Done(v) => format!("done, {v}"),
            Outcome::Failed(e) => {
                ok = false;
                format!("stopped: {e:?}")
            }
            Outcome::NotStarted => "not started".into(),
        };
        println!("{}: {line}", name(r.node));
    }
    if ok {
        Ok(())
    } else {
        Err("apply stopped; see above".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::payload::{SlotInfo, SlotVersion};
    use dagon_proto::Cmd;

    #[test]
    fn status_shows_the_running_boot_core_or_why_there_is_none() {
        let mut st = StatusResp {
            active: 1,
            confirmed: 1,
            ..Default::default()
        };
        st.slots[1] = SlotInfo {
            state: SlotState::Confirmed,
            version: Version::new(1, 2, 0),
            digest16: [0xAB; 16],
        };
        let line = core_line::<()>("hub", Ok(st), None);
        assert!(
            line.starts_with("hub: boot core 1.2.0 (slot B, digest abababababababab)"),
            "{line}"
        );
        let none = core_line::<()>(
            "watch",
            Err(dagon_host::Error::Rejected {
                cmd: Cmd::Status,
                status: Status::Unsupported,
            }),
            None,
        );
        assert_eq!(
            none,
            "watch: boot core not written through releases on this node"
        );
    }

    #[test]
    fn version_line_names_each_part_and_unknown_or_none() {
        let sv = |state, version| SlotVersion { state, version };
        let v = VersionResp {
            wire: Some(1),
            mcu_id: Some(2),
            stage0: Some(Version::new(0, 1, 0)),
            boot_core: Some(Version::new(1, 0, 0)),
            core_slots: Some(VersionSlots {
                active: 0,
                slots: [
                    sv(SlotState::Confirmed, Version::new(1, 0, 0)),
                    SlotVersion::default(),
                ],
            }),
            app_slots: Some(VersionSlots {
                active: 1,
                slots: [
                    sv(SlotState::Written, Version::new(1, 0, 0)),
                    sv(SlotState::Confirmed, Version::new(1, 1, 0)),
                    SlotVersion::default(),
                ],
            }),
        };
        let self_boot = Some(port_kind::SELF_BOOT);
        assert_eq!(
            version_line::<()>("seg", Ok(Some(v)), self_boot, None),
            "seg: versions: stage0 0.1.0; boot core A:Confirmed 1.0.0 (running), B:Empty; \
             app A:Written 1.0.0, B:Confirmed 1.1.0 (running), F:Empty; wire 1; mcu 0x0002"
        );
        let left_out = VersionResp {
            stage0: None,
            boot_core: None,
            core_slots: None,
            ..v
        };
        let line = version_line::<()>("seg", Ok(Some(left_out)), self_boot, None);
        assert!(
            line.contains("stage0 unknown; boot core unknown;"),
            "{line}"
        );
        let watch = version_line::<()>(
            "watch",
            Ok(Some(left_out)),
            Some(port_kind::VENDOR_STAGE2),
            None,
        );
        assert!(
            watch.contains("stage0 none (vendor 2nd stage); boot core none (vendor 2nd stage);"),
            "{watch}"
        );
        assert_eq!(
            version_line::<()>("seg", Ok(None), self_boot, None),
            "seg: versions unknown (its agent predates VERSION)"
        );
        let e = version_line::<()>("seg", Err(dagon_host::Error::Timeout), None, None);
        assert_eq!(e, "seg: versions unreadable (Timeout)");
    }

    /// A node whose definition gives the app and the boot core one slot
    /// each shows those slots only, never a slot B it does not have.
    #[test]
    fn the_status_lines_show_the_slots_the_definition_gives() {
        let reg = dagon_topology::defs::Registry::builtin();
        let rows = Rows::of(reg, "stm32f401re").unwrap();
        let one = Rows {
            app: vec![0],
            core: vec![0],
            image: vec![0],
        };
        assert_eq!(
            (rows.app.as_slice(), rows.core.as_slice()),
            (&[0, 1][..], &[0, 1][..])
        );
        assert_eq!(
            Rows::of(reg, "esp32s3").unwrap().image,
            [0, 1],
            "no factory slot"
        );
        let mut st = StatusResp {
            active: 0,
            confirmed: 0,
            ..Default::default()
        };
        st.slots[0] = SlotInfo {
            state: SlotState::Confirmed,
            version: Version::new(1, 0, 0),
            digest16: [0xCD; 16],
        };
        let line = core_line::<()>("seg", Ok(st), Some(&one));
        assert_eq!(
            line,
            "seg: boot core 1.0.0 (slot A, digest cdcdcdcdcdcdcdcd), [A:Confirmed 1.0.0]"
        );
        let sv = SlotVersion {
            state: SlotState::Confirmed,
            version: Version::new(1, 0, 0),
        };
        let v = VersionResp {
            wire: Some(1),
            mcu_id: Some(2),
            stage0: None,
            boot_core: None,
            core_slots: Some(VersionSlots {
                active: 0,
                slots: [sv, SlotVersion::default()],
            }),
            app_slots: Some(VersionSlots {
                active: 0,
                slots: [sv, SlotVersion::default(), SlotVersion::default()],
            }),
        };
        let line = version_line::<()>("seg", Ok(Some(v)), None, Some(&one));
        assert!(
            line.contains(
                "boot core A:Confirmed 1.0.0 (running); app A:Confirmed 1.0.0 (running);"
            ),
            "{line}"
        );
        assert_eq!(slots_of(&st, Some(&one.app)), "A:Confirmed 1.0.0");
    }
}
