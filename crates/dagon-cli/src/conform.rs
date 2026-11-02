//! `conform`: runs dagon-conform's 12 tests against one node, through the
//! hub (RELAY) or on a node plugged in directly (`--usb`, run as a UART
//! link), and writes the result file (JSON) and a static HTML report.

use std::fmt::Write as _;
use std::path::PathBuf;

use dagon_conform::pc::{Channel, Images, NoAnswer, Runner, Signer};
use dagon_conform::report::{CtResult, Progress, Report};
use dagon_conform::{Label, CTS};
use dagon_format::header::SIGN_DOMAIN;
use dagon_host::{Clock, Error, Host, Target, Transport};
use dagon_port::LinkKind;
use dagon_sim::block_on;

use crate::config::{load_key, read_file, read_topology, write_file, Config, Opts};
use crate::serial;

pub const RESULT_FILE: &str = "result.json";
pub const REPORT_FILE: &str = "report.html";
/// Image files expected in the images directory.
pub const IMAGE_FILES: [&str; 4] = [
    "good-a.img",
    "good-b.img",
    "no-confirm-a.img",
    "no-confirm-b.img",
];

/// dagon-conform's synchronous channel over a dagon-host session.
pub struct HostChannel<'h, T: Transport, C: Clock> {
    pub host: &'h mut Host<T, C>,
    pub to: Target,
}

impl<T: Transport, C: Clock> Channel for HostChannel<'_, T, C> {
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

/// Runs all 12 tests. `images` are good A, good B, no-confirm A, no-confirm
/// B; `sign` signs re-built headers for the compatibility test.
pub fn run<T: Transport, C: Clock>(
    host: &mut Host<T, C>,
    to: Target,
    link: LinkKind,
    images: &[Vec<u8>; 4],
    sign: Signer<'_>,
) -> Report {
    let imgs = Images {
        good: [&images[0], &images[1]],
        no_confirm: [&images[2], &images[3]],
    };
    let ch = HostChannel { host, to };
    match Runner::new(ch, link, imgs, sign) {
        Ok(mut r) => r.run_all(),
        Err(e) => {
            eprintln!("conform: setup failed: {e:?}");
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
    let link = match report.link {
        LinkKind::I2cTarget => "I2C",
        LinkKind::Uart => "UART",
        LinkKind::UsbCdc => "USB",
    };
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

fn link_of(to: Target) -> LinkKind {
    match to {
        Target::I2c(_) => LinkKind::I2cTarget,
        // Direct USB runs as the UART link (same payload limit and speed floor).
        Target::Uart | Target::Hub => LinkKind::Uart,
    }
}

pub fn command(args: &[&str]) -> Result<(), String> {
    let usb = args.contains(&"--usb");
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != "--usb").collect();
    let o = Opts::parse(
        &args,
        &["node", "images", "out", "port", "topology", "config", "key"],
    )?;
    if !o.pos.is_empty() {
        return Err("conform takes options only".into());
    }
    let node = o.need("node")?;
    let cfg = Config::load(o.get("config"))?;
    let to = if usb {
        Target::Hub
    } else {
        let t = read_topology(&cfg.topology_path(o.get("topology"))?)?;
        let bin = dagon_topology::compile(&t).map_err(|e| e.to_string())?;
        let view = dagon_format::TopologyView::parse(&bin).map_err(|e| format!("{e:?}"))?;
        let i = t
            .nodes
            .iter()
            .position(|n| n.id == node)
            .ok_or_else(|| format!("no node '{node}' in the topology"))?;
        match Target::of_node(&view, i as u8) {
            Some(Target::Hub) => return Err(format!("'{node}' is the hub; use --usb")),
            Some(to) => to,
            None => return Err(format!("'{node}' is not reachable through the hub")),
        }
    };
    let dir = o
        .get("images")
        .map_or_else(|| PathBuf::from("out/conform").join(node), PathBuf::from);
    let images = IMAGE_FILES.map(|f| read_file(&dir.join(f)));
    let [a, b, c, d] = images;
    let images = [a?, b?, c?, d?];
    let key = load_key(o.get("key"))?;
    let mut sign = |m: &[u8]| *key.sk.sign([SIGN_DOMAIN, m].concat(), None);
    let mut host = serial::host(o.get("port"))?;
    let report = run(&mut host, to, link_of(to), &images, &mut sign);

    let out = o.get("out").map_or_else(|| dir.clone(), PathBuf::from);
    write_file(&out.join(RESULT_FILE), json(&report, node).as_bytes())?;
    write_file(&out.join(REPORT_FILE), html(&report, node).as_bytes())?;
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
        out.display()
    );
    if report.passed() == CTS.len() {
        Ok(())
    } else {
        Err("conformance not passed".into())
    }
}

#[cfg(test)]
mod tests;
