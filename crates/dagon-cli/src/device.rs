//! `status`, `apply`, `rollback`, `topo push`: thin shells over
//! dagon-host on the hub's serial port.

use std::path::{Path, PathBuf};

use dagon_format::header::port_kind;
use dagon_format::Version;
use dagon_host::{Clock, Event, Host, NodeState, Outcome, Target, Transport, WritePath};
use dagon_proto::payload::{phase, SlotState, StatusResp, VersionResp, VersionSlots};
use dagon_proto::Status;
use dagon_sign::to_hex;
use dagon_sim::block_on;
use dagon_topology::defs::Registry;
use dagon_topology::Topology;

use crate::cli::{ApplyArgs, RollbackArgs, StatusArgs, TopoPushArgs};
use crate::config::{load_key, read_file, read_topology, Config};
use crate::serial;

type Res = Result<(), String>;

fn topology_bin(reg: &Registry, t: &Topology) -> Result<Vec<u8>, String> {
    dagon_topology::compile_with(reg, t).map_err(|e| e.to_string())
}

fn show_state(name: &str, s: &NodeState) {
    let st = &s.status;
    let slots: Vec<String> = st
        .slots
        .iter()
        .zip(["A", "B", "F"])
        .map(|(x, n)| format!("{n}:{:?} {}", x.state, x.version))
        .collect();
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
        slots.join(", ")
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
) -> String {
    match r {
        Ok(st) => {
            let slots: Vec<String> = st.slots[..2]
                .iter()
                .zip(["A", "B"])
                .map(|(x, n)| format!("{n}:{:?} {}", x.state, x.version))
                .collect();
            let running = st.slots.get(usize::from(st.active));
            match running {
                Some(r) => format!(
                    "{name}: boot core {} (slot {}, digest {}), [{}]",
                    r.version,
                    if st.active == 0 { "A" } else { "B" },
                    to_hex(&r.digest16[..8]),
                    slots.join(", ")
                ),
                None => format!("{name}: boot core unknown, [{}]", slots.join(", ")),
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
    let hub = block_on(host.status(Target::Hub)).map_err(|e| format!("hub: {e:?}"))?;
    show_state("hub", &hub);
    println!(
        "{}",
        core_line("hub", block_on(host.boot_core_status(Target::Hub)))
    );
    let port = Some(hub.hello.port_kind);
    println!(
        "{}",
        version_line("hub", block_on(host.version(Target::Hub)), port)
    );
    let Some(t) = t else {
        return Ok(());
    };
    for (id, to) in node_targets(&cfg, &t)? {
        match to {
            Some(to) => match block_on(host.status(to)) {
                Ok(s) => {
                    show_state(&id, &s);
                    println!("{}", core_line(&id, block_on(host.boot_core_status(to))));
                    let port = Some(s.hello.port_kind);
                    println!("{}", version_line(&id, block_on(host.version(to)), port));
                }
                Err(e) => println!("{id}: no answer ({e:?})"),
            },
            None => println!("{id}: not reachable through the hub"),
        }
    }
    Ok(())
}

/// The topology's nodes other than the hub, each with its route through
/// the hub (`None`: not reachable through it).
fn node_targets(cfg: &Config, t: &Topology) -> Result<Vec<(String, Option<Target>)>, String> {
    let bin = topology_bin(&crate::defs::registry(cfg)?, t)?;
    let view = dagon_format::TopologyView::parse(&bin).map_err(|e| format!("{e:?}"))?;
    Ok(t.nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), Target::of_node(&view, i as u8)))
        .filter(|(_, to)| *to != Some(Target::Hub))
        .collect())
}

/// The VERSION line of `dagon status` and `dagon doctor`: stage0, boot
/// core slots, app slots, wire version and MCU id. A value the node left
/// out is "unknown", or "none" on an MCU booted by its vendor's 2nd stage
/// (HELLO `port_kind`); a node whose agent predates VERSION is "unknown".
pub(crate) fn version_line<E: std::fmt::Debug>(
    name: &str,
    r: Result<Option<VersionResp>, dagon_host::Error<E>>,
    port: Option<u8>,
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
        .map_or_else(absent, |c| slots_text(&c, &["A", "B"]));
    let app = v
        .app_slots
        .map_or_else(unknown, |a| slots_text(&a, &["A", "B", "F"]));
    let wire = v.wire.map_or_else(unknown, |w| w.to_string());
    let mcu = v.mcu_id.map_or_else(unknown, |m| format!("{m:#06x}"));
    format!(
        "{name}: versions: stage0 {stage0}; boot core {core}; app {app}; wire {wire}; mcu {mcu}"
    )
}

/// `A:Confirmed 1.0.0 (running), B:Empty`
fn slots_text<const N: usize>(s: &VersionSlots<N>, names: &[&str]) -> String {
    let parts: Vec<String> = s
        .slots
        .iter()
        .zip(names)
        .enumerate()
        .map(|(i, (x, n))| {
            let running = if i == usize::from(s.active) {
                " (running)"
            } else {
                ""
            };
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
pub(crate) fn version_lines<T: Transport, C: Clock>(
    host: &mut Host<T, C>,
    nodes: &[(String, Option<Target>)],
) -> Vec<String> {
    let mut one = |name: &str, to: Target| {
        let port = block_on(host.hello(to)).ok().map(|h| h.port_kind);
        version_line(name, block_on(host.version(to)), port)
    };
    let mut out = vec![one("hub", Target::Hub)];
    for (id, to) in nodes {
        out.push(match to {
            Some(to) => one(id, *to),
            None => format!("{id}: not reachable through the hub"),
        });
    }
    out
}

/// Rolls topology node `node` back to the version in its other slot and
/// confirms it there; returns that version.
pub fn rollback_node<T: Transport, C: Clock>(
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
    let bin = topology_bin(reg, t)?;
    let view = dagon_format::TopologyView::parse(&bin).map_err(|e| format!("{e:?}"))?;
    let to = Target::of_node(&view, i as u8)
        .ok_or_else(|| format!("'{node}' is not reachable through the hub"))?;
    block_on(host.rollback(to, i as u8, on_event)).map_err(|e| match e {
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

pub fn topo_push(a: &TopoPushArgs) -> Res {
    let cfg = Config::load(a.config.as_deref())?;
    let path = match &a.yaml {
        Some(p) => PathBuf::from(p),
        None => cfg.topology_path(None)?,
    };
    let bin = topology_bin(&crate::defs::registry(&cfg)?, &read_topology(&cfg, &path)?)?;
    let mut host = serial::host(a.port.as_deref())?;
    block_on(host.push_topology(&bin)).map_err(|e| format!("topo push: {e:?}"))?;
    println!(
        "{}: {} bytes sent to the hub, sha256 {}",
        path.display(),
        bin.len(),
        to_hex(&dagon_format::sha256(&bin))
    );
    Ok(())
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

/// The silent nodes of a write, before any transfer: one with a ROM entry is
/// rewritten through it once `ask` says yes; one without goes to the direct
/// write and nothing is written. Returns the indices of those that answer,
/// for the normal transfer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_silent_nodes<T: Transport, C: Clock>(
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
    // A hub routes by its topology, so a node is silent only after it has one.
    block_on(host.push_topology(topology)).map_err(|e| match e {
        dagon_host::Error::Timeout => {
            let hub = t.nodes.first().map_or("hub", |n| n.id.as_str());
            format!("the hub does not answer; write it directly: dagon flash {hub}")
        }
        e => format!("{e:?}"),
    })?;
    let mut answering = Vec::new();
    let mut rom = Vec::new();
    for node in nodes {
        let index = t
            .nodes
            .iter()
            .position(|n| &n.id == node)
            .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?
            as u8;
        match block_on(host.write_path(index, topology, roms)).map_err(|e| format!("{e:?}"))? {
            WritePath::Transfer => answering.push(index),
            WritePath::Rom => rom.push((node, index)),
            WritePath::Direct => {
                return Err(format!(
                    "{node} does not answer; write it directly: dagon flash {node}"
                ))
            }
        }
    }
    for (node, index) in rom {
        let boot_path = provision.join(node).join("bootcore.img");
        let boot = std::fs::read(&boot_path).map_err(|_| {
            format!(
                "{node} does not answer; its ROM write needs {}",
                boot_path.display()
            )
        })?;
        // Stage0 as the first write left it, for an MCU with a stage0.
        let stage0 = std::fs::read(provision.join(node).join(crate::commands::STAGE0_FILE)).ok();
        if !ask(&format!(
            "{node} does not answer. Writing it through its ROM loader rewrites all its \
             contents. A loose wire also stops an answer: check it first.\n"
        )) {
            return Err(format!("stopped; {node} was not touched"));
        }
        let assets = dagon_host::RecoverAssets {
            release,
            topology,
            pubkey: key,
            bootcore: Some(&boot),
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
        let (_, v) = block_on(host.recover(index, &assets, &mut print))
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
pub fn apply_bundle<T: Transport, C: Clock>(
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
    let bin = topology_bin(reg, t)?;
    let ids: Vec<String> = t.nodes.iter().map(|n| n.id.clone()).collect();
    let name = |n: u8| ids.get(n as usize).map_or("?", String::as_str);
    let mut print = |e: Event| {
        let pct = if e.phase == phase::TRANSFER {
            format!(" {}%", e.percent)
        } else {
            String::new()
        };
        println!(
            "{}: {}{pct} ({})",
            name(e.node),
            phase_name(e.phase),
            e.version
        );
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
        let line = core_line::<()>("hub", Ok(st));
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
            version_line::<()>("seg", Ok(Some(v)), self_boot),
            "seg: versions: stage0 0.1.0; boot core A:Confirmed 1.0.0 (running), B:Empty; \
             app A:Written 1.0.0, B:Confirmed 1.1.0 (running), F:Empty; wire 1; mcu 0x0002"
        );
        let left_out = VersionResp {
            stage0: None,
            boot_core: None,
            core_slots: None,
            ..v
        };
        let line = version_line::<()>("seg", Ok(Some(left_out)), self_boot);
        assert!(
            line.contains("stage0 unknown; boot core unknown;"),
            "{line}"
        );
        let watch = version_line::<()>("watch", Ok(Some(left_out)), Some(port_kind::VENDOR_STAGE2));
        assert!(
            watch.contains("stage0 none (vendor 2nd stage); boot core none (vendor 2nd stage);"),
            "{watch}"
        );
        assert_eq!(
            version_line::<()>("seg", Ok(None), self_boot),
            "seg: versions unknown (its agent predates VERSION)"
        );
        let e = version_line::<()>("seg", Err(dagon_host::Error::Timeout), None);
        assert_eq!(e, "seg: versions unreadable (Timeout)");
    }
}
