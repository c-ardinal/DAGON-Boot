//! A project-defined MCU and board go through every project command with
//! no code change: topology check, compile, image sign, release build and
//! the first-write files.

use std::path::{Path, PathBuf};

use dagon_format::header::HEADER_SIZE;
use dagon_format::header::{image_kind, slot_binding};
use dagon_format::{BundleView, ImageHeader};

use crate::config::{read_topology, Config};

/// An MCU no DAGON source knows: probe-rs writer, A/B app slots.
const FAKE_MCU: &str = r#"
id = "fake-m0"
name = "FAKE-M0"
wire_id = 0x7F01
port = "self-boot"
gpio_count = 16

[[flash_map]]
name = "fake-1m"
base = 0x0010_0000
bytes = 0x10_0000

[layout]
app_log = { faces = [0x0010_8000, 0x0010_9000], face_size = 0x1000 }

[[slot]]
image = "bootcore"
binding = "a"
header = "trailer"
at = 0x0010_0000
size = 0x8000

[[slot]]
image = "app"
binding = "a"
header = "inline"
at = 0x0011_0000
size = 0x4_0000

[[slot]]
image = "app"
binding = "b"
header = "inline"
at = 0x0015_0000
size = 0x4_0000

[writer]
kind = "probe-rs"
chip = "FAKE0"

[[part]]
name = "bootcore"
sub = "sk-ant-FAKE-TOKEN-4242/bootcore"

[links]
up = ["i2c"]

[rom_i2c]
name = "Fake ROM loader"
addr = 0x3A
max_hz = 400_000
pairs = [{ controller = "i2c0", sda = "GPIO2", scl = "GPIO3" }]

[connection]
label = "SWD"
hint = "SWD probe"
"#;

const FAKE_BOARD: &str = r#"
[board.fake-board]
product = "Fake Board"
mcu = "fake-m0"
flash_bytes = 0x20_0000
i2c_target = true
drawing = "nucleo-64"
header = [{ pin = "GPIO2", label = "D2" }]
i2c = [{ controller = "i2c0", sda = "GPIO2", scl = "GPIO3" }]
default_link = { kind = "i2c", addr = 0x42, control = "full" }
"#;

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// The desk demo with its 7セグ node on the fake MCU and board, in a
/// project that defines both.
fn fake_project(dir: &Path) -> PathBuf {
    let yaml = include_str!("../../../../topology/desk-demo.yaml")
        .replace("mcu: stm32f401re", "mcu: fake-m0")
        .replace("board: nucleo-f401re", "board: fake-board")
        .replace("flash_map: f401-default", "flash_map: fake-1m")
        .replace("{ sda: PB4, scl: PA8 }", "{ sda: GPIO2, scl: GPIO3 }");
    write(&dir.join("t.yaml"), yaml.as_bytes());
    write(&dir.join("m/dagon-mcu.toml"), FAKE_MCU.as_bytes());
    // The MCU package's boot core part: a stub crate, built where the
    // package is (not looked up in a DAGON source).
    write(
        &dir.join("m/sk-ant-FAKE-TOKEN-4242/bootcore/Cargo.toml"),
        b"[package]\nname = \"fake-bootcore\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n",
    );
    write(
        &dir.join("m/sk-ant-FAKE-TOKEN-4242/bootcore/src/main.rs"),
        b"fn main() {}\n",
    );
    // The node's app: a stub crate in a folder whose names are private.
    write(
        &dir.join("private/token/app/Cargo.toml"),
        b"[package]\nname = \"fake-app\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n",
    );
    write(
        &dir.join("private/token/app/src/main.rs"),
        b"fn main() {}\n",
    );
    write(&dir.join("boot.bin"), &[0xB0; 1024]);
    write(&dir.join("app.bin"), &[0xA0; 2048]);
    let toml = dir.join("dagon.toml");
    write(
        &toml,
        format!(
            "topology = \"t.yaml\"\nmcu_files = [\"m/dagon-mcu.toml\"]\n\
             [build.seg]\npackage = \"private/token/app\"\n\
             [provision.seg]\nbootcore = \"boot.bin\"\napp = \"app.bin\"\nversion = \"1.0.0\"\n\
             {FAKE_BOARD}"
        )
        .as_bytes(),
    );
    toml
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

#[test]
fn a_project_defined_mcu_and_board_go_through_every_command() {
    let dir = std::env::temp_dir().join(format!("dagon-cli-fake-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let toml = fake_project(&dir);
    let key = dagon_sign::generate_to_dir(&dir.join("keys")).unwrap();
    let (cfg_arg, key_arg) = (s(&toml), s(&key.secret_path));
    let cfg = Config::load(Some(&cfg_arg)).unwrap();
    let topo = dir.join("t.yaml");

    // topology check: clean against the project's definitions, not the builtin set.
    let reg = super::registry(&cfg).unwrap();
    let yaml = std::fs::read_to_string(&topo).unwrap();
    let report = dagon_topology::check_str_with(&reg, &yaml);
    assert_eq!(report.errors().count(), 0, "{}", report.to_json());
    assert!(!dagon_topology::check_str(&yaml).is_pushable());
    read_topology(&cfg, &topo).unwrap();

    // compile
    let bin = dir.join("t.bin");
    crate::commands::topo_compile(&[&s(&topo), "--config", &cfg_arg, "--out", &s(&bin)]).unwrap();
    assert!(!std::fs::read(&bin).unwrap().is_empty());

    // image sign, both app slots of the fake node
    let imgs = dir.join("imgs");
    for (slot, want_at) in [("a", 0x0011_0200), ("b", 0x0015_0200)] {
        let out = imgs.join(format!("seg-{slot}.img"));
        crate::commands::image_sign(&[
            &s(&dir.join("app.bin")),
            "--node",
            "seg",
            "--slot",
            slot,
            "--version",
            "1.1.0",
            "--config",
            &cfg_arg,
            "--key",
            &key_arg,
            "--out",
            &s(&out),
        ])
        .unwrap();
        let img = std::fs::read(&out).unwrap();
        let h = ImageHeader::parse(&img[..HEADER_SIZE].try_into().unwrap()).unwrap();
        assert_eq!((h.mcu_id, h.load_addr), (0x7F01, want_at), "slot {slot}");
    }

    // release build
    let dgb = dir.join("r.dgb");
    crate::commands::release_build(&[
        &s(&imgs),
        "--name",
        "fake-1.1",
        "--config",
        &cfg_arg,
        "--key",
        &key_arg,
        "--out",
        &s(&dgb),
    ])
    .unwrap();
    let bundle = std::fs::read(&dgb).unwrap();
    let view = BundleView::parse(&bundle).unwrap();
    view.verify(&key.public_key).unwrap();
    assert!(
        view.item_count() >= 2,
        "both signed images and the topology"
    );

    // first-write files: the probe-rs binary from the fake flash base
    let out = dir.join("prov");
    crate::commands::provision(&[
        "seg",
        "--config",
        &cfg_arg,
        "--key",
        &key_arg,
        "--out",
        &s(&out),
    ])
    .unwrap();
    let flash = std::fs::read(out.join("flash.bin")).unwrap();
    let app = std::fs::read(out.join("app-a.img")).unwrap();
    // App A's header sits at 0x0011_0000, 0x1_0000 past the flash base.
    assert_eq!(&flash[0x1_0000..0x1_0000 + app.len()], &app[..]);
    let commands = std::fs::read_to_string(out.join("commands.txt")).unwrap();
    assert!(commands.contains("--chip FAKE0"), "{commands}");
    assert!(commands.contains("--base-address 0x00100000"), "{commands}");
    let boot = dagon_sign::image::slot_with(&reg, "fake-m0", image_kind::BOOTCORE, slot_binding::A);
    assert_eq!(boot.unwrap().header_addr, 0x0010_7E00);

    // flash: the plan names the part in the project package (relative for
    // the page), and the part builds there, with no DAGON source at all.
    let t = read_topology(&cfg, &topo).unwrap();
    let page = crate::flash::plan(&cfg, &t, "seg", &out, crate::flash::Paths::Relative)
        .unwrap()
        .join("\n");
    // A project's own folder names (here a token-like one) never reach the page.
    assert!(
        page.contains("build    bootcore dagon-mcu-fake-m0 (the project's own package) (tag"),
        "{page}"
    );
    assert!(!page.contains("sk-ant") && !page.contains("m/"), "{page}");
    assert!(page.contains("through the SWD chosen"), "{page}");
    assert!(!page.contains(&*dir.to_string_lossy()), "{page}");
    let mut log = Vec::new();
    let built = crate::flash::build_parts(
        &cfg,
        &reg,
        t.node("seg").unwrap(),
        Path::new("no-dagon-source"),
        "00",
        crate::flash::Paths::Relative,
        &mut |l| log.push(l.to_string()),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(built.len(), 1);
    assert_eq!(built[0].0, "bootcore");
    assert!(built[0].1.is_file(), "{:?}", built[0].1);
    // cargo's own lines included.
    assert!(!log.iter().any(|l| l.contains("sk-ant")), "{log:?}");
    assert!(
        log.iter()
            .any(|l| l.contains("Compiling fake-bootcore")
                && l.contains("(the project's own package)")),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l
                == "seg: building bootcore in dagon-mcu-fake-m0 (the project's own package)"),
        "{log:?}"
    );

    // The app: the page names its package, never its folder (the terminal does).
    assert!(
        page.contains("build    app      fake-app (the seg app)"),
        "{page}"
    );
    let private = |l: &str| l.contains("private") || l.contains("token");
    assert!(!private(&page), "{page}");
    let full = crate::flash::plan(&cfg, &t, "seg", &out, crate::flash::Paths::Full)
        .unwrap()
        .join("\n");
    assert!(
        full.contains("build    app      private/token/app"),
        "{full}"
    );
    let mut log = Vec::new();
    crate::flash::build_app(
        &cfg,
        "seg",
        [0; 32],
        None,
        crate::flash::Paths::Relative,
        &mut |l| log.push(l.to_string()),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert!(!log.iter().any(|l| private(l)), "{log:?}");
    assert!(
        !log.iter().any(|l| l.contains(&*dir.to_string_lossy())),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|l| l.contains("Compiling fake-app") && l.contains("(the seg app)")),
        "{log:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
