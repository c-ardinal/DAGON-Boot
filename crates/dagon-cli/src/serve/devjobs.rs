//! `dagon flash` and `dagon doctor` as serve jobs.
//!
//! flash: the plan goes to the job's log, the parts and the app are built,
//! the writer's devices are listed (a UF2 writer takes the hub from the
//! page first and waits for its drive), then the job asks the page about
//! the one device; only the answer "yes" writes to it (anything else, a
//! cancel or no answer writes nothing). Several devices and no `device`
//! in the request: the job ends with the candidates, nothing written.
//!
//! doctor: the checks only; installing tools stays a terminal command.

use std::path::Path;

use dagon_topology::Topology;
use ed25519_compact::KeyPair;
use serde_json::{json, Value};

use super::api::{inside, plain, serve_key};
use super::jobs::Job;
use crate::commands::FirstWrite;
use crate::config::{read_topology, Config};
use crate::flashdev::{self, Candidate, Pick};
use dagon_topology::defs::Writer;

/// The question before the write; the page's answer is the confirmation.
const ASK: usize = 1;

/// Whether the page's answer allows the write.
fn confirmed(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "はい"
    )
}

type Prepare<'a> = dyn FnMut(&Config, &Topology, &str, &KeyPair, &Path, &mut Job) -> Result<FirstWrite, String>
    + 'a;
type Find<'a> =
    dyn FnMut(&Config, &Topology, &str, &mut Job) -> Result<(Writer, Vec<Candidate>), String> + 'a;
type Write<'a> = dyn FnMut(&FirstWrite, &Candidate, &mut Job) -> Result<(), String> + 'a;

/// POST /v1/flash {node, device?}: `device` names one of the candidates
/// the writer reports; with several and none named, the job ends with the
/// candidates (written: false) and writes nothing.
pub fn job_flash(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    flash_with(
        job,
        &cfg,
        &v,
        &mut |cfg, t, node, key, out, job| {
            let cancel = job.cancel_flag();
            crate::flash::prepare(
                cfg,
                t,
                node,
                key,
                out,
                crate::flash::Paths::Relative,
                &mut |l| job.log(l),
                &cancel,
            )
        },
        &mut |cfg, t, node, job| {
            let cancel = job.cancel_flag();
            crate::flash::devices(cfg, t, node, &mut |l| job.log(l), &cancel)
        },
        &mut |w, device, job| {
            let cancel = job.cancel_flag();
            crate::flash::write(w, device, &mut |l| job.log(l), &cancel)
        },
    )
}

fn flash_with(
    job: &mut Job,
    cfg: &Config,
    v: &Value,
    prepare: &mut Prepare<'_>,
    find: &mut Find<'_>,
    write: &mut Write<'_>,
) -> Result<Value, String> {
    let node = v["node"]
        .as_str()
        .filter(|n| plain(n))
        .ok_or("node is required")?;
    // Only ever compared with the candidates, never passed on as given.
    let device = v["device"].as_str();
    let t = read_topology(cfg, &inside(cfg, &cfg.topology_path(None)?)?)?;
    let out = inside(cfg, &cfg.provision_dir().join(node))?;
    // The page gets paths relative to the project or the DAGON source.
    let steps = crate::flash::plan(cfg, &t, node, &out, crate::flash::Paths::Relative)?;
    for l in &steps {
        job.log(l);
    }
    let key = serve_key(cfg)?;
    job.progress("building", None);
    let w = prepare(cfg, &t, node, &key, &out, job)?;
    let mut step = |job: &mut Job| -> Result<Value, String> {
        let (writer, found) = find(cfg, &t, node, job)?;
        let c = match flashdev::pick(found, device, &writer)? {
            Pick::Use(c) => c,
            Pick::Choose(list) => {
                job.log(&flashdev::several(&list, &writer));
                return Ok(json!({ "node": node, "written": false, "candidates": list }));
            }
        };
        job.log(&format!("  device   {}  {}", c.id, c.label));
        // The page shows this question (its own wording, with はい / いいえ);
        // the plan is in the log above. 案: the owner has not fixed the words.
        let question = format!(
            "{node} を {}（{}）に書き込みます。よろしいですか。",
            c.label, c.id
        );
        if !confirmed(&job.ask(ASK, &question)?) {
            return Err("nothing written to the board".into());
        }
        job.progress("writing", None);
        write(&w, &c, job)?;
        Ok(json!({ "node": node, "written": true, "device": c.id }))
    };
    match w {
        // A hub restarts into its boot ROM: the page lets go of its port
        // before the drive is looked for.
        FirstWrite::Uf2 { .. } => job.with_device(step),
        FirstWrite::ProbeRs { .. } | FirstWrite::Espflash { .. } => step(job),
    }
}

/// POST /v1/doctor {}: the tools the project's MCUs need, by its
/// definitions. Nobody can confirm a command here, so a check the project
/// itself defines is reported as one for the terminal's `dagon doctor`.
pub fn job_doctor(job: &mut Job, cfg: Config, _v: Value) -> Result<Value, String> {
    let reg = super::api::project_registry(&cfg)?;
    let mcus: Vec<String> = cfg
        .topology_path(None)
        .and_then(|p| inside(&cfg, &p))
        .and_then(|p| read_topology(&cfg, &p))
        .map(|t| t.nodes.iter().map(|n| n.mcu.clone()).collect())
        .unwrap_or_else(|_| crate::doctor::all_mcus(&reg));
    let mut lines = Vec::new();
    let ok = crate::doctor::check(&reg, &mcus, &mut |l| {
        lines.push(l.to_string());
        job.log(l);
    });
    Ok(json!({ "ok": ok, "lines": lines }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_yes_writes() {
        for a in ["yes", "y", " YES ", "はい"] {
            assert!(confirmed(a), "{a}");
        }
        for a in ["", "no", "yes please", "ok", "n"] {
            assert!(!confirmed(a), "{a}");
        }
    }

    /// A project with seg built from nodes/seg and its own key pair.
    fn project(name: &str) -> (std::path::PathBuf, Config) {
        let dir =
            std::env::temp_dir().join(format!("dagon-flashjob-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let k = dagon_sign::generate_to_dir(&dir.join("keys")).unwrap();
        std::fs::write(
            dir.join("t.yaml"),
            include_str!("../../../../topology/desk-demo.yaml"),
        )
        .unwrap();
        let pk = k.public_path.to_string_lossy().replace(char::from(92), "/");
        let toml = dir.join("dagon.toml");
        std::fs::write(
            &toml,
            format!(
                "topology = \"t.yaml\"\npubkey = {pk:?}\n[build.seg]\npackage = \"nodes/seg\"\n"
            ),
        )
        .unwrap();
        let cfg = Config::load(Some(&toml.to_string_lossy())).unwrap();
        (dir, cfg)
    }

    /// Runs flash_with on fakes with the probes `found` and the request's
    /// `device`, answers `answer` (None: cancels; no answer is asked for
    /// when the job ends without a question), and returns (final job JSON,
    /// devices written).
    fn run(
        name: &str,
        found: &[&str],
        device: Option<&str>,
        answer: Option<&str>,
    ) -> (Value, Vec<String>) {
        let (dir, cfg) = project(name);
        let server =
            super::super::jobs::Server::new("p".repeat(32), 1, None, std::path::PathBuf::new());
        let writes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let w2 = writes.clone();
        let found: Vec<Candidate> = found
            .iter()
            .map(|id| Candidate {
                id: id.to_string(),
                label: "probe".into(),
                // Ids ending in ":M" stand for hint-matched devices.
                matched: id.ends_with(":M"),
                path: None,
            })
            .collect();
        let request = json!({ "node": "seg", "device": device });
        let id = server
            .spawn("flash", move |job| {
                flash_with(
                    job,
                    &cfg,
                    &request,
                    &mut |_, _, _, _, out, _| {
                        Ok(FirstWrite::ProbeRs {
                            bin: out.join("flash.bin"),
                            bytes: 1,
                            commands: vec![],
                        })
                    },
                    &mut |_, _, _, _| Ok((Writer::ProbeRs { chip: "X".into() }, found.clone())),
                    &mut |_, device, _| {
                        w2.lock().unwrap().push(device.id.clone());
                        Ok(())
                    },
                )
            })
            .unwrap();
        let state = |s: &str| server.job_json(id).unwrap()["state"] == s;
        let wait = |busy: &dyn Fn() -> bool| {
            let t = std::time::Instant::now();
            while busy() && t.elapsed() < std::time::Duration::from_secs(10) {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        wait(&|| state("running") || state("queued"));
        if state("question") {
            assert!(
                writes.lock().unwrap().is_empty(),
                "nothing before the answer"
            );
            match answer {
                Some(a) => assert!(server.answer(id, a.into())),
                None => assert!(server.cancel(id)),
            }
            wait(&|| state("question") || state("running"));
        }
        let _ = std::fs::remove_dir_all(&dir);
        let written = writes.lock().unwrap().clone();
        (server.job_json(id).unwrap(), written)
    }

    #[test]
    fn the_board_is_written_only_after_the_page_answers_yes() {
        let (j, w) = run("yes", &["0483:374b:A"], None, Some("yes"));
        assert_eq!(
            (j["state"].as_str(), &w[..]),
            (Some("succeeded"), &["0483:374b:A".to_string()][..]),
            "{j}"
        );
        let (j, w) = run("no", &["0483:374b:A"], None, Some("no"));
        assert_eq!((j["state"].as_str(), w.len()), (Some("failed"), 0), "{j}");
        let (j, w) = run("cancel", &["0483:374b:A"], None, None);
        assert!(w.is_empty(), "{j}");
        assert_ne!(j["state"], "succeeded");
    }

    #[test]
    fn several_devices_and_none_named_end_with_the_candidates() {
        let two = ["0483:374b:A", "0483:374b:B"];
        let (j, w) = run("several", &two, None, None);
        assert!(w.is_empty(), "{j}");
        assert_eq!(j["state"], "succeeded", "{j}");
        assert_eq!(j["result"]["written"], false, "{j}");
        assert_eq!(j["result"]["candidates"][1]["device"], "0483:374b:B", "{j}");

        // A matched device is listed first and flagged, nothing is dropped.
        let three = ["0483:374b:A", "0483:374b:B", "0483:374b:M"];
        let (j, w) = run("matched", &three, None, None);
        assert!(w.is_empty(), "{j}");
        let c = &j["result"]["candidates"];
        assert_eq!(c[0]["device"], "0483:374b:M", "{j}");
        assert_eq!(c[0]["matched"], true, "{j}");
        assert_eq!(c[1]["device"], "0483:374b:A", "{j}");
        assert_eq!(c[1]["matched"], false, "{j}");
        assert_eq!(c.as_array().unwrap().len(), 3, "{j}");

        let (j, w) = run("named", &two, Some("0483:374b:B"), Some("yes"));
        assert_eq!(w, ["0483:374b:B"], "{j}");
        assert_eq!(j["result"]["device"], "0483:374b:B", "{j}");

        let (j, w) = run("unknown", &two, Some("--erase-all"), Some("yes"));
        assert!(w.is_empty(), "{j}");
        assert_eq!(j["state"], "failed", "{j}");
        let (j, w) = run("none", &[], None, Some("yes"));
        assert!(w.is_empty(), "{j}");
        assert_eq!(j["state"], "failed", "{j}");
    }
}
