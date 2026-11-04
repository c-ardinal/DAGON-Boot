//! `status`, `apply`, `rollback`, `recover`, `topo push`: thin shells over
//! dagon-host on the hub's serial port.

use std::path::{Path, PathBuf};

use dagon_format::Version;
use dagon_host::{Clock, Event, Host, NodeState, Outcome, Target, Transport};
use dagon_proto::payload::phase;
use dagon_sign::to_hex;
use dagon_sim::block_on;
use dagon_topology::defs::Registry;
use dagon_topology::Topology;

use crate::config::{load_key, read_file, read_topology, Config, Opts};
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

/// The topology from `--topology` or `dagon.toml`, if any.
fn optional_topology(o: &Opts<'_>, cfg: &Config) -> Result<Option<Topology>, String> {
    match o
        .get("topology")
        .map(PathBuf::from)
        .or(cfg.topology.clone())
    {
        Some(p) => read_topology(cfg, &p).map(Some),
        None => Ok(None),
    }
}

pub fn status(args: &[&str]) -> Res {
    let o = Opts::parse(args, &["port", "topology", "config"])?;
    let cfg = Config::load(o.get("config"))?;
    let t = optional_topology(&o, &cfg)?;
    println!("{}", crate::versions::known_text());
    let mut host = serial::host(o.get("port"))?;
    let hub = block_on(host.status(Target::Hub)).map_err(|e| format!("hub: {e:?}"))?;
    show_state("hub", &hub);
    let Some(t) = t else {
        return Ok(());
    };
    let bin = topology_bin(&crate::defs::registry(&cfg)?, &t)?;
    let view = dagon_format::TopologyView::parse(&bin).map_err(|e| format!("{e:?}"))?;
    for (i, n) in t.nodes.iter().enumerate() {
        match Target::of_node(&view, i as u8) {
            Some(Target::Hub) => {}
            Some(to) => match block_on(host.status(to)) {
                Ok(s) => show_state(&n.id, &s),
                Err(e) => println!("{}: no answer ({e:?})", n.id),
            },
            None => println!("{}: not reachable through the hub", n.id),
        }
    }
    Ok(())
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

pub fn rollback(args: &[&str]) -> Res {
    let o = Opts::parse(args, &["node", "port", "topology", "config"])?;
    if !o.pos.is_empty() {
        return Err("rollback takes --node <id>".into());
    }
    let node = o.need("node")?;
    let cfg = Config::load(o.get("config"))?;
    let t = read_topology(&cfg, &cfg.topology_path(o.get("topology"))?)?;
    let mut host = serial::host(o.get("port"))?;
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

pub fn topo_push(args: &[&str]) -> Res {
    let o = Opts::parse(args, &["port", "config"])?;
    let cfg = Config::load(o.get("config"))?;
    let path = match o.pos[..] {
        [p] => PathBuf::from(p),
        [] => cfg.topology_path(None)?,
        _ => return Err("topo push takes at most one <yaml>".into()),
    };
    let bin = topology_bin(&crate::defs::registry(&cfg)?, &read_topology(&cfg, &path)?)?;
    let mut host = serial::host(o.get("port"))?;
    block_on(host.push_topology(&bin)).map_err(|e| format!("topo push: {e:?}"))?;
    println!(
        "{}: {} bytes sent to the hub, sha256 {}",
        path.display(),
        bin.len(),
        to_hex(&dagon_format::sha256(&bin))
    );
    Ok(())
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
pub(crate) fn pubkey(o: &Opts<'_>, cfg: &Config) -> Result<[u8; 32], String> {
    match o.get("pubkey").map(PathBuf::from).or(cfg.pubkey.clone()) {
        Some(p) => {
            let k = dagon_sign::load_public_key(&p).map_err(|e| e.to_string())?;
            crate::config::allow_key(&k, crate::config::test_key_allowed())?;
            Ok(k)
        }
        None => Ok(*load_key(o.get("key"))?.pk),
    }
}

pub fn recover(args: &[&str]) -> Res {
    let o = Opts::parse(
        args,
        &[
            "port", "topology", "config", "release", "bootcore", "pubkey", "key",
        ],
    )?;
    let [node] = o.pos[..] else {
        return Err("recover needs <node>".into());
    };
    let cfg = Config::load(o.get("config"))?;
    let t = read_topology(&cfg, &cfg.topology_path(o.get("topology"))?)?;
    let bin = topology_bin(&crate::defs::registry(&cfg)?, &t)?;
    let index =
        t.nodes
            .iter()
            .position(|n| n.id == node)
            .ok_or_else(|| format!("no updatable node '{node}' in the topology"))? as u8;
    let dgb = o
        .get("release")
        .map(PathBuf::from)
        .or(cfg.release.clone())
        .ok_or("no release: pass --release <dgb> or set release in dagon.toml")?;
    let release = read_file(&dgb)?;
    crate::versions::check_number("bundle", &dgb.display().to_string(), &release)?;
    let boot_path = o.get("bootcore").map_or_else(
        || cfg.provision_dir().join(node).join("bootcore.img"),
        PathBuf::from,
    );
    // Only the ROM path needs it; a missing file is reported if it gets there.
    let boot = std::fs::read(&boot_path).ok();
    let key = pubkey(&o, &cfg)?;
    let roms = dagon_host::rom::rom_maps(&crate::defs::registry(&cfg)?);
    let assets = dagon_host::RecoverAssets {
        release: &release,
        topology: &bin,
        pubkey: &key,
        bootcore: boot.as_deref(),
        roms: &roms,
    };
    let mut host = serial::host(o.get("port"))?;
    let mut print = |e: Event| {
        println!(
            "{node}: {} {}% ({})",
            phase_name(e.phase),
            e.percent,
            e.version
        );
    };
    let (path, v) = block_on(host.recover(index, &assets, &mut print)).map_err(|e| match e {
        dagon_host::Error::NoImage { .. } if boot.is_none() => format!(
            "{node} does not answer; the ROM path needs {}",
            boot_path.display()
        ),
        e => format!("recover {node}: {e:?}"),
    })?;
    match path {
        dagon_host::RecoverPath::Rom(s) => println!(
            "{node}: rewritten through the ROM loader, {v}; {} frames, max round trip {} ms, \
             {} ms, {} session(s){}",
            s.frames,
            s.max_rtt_ms,
            s.elapsed_ms,
            s.attempts,
            if s.no_stretch {
                ""
            } else {
                ", stretching commands"
            }
        ),
        p => println!("{node}: {p:?}, {v}"),
    }
    Ok(())
}

/// Flag of `apply`: skip the PC-side signature and digest checks.
const NO_HOST_VERIFY: &str = "--no-host-verify";

pub fn apply(args: &[&str]) -> Res {
    let no_verify = args.contains(&NO_HOST_VERIFY);
    let args: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| *a != NO_HOST_VERIFY)
        .collect();
    let o = Opts::parse(
        &args,
        &["port", "topology", "config", "node", "pubkey", "key"],
    )?;
    let cfg = Config::load(o.get("config"))?;
    let dgb = match o.pos[..] {
        [p] => PathBuf::from(p),
        [] => cfg
            .release
            .clone()
            .ok_or("no release: pass <dgb> or set release in dagon.toml")?,
        _ => return Err("apply takes at most one <dgb>".into()),
    };
    let t = read_topology(&cfg, &cfg.topology_path(o.get("topology"))?)?;
    let ids: Vec<String> = t.nodes.iter().map(|n| n.id.clone()).collect();
    let picked = o
        .all("node")
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
            "warning: {NO_HOST_VERIFY}: signatures and digests are left to the nodes; \
             the topology must still match"
        );
        None
    } else {
        Some(pubkey(&o, &cfg)?)
    };
    let bundle = read_file(Path::new(&dgb))?;
    crate::versions::check_number("bundle", &dgb.display().to_string(), &bundle)?;

    let mut host = serial::host(o.get("port"))?;
    let reg = crate::defs::registry(&cfg)?;
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
