//! `conform`: runs dagon-conform's 12 tests against one node, through the
//! hub (RELAY) or on a node plugged in directly (`--usb`, run as a UART
//! link), and writes the result file (JSON) and a static HTML report.

use std::fmt::Write as _;
use std::path::PathBuf;

use dagon_conform::pc::{Channel, Images, NoAnswer, Runner, Signer, Timings};
use dagon_conform::report::{CtResult, Progress, Report};
use dagon_conform::{Label, CTS};
use dagon_format::header::SIGN_DOMAIN;
use dagon_host::{Error, Host, HostClock, HostTimings, Target, Transport};
use dagon_port::timeout::NodeTimings;
use dagon_port::LinkKind;
use dagon_sim::block_on;

use crate::cli::ConformArgs;
use crate::config::{load_key, read_file, read_topology, Config};

/// Where conformance results go in the work area, per node.
pub(crate) const CONFORM_DIR: &str = "out/conform";
use crate::serial;

pub const RESULT_FILE: &str = "result.json";
pub const REPORT_FILE: &str = "report.html";
/// The conformance images in `dir`, one good and one no-confirm per app
/// slot of the node: `good-a.img` and `no-confirm-a.img`, then the same for
/// slot b when the node has it (either file there).
pub(crate) fn conform_images(dir: &std::path::Path) -> Result<ConformImages, String> {
    let mut images = ConformImages::default();
    for (i, s) in crate::commands::slot_letters().enumerate() {
        let good = dir.join(format!("good-{s}.img"));
        let no_confirm = dir.join(format!("no-confirm-{s}.img"));
        if i > 0 && !good.exists() && !no_confirm.exists() {
            break;
        }
        images.good.push(read_file(&good)?);
        images.no_confirm.push(read_file(&no_confirm)?);
    }
    Ok(images)
}

/// The images a run sends, index = slot.
#[derive(Default)]
pub struct ConformImages {
    pub good: Vec<Vec<u8>>,
    pub no_confirm: Vec<Vec<u8>>,
}

/// dagon-conform's synchronous channel over a dagon-host session.
pub struct HostChannel<'h, T: Transport, C: HostClock> {
    pub host: &'h mut Host<T, C>,
    pub to: Target,
}

impl<T: Transport, C: HostClock> Channel for HostChannel<'_, T, C> {
    fn request(
        &mut self,
        cmd: dagon_proto::Cmd,
        payload: &[u8],
        timeout_ms: u32,
        resp: &mut [u8],
    ) -> Result<usize, NoAnswer> {
        match block_on(self.host.call(self.to, cmd, payload, timeout_ms)) {
            Ok(body) if body.len() < resp.len() => {
                resp[0] = dagon_proto::Status::Ok as u8;
                resp[1..=body.len()].copy_from_slice(&body);
                Ok(1 + body.len())
            }
            Err(Error::Rejected { status, .. }) => {
                resp[0] = status as u8;
                Ok(1)
            }
            _ => Err(NoAnswer),
        }
    }

    fn now_ms(&mut self) -> u64 {
        self.host.clock.now_ms()
    }

    fn sleep_ms(&mut self, ms: u32) {
        block_on(self.host.clock.sleep_ms(ms));
    }
}

/// Runs all 12 tests. `images` are one good and one no-confirm image per
/// app slot of the node; `sign` signs re-built headers for the compatibility
/// test; `timings` are the node's own limits (its MCU definition's).
pub fn run<T: Transport, C: HostClock>(
    host: &mut Host<T, C>,
    to: Target,
    link: LinkKind,
    images: &ConformImages,
    sign: Signer<'_>,
    timings: Timings<'_>,
) -> Report {
    let good: Vec<&[u8]> = images.good.iter().map(Vec::as_slice).collect();
    let no_confirm: Vec<&[u8]> = images.no_confirm.iter().map(Vec::as_slice).collect();
    let imgs = Images {
        good: &good,
        no_confirm: &no_confirm,
    };
    let ch = HostChannel { host, to };
    match Runner::new(ch, link, imgs, sign, timings) {
        Ok(mut r) => r.run_all(),
        Err(e) => {
            eprintln!("conform: setup failed: {e}");
            Report::unreachable(link)
        }
    }
}

pub fn json(report: &Report, node: &str) -> String {
    let mut s = String::new();
    report
        .write_json(&mut s, node, Progress::default())
        .expect("writing to a String");
    s
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '&' => "&amp;".into(),
            '"' => "&quot;".into(),
            c => c.to_string(),
        })
        .collect()
}

/// A self-contained HTML page (inline style, no scripts, no external
/// assets).
pub fn html(report: &Report, node: &str) -> String {
    let mut s = String::new();
    let link = report.link.name();
    let _ = write!(
        s,
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>DAGON conformance: {n}</title>\n<style>\
         body{{font:14px system-ui,sans-serif;margin:2em;color:#222}}\
         table{{border-collapse:collapse}}td,th{{border:1px solid #ccc;padding:4px 8px;text-align:left}}\
         .pass{{color:#0a6b2f;font-weight:600}}.fail{{color:#b00020;font-weight:600}}\
         .not_run{{color:#777}}</style></head><body>\n\
         <h1>Conformance: {n}</h1>\n<p>Link {link}, {p} of {t} passed.",
        n = esc(node),
        p = report.passed(),
        t = CTS.len(),
    );
    if let Some(h) = report.hello {
        let _ = write!(
            s,
            " MCU id {:#06x}, port kind {}, version {}.",
            h.mcu_id, h.port_kind, h.active_version
        );
    }
    s.push_str(
        "</p>\n<table><tr><th>Test</th><th>Name</th><th>Side</th><th>Verdict</th>\
         <th>Measured</th><th>Duration</th><th>Detail</th></tr>\n",
    );
    for c in CTS {
        let r = report
            .results
            .iter()
            .find(|r| r.id == c.id)
            .copied()
            .unwrap_or(CtResult::not_run(c.id));
        let v = r.verdict.as_str();
        let detail = String::from_utf8_lossy(r.detail.as_bytes()).into_owned();
        let _ = writeln!(
            s,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"{v}\">{v}</td>\
             <td>{} {}</td><td>{} ms</td><td>{}</td></tr>",
            Label(c.id),
            esc(c.name),
            c.side.as_str(),
            r.measured,
            esc(c.unit),
            r.duration_ms,
            esc(&detail)
        );
    }
    s.push_str("</table>\n</body></html>\n");
    s
}

/// The link kind a node plugged in directly (`--usb`) runs as: the UART
/// link's payload limit and speed floor.
const DIRECT: LinkKind = LinkKind::Uart;

/// The relay target of topology node `node` (not the node the PC is
/// connected to) and the kind of the link it is reached over, from that
/// node's relay table entry.
pub(crate) fn node_target(
    reg: &dagon_topology::defs::Registry,
    t: &dagon_topology::Topology,
    node: &str,
) -> Result<(Target, LinkKind), String> {
    let to = crate::device::target_of(reg, t, node)?;
    let Target::Linked { link, .. } = to else {
        return Err(format!(
            "'{node}' is the node the PC is connected to; use --usb"
        ));
    };
    let local = crate::device::local(reg, t).map_err(|e| e.to_string())?;
    let kind = local
        .links
        .iter()
        .find(|d| d.id == link)
        .map(|d| d.kind)
        .ok_or_else(|| format!("'{node}': link {link} is not in the relay table"))?;
    Ok((to, kind))
}

/// Node `node`'s own limits from its MCU definition; the fallback for a
/// node or an MCU the definitions do not name.
pub(crate) fn node_timings(
    reg: &dagon_topology::defs::Registry,
    t: &dagon_topology::Topology,
    node: &str,
) -> Result<HostTimings, String> {
    let i = t.nodes.iter().position(|n| n.id == node);
    let all = dagon_host::node_timings(reg, t).map_err(|e| e.to_string())?;
    Ok(i.and_then(|i| all.into_iter().nth(i))
        .map_or(NodeTimings::FALLBACK, |(_, t)| t))
}

/// Runs the 12 tests on the hub's port and writes the result file and the
/// report into `out`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn conform_files(
    node: &str,
    (to, link): (Target, LinkKind),
    timings: &HostTimings,
    images_dir: &std::path::Path,
    out: &crate::genfs::Out,
    base: &std::path::Path,
    key: &ed25519_compact::KeyPair,
    port: Option<&str>,
) -> Result<Report, String> {
    let images = conform_images(images_dir)?;
    let mut sign = |m: &[u8]| *key.sk.sign([SIGN_DOMAIN, m].concat(), None);
    let mut host = serial::host(port)?;
    let report = run(&mut host, to, link, &images, &mut sign, timings.borrowed());
    crate::genfs::write_out(
        base,
        &out.join(RESULT_FILE)?,
        json(&report, node).as_bytes(),
    )?;
    crate::genfs::write_out(
        base,
        &out.join(REPORT_FILE)?,
        html(&report, node).as_bytes(),
    )?;
    Ok(report)
}

pub fn command(a: &ConformArgs) -> Result<(), String> {
    let usb = a.usb;
    let node = a.node.as_str();
    let cfg = Config::load(a.config.as_deref())?;
    // With --usb no topology is needed: the node's limits are its MCU
    // definition's when the topology names it, else the fallback.
    let reg = crate::defs::registry(&cfg)?;
    let topology = cfg
        .topology_path(a.topology.as_deref())
        .and_then(|p| read_topology(&cfg, &p));
    let (to, timings) = match (usb, topology) {
        (false, t) => {
            let t = t?;
            (node_target(&reg, &t, node)?, node_timings(&reg, &t, node)?)
        }
        (true, Ok(t)) => ((Target::Local, DIRECT), node_timings(&reg, &t, node)?),
        (true, Err(_)) => ((Target::Local, DIRECT), NodeTimings::FALLBACK),
    };
    let dir = a
        .images
        .as_ref()
        .map_or_else(|| cfg.base.join(CONFORM_DIR).join(node), PathBuf::from);
    let key = load_key(a.key.as_deref())?;
    // Results go to --out, else next to the images (--images), else to
    // out/conform/<node> in the work area.
    let out = match (a.out.as_deref(), a.images.as_deref()) {
        (Some(p), _) => crate::genfs::Out::User(PathBuf::from(p)),
        (None, Some(_)) => crate::genfs::Out::of(&cfg.base, &dir)?,
        (None, None) => {
            crate::genfs::Out::Work(crate::genfs::Rel::new(format!("{CONFORM_DIR}/{node}"))?)
        }
    };
    let report = conform_files(
        node,
        to,
        &timings,
        &dir,
        &out,
        &cfg.base,
        &key,
        a.port.as_deref(),
    )?;
    for c in CTS {
        if let Some(r) = report.results.iter().find(|r| r.id == c.id) {
            println!("{} {:<24} {}", Label(c.id), c.name, r.verdict.as_str());
        }
    }
    println!(
        "{node}: {} of {} passed; {} and {} in {}",
        report.passed(),
        CTS.len(),
        RESULT_FILE,
        REPORT_FILE,
        out.path(&cfg.base).display()
    );
    if report.passed() == CTS.len() {
        Ok(())
    } else {
        Err("conformance not passed".into())
    }
}

#[cfg(test)]
mod tests;
