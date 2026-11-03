//! `dagon demo`: DAGON Console against a simulated desk, no boards needed.
//! The 親機 (RP2040) with its two 子機 (the F401 on I2C, the ESP32-S3 watch
//! on the UART), all on 1.0.0, as dagon-sim simulates them for `dagon sim
//! run`; the page drives them with the same dagon-host code as real
//! hardware (see serve::sim for the link and its timing).
//!
//! Everything is made fresh at start in a demo folder under the system's
//! temporary folder: a demo key pair (never the developer's key), the
//! desk-demo 構成, the 1.0.0 images the simulated boards run and release
//! demo-1.1.0, all signed with the demo key. `--sim-fail <node>` makes that
//! 子機's new version crash at boot, so its update falls back to 1.0.0.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dagon_format::header::{image_kind, target_tag};
use dagon_format::Version;
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sim::{demo_hub, watch_config, Layout, SimFlash, SimHub, SimLink, SimNode, TrialApp};
use ed25519_compact::KeyPair;

use crate::config::Opts;

const YAML: &str = include_str!("../../../topology/desk-demo.yaml");
/// Marks a folder this command made (and may make again).
const MARK: &str = ".dagon-demo";
const DEMO_DIR: &str = "dagon-demo";
const RELEASE: &str = "demo-1.1.0";
const OLD: Version = Version::new(1, 0, 0);
const NEW: Version = Version::new(1, 1, 0);
/// Image bodies: about what the desk-demo apps weigh, so a transfer takes
/// its real time.
const HUB_LEN: usize = 128 * 1024;
const SEG_LEN: usize = 48 * 1024;
const WATCH_LEN: usize = 48 * 1024;
const SPEEDS: std::ops::RangeInclusive<f64> = 0.1..=20.0;
/// The 子機 a failure can be set on.
const NODES: [&str; 2] = ["seg", "watch"];

/// A node of the desk: MCU, port kind, target tag.
struct Board {
    mcu: &'static str,
    port: &'static str,
    tag: &'static str,
}

const HUB: Board = Board {
    mcu: "rp2040",
    port: "self-boot",
    tag: "hub-rp2040-lcd",
};
const SEG: Board = Board {
    mcu: "stm32f401re",
    port: "self-boot",
    tag: "seg-f401-7seg",
};
const WATCH: Board = Board {
    mcu: "esp32s3",
    port: "vendor-stage2",
    tag: "watch-s3",
};

fn img(key: &KeyPair, b: &Board, slot: u8, v: Version, len: usize) -> Result<Vec<u8>, String> {
    let (flags, caps) = image::default_bits(image_kind::APP);
    let spec = ImageSpec {
        image_kind: image_kind::APP,
        port_kind: image::port_kind_of(b.port).ok_or("port kind")?,
        mcu_id: image::mcu_id_of(b.mcu).ok_or("mcu")?,
        slot: image::slot(b.mcu, image_kind::APP, slot).ok_or("slot")?,
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        target_tag: target_tag(b.tag),
        proto_version: 1,
        capabilities: caps,
    };
    let salt = b.tag.len() as u8 ^ v.minor() ^ (slot << 4);
    let body: Vec<u8> = (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ salt)
        .collect();
    sign_image(&spec, &body, key).map_err(|e| format!("{e:?}"))
}

/// The simulated desk on 1.0.0, verifying under `key`; `fail`'s next
/// version crashes at boot.
pub fn desk(key: &KeyPair, fail: Option<&str>, seed: u64) -> Result<SimHub, String> {
    let pk = *key.pk;
    let mut h = demo_hub(pk, seed);
    h.node
        .provision(&img(key, &HUB, 0, OLD, HUB_LEN)?, 0)
        .map_err(|e| format!("hub: {e:?}"))?;
    let seg = h
        .i2c_node(dagon_sim::NODE_I2C_ADDR)
        .ok_or("the desk has no seg")?;
    seg.provision(&img(key, &SEG, 0, OLD, SEG_LEN)?, 0)
        .map_err(|e| format!("seg: {e:?}"))?;
    if fail == Some("seg") {
        seg.trial_app = TrialApp::CrashesAtBoot;
    }
    let mut watch = SimNode::new(
        watch_config(pk),
        SimFlash::esp32s3_4m(seed ^ 2),
        Layout::ESP32S3_WATCH,
    );
    let w = img(key, &WATCH, 0, OLD, WATCH_LEN)?;
    watch
        .store()
        .provision_factory(&w)
        .map_err(|e| format!("watch: {e:?}"))?;
    watch
        .provision(&w, 0)
        .map_err(|e| format!("watch: {e:?}"))?;
    if fail == Some("watch") {
        watch.trial_app = TrialApp::CrashesAtBoot;
    }
    h.set_uart(SimLink::new(seed ^ 3), watch);
    Ok(h)
}

/// Release demo-1.1.0 of the desk-demo 構成, signed with `key`.
fn release(key: &KeyPair) -> Result<Vec<u8>, String> {
    let t = dagon_topology::parse_str(YAML).map_err(|e| e.to_string())?;
    let bin = dagon_topology::compile_str(YAML).map_err(|e| e.to_string())?;
    let images = [
        img(key, &SEG, 0, NEW, SEG_LEN)?,
        img(key, &SEG, 1, NEW, SEG_LEN)?,
        img(key, &WATCH, 0, NEW, WATCH_LEN)?,
        img(key, &HUB, 0, NEW, HUB_LEN)?,
        img(key, &HUB, 1, NEW, HUB_LEN)?,
    ];
    let r =
        dagon_sign::release::build(&t, &bin, &images, RELEASE, key).map_err(|e| e.to_string())?;
    Ok(r.bundle)
}

/// Makes the demo folder `dir` afresh (only one this command made before is
/// replaced): its 構成, a demo key pair, dagon.toml and the release.
/// Returns dagon.toml's path and the demo key.
pub fn project(dir: &Path) -> Result<(PathBuf, KeyPair), String> {
    if dir.exists() {
        if !dir.join(MARK).is_file() {
            return Err(format!(
                "{} exists and was not made by dagon demo; remove it or move it away",
                dir.display()
            ));
        }
        std::fs::remove_dir_all(dir).map_err(|e| format!("cannot clear {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let write = |rel: &str, bytes: &[u8]| crate::config::write_file(&dir.join(rel), bytes);
    write(MARK, b"made by dagon demo; replaced on its next start\n")?;
    let k = dagon_sign::generate_to_dir(&dir.join("demo-key")).map_err(|e| e.to_string())?;
    let key = dagon_sign::load_key_pair(&k.secret_path).map_err(|e| e.to_string())?;
    write("topology.yaml", YAML.as_bytes())?;
    write(
        crate::config::CONFIG_FILE,
        b"# The dagon demo project (simulated desk). Its key is a DEMO key made\n\
          # for this demo only; nothing it signs runs on real boards.\n\
          topology = \"topology.yaml\"\n\
          pubkey = \"demo-key/dagon-signing.pub\"\n\
          signing_key = \"demo-key/dagon-signing.key\"\n",
    )?;
    write(&format!("out/release/{RELEASE}.dgb"), &release(&key)?)?;
    Ok((dir.join(crate::config::CONFIG_FILE), key))
}

/// The demo's folder name, one per user: a temporary folder may be shared
/// (Linux /tmp), and user names that differ only in punctuation or
/// non-ASCII letters must not share one, so it carries a hash of the name.
fn demo_dir_name(user: &str) -> String {
    use sha1::{Digest, Sha1};
    let hex: String = Sha1::digest(user.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{DEMO_DIR}-{hex}")
}

/// `dagon demo [--sim-speed <x>] [--sim-fail seg|watch] [--port <n>] [--no-open]`
pub fn command(args: &[&str]) -> Result<(), String> {
    let no_open = args.contains(&"--no-open");
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--no-open").collect();
    let o = Opts::parse(&rest, &["sim-speed", "sim-fail", "port"])?;
    if !o.pos.is_empty() {
        return Err("demo takes options only".into());
    }
    let speed: f64 = o.get("sim-speed").map_or(Ok(1.0), |s| {
        s.parse().map_err(|_| "--sim-speed must be a number")
    })?;
    if !SPEEDS.contains(&speed) {
        return Err(format!(
            "--sim-speed must be {} to {}",
            SPEEDS.start(),
            SPEEDS.end()
        ));
    }
    let fail = o.get("sim-fail");
    if let Some(f) = fail.filter(|f| !NODES.contains(f)) {
        return Err(format!(
            "--sim-fail takes a 子機: {} (not {f})",
            NODES.join(" or ")
        ));
    }
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(demo_dir_name(&user));
    let (config, key) = project(&dir)?;
    let hub = desk(&key, fail, 1)?;
    let mut sim = crate::serve::SimDevice::new(hub, speed, fail.map(String::from));
    sim.notes = vec![
        format!(
            "demo: a SIMULATED desk (親機 + 2 子機, desk-demo), speed x{speed}{}",
            fail.map_or(String::new(), |f| format!(", {f}'s update set to fail"))
        ),
        format!(
            "demo: DEMO key {} in {} (made for this demo; not your key)",
            crate::config::key_id_text(&key.pk),
            dir.join("demo-key").display()
        ),
    ];
    let sim = Arc::new(sim);
    let config = config.to_string_lossy().into_owned();
    let mut serve_args = vec!["--config", config.as_str()];
    if let Some(p) = o.get("port") {
        serve_args.extend(["--port", p]);
    }
    if no_open {
        serve_args.push("--no-open");
    }
    crate::serve::run(&serve_args, Some(sim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_user_name_has_its_own_demo_folder() {
        let names = ["ab", "a.b", "a b", "山田", "田中", ""].map(demo_dir_name);
        let distinct: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(distinct.len(), names.len());
        assert_eq!(demo_dir_name("ab"), demo_dir_name("ab"));
        assert!(names
            .iter()
            .all(|n| n.starts_with("dagon-demo-") && n.is_ascii()));
    }

    #[test]
    fn the_demo_project_is_made_afresh_with_its_own_key() {
        let dir = std::env::temp_dir().join(format!("dagon-demo-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (config, key) = project(&dir).unwrap();
        let cfg = crate::config::Config::load(Some(&config.to_string_lossy())).unwrap();
        // The release verifies under the demo key, and the key checks out
        // as the project's (its pubkey and signing key belong together).
        let bundle = std::fs::read(dir.join(format!("out/release/{RELEASE}.dgb"))).unwrap();
        dagon_format::bundle::BundleView::parse(&bundle)
            .unwrap()
            .verify(&key.pk)
            .unwrap();
        let pubkey = cfg.pubkey.clone().unwrap();
        assert_eq!(
            crate::config::checked_pubkey(&cfg, &pubkey).unwrap(),
            *key.pk
        );
        assert_ne!(*key.pk, dagon_sign::image::TEST_PUBLIC_KEY);
        // Made again: a new key; a folder it did not make is left alone.
        let (_, again) = project(&dir).unwrap();
        assert_ne!(*again.pk, *key.pk);
        std::fs::remove_file(dir.join(MARK)).unwrap();
        assert!(project(&dir)
            .unwrap_err()
            .contains("not made by dagon demo"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use dagon_host::{Host, Outcome};
    use std::time::{Duration, Instant};

    /// dagon-host's link to the demo desk, as the page has it through serve.
    struct Link(Arc<crate::serve::SimDevice>, Vec<u8>);

    impl dagon_host::Transport for Link {
        type Error = ();
        async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
            self.0.write(bytes);
            Ok(())
        }
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            if self.1.is_empty() {
                self.1 = self.0.read(Duration::from_millis(2));
            }
            let n = buf.len().min(self.1.len());
            buf[..n].copy_from_slice(&self.1[..n]);
            self.1.drain(..n);
            Ok(n)
        }
        async fn reopen(&mut self) -> Result<(), ()> {
            Ok(())
        }
    }

    /// The page's clock at the demo's speed.
    struct Clock(Instant, f64);

    impl dagon_host::Clock for Clock {
        fn now_ms(&self) -> u64 {
            (self.0.elapsed().as_secs_f64() * 1_000.0 * self.1) as u64
        }
        async fn sleep_ms(&mut self, ms: u32) {
            std::thread::sleep(Duration::from_secs_f64(ms as f64 / 1_000.0 / self.1));
        }
    }

    /// Release demo-1.1.0 applied to the whole desk through the demo link.
    fn update(name: &str, fail: Option<&str>) -> Vec<(u8, String)> {
        const SPEED: f64 = 50.0;
        let dir = std::env::temp_dir().join(format!("dagon-demo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (_, key) = project(&dir).unwrap();
        let bundle = std::fs::read(dir.join(format!("out/release/{RELEASE}.dgb"))).unwrap();
        let topo = dagon_topology::compile_str(YAML).unwrap();
        let dev = Arc::new(crate::serve::SimDevice::new(
            desk(&key, fail, 1).unwrap(),
            SPEED,
            None,
        ));
        let mut h = Host::new(Link(dev, Vec::new()), Clock(Instant::now(), SPEED));
        let out = dagon_sim::block_on(h.apply(&bundle, &topo, &key.pk, None, &mut |_| {})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        out.into_iter()
            .map(|o| {
                let what = match o.outcome {
                    Outcome::Done(v) => format!("done {v}"),
                    Outcome::Failed(e) => format!("failed {e:?}"),
                    Outcome::NotStarted => "not started".into(),
                };
                (o.node, what)
            })
            .collect()
    }

    #[test]
    fn the_whole_desk_updates_through_the_demo_link() {
        let out = update("all", None);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out.iter().all(|(_, w)| w == "done 1.1.0"), "{out:?}");
    }

    #[test]
    fn a_failing_node_falls_back_to_its_old_version() {
        let out = update("fail", Some("watch"));
        let watch = out
            .iter()
            .find(|(_, w)| w.starts_with("failed"))
            .expect("one fails");
        let running = format!("running: {OLD:?}, trial: false");
        assert!(watch.1.contains(&running), "back on 1.0.0: {out:?}");
        assert!(out.iter().any(|(_, w)| w == "done 1.1.0"), "{out:?}");
    }

    #[test]
    fn options_are_checked() {
        for bad in [
            &["--sim-speed", "0"][..],
            &["--sim-speed", "fast"],
            &["--sim-fail", "hub"],
            &["extra"],
        ] {
            assert!(command(bad).is_err(), "{bad:?}");
        }
    }
}
