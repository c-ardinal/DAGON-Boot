//! `dagon flash` and `dagon doctor` as serve jobs.
//!
//! flash: the plan goes to the job's log, the parts and the app are built,
//! then the job asks the page; only the answer "yes" writes to the board
//! (anything else, a cancel or no answer writes nothing). The RP2040's
//! write waits for exactly one RPI-RP2 drive (flash refuses several) and
//! takes the hub from the page first.
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
type Write<'a> = dyn FnMut(&FirstWrite, &mut Job) -> Result<(), String> + 'a;

/// POST /v1/flash {node}.
pub fn job_flash(job: &mut Job, cfg: Config, v: Value) -> Result<Value, String> {
    flash_with(
        job,
        &cfg,
        &v,
        &mut |cfg, t, node, key, out, job| {
            let cancel = job.cancel_flag();
            crate::flash::prepare(cfg, t, node, key, out, &mut |l| job.log(l), &cancel)
        },
        &mut |w, job| {
            let write = |job: &mut Job| {
                let cancel = job.cancel_flag();
                crate::flash::write(w, &mut |l| job.log(l), &cancel)
            };
            match w {
                // The hub itself restarts into its boot ROM: the page lets
                // go of its port first.
                FirstWrite::Uf2 { .. } => job.with_device(write),
                FirstWrite::ProbeRs { .. } => write(job),
            }
        },
    )
}

fn flash_with(
    job: &mut Job,
    cfg: &Config,
    v: &Value,
    prepare: &mut Prepare<'_>,
    write: &mut Write<'_>,
) -> Result<Value, String> {
    let node = v["node"]
        .as_str()
        .filter(|n| plain(n))
        .ok_or("node is required")?;
    let t = read_topology(&inside(cfg, &cfg.topology_path(None)?)?)?;
    let out = inside(cfg, &cfg.provision_dir().join(node))?;
    let steps = crate::flash::plan(cfg, &t, node, &out)?;
    for l in &steps {
        job.log(l);
    }
    let key = serve_key(cfg)?;
    job.progress("building", None);
    let w = prepare(cfg, &t, node, &key, &out, job)?;
    let question = format!(
        "{}\nWrite to the board now? Answer yes to write; any other answer writes nothing.",
        steps.join("\n")
    );
    if !confirmed(&job.ask(ASK, &question)?) {
        return Err("nothing written to the board".into());
    }
    job.progress("writing", None);
    write(&w, job)?;
    Ok(json!({ "node": node, "written": true }))
}

/// POST /v1/doctor {}: the tools the project's MCU families need.
pub fn job_doctor(job: &mut Job, cfg: Config, _v: Value) -> Result<Value, String> {
    let mcus: Vec<String> = cfg
        .topology_path(None)
        .and_then(|p| inside(&cfg, &p))
        .and_then(|p| read_topology(&p))
        .map(|t| t.nodes.iter().map(|n| n.mcu.clone()).collect())
        .unwrap_or_else(|_| crate::doctor::FAMILIES.map(String::from).to_vec());
    let mut lines = Vec::new();
    let ok = crate::doctor::check(&mcus, &mut |l| {
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

    /// Runs flash_with on fakes, answers `answer` (None: cancels), and
    /// returns (final job JSON, writes made).
    fn run(name: &str, answer: Option<&str>) -> (Value, usize) {
        let (dir, cfg) = project(name);
        let server =
            super::super::jobs::Server::new("p".repeat(32), 1, None, std::path::PathBuf::new());
        let writes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w2 = writes.clone();
        let id = server
            .spawn("flash", move |job| {
                flash_with(
                    job,
                    &cfg,
                    &json!({ "node": "seg" }),
                    &mut |_, _, _, _, out, _| {
                        Ok(FirstWrite::ProbeRs {
                            bin: out.join("flash.bin"),
                            bytes: 1,
                            commands: vec![],
                        })
                    },
                    &mut |_, _| {
                        w2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    },
                )
            })
            .unwrap();
        let state = |s: &str| server.job_json(id).unwrap()["state"] == s;
        let t = std::time::Instant::now();
        while !state("question") && t.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state("question"), "{:?}", server.job_json(id));
        assert_eq!(
            writes.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "nothing before the answer"
        );
        match answer {
            Some(a) => assert!(server.answer(id, a.into())),
            None => assert!(server.cancel(id)),
        }
        let t = std::time::Instant::now();
        while (state("question") || state("running"))
            && t.elapsed() < std::time::Duration::from_secs(10)
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(&dir);
        (
            server.job_json(id).unwrap(),
            writes.load(std::sync::atomic::Ordering::SeqCst),
        )
    }

    #[test]
    fn the_board_is_written_only_after_the_page_answers_yes() {
        let (j, n) = run("yes", Some("yes"));
        assert_eq!((j["state"].as_str(), n), (Some("succeeded"), 1), "{j}");
        let (j, n) = run("no", Some("no"));
        assert_eq!((j["state"].as_str(), n), (Some("failed"), 0), "{j}");
        let (j, n) = run("cancel", None);
        assert_eq!(n, 0, "{j}");
        assert_ne!(j["state"], "succeeded");
    }
}
