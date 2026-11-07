//! `image sign`, `release build`, `topo compile`, `provision`, `rescue hub`.

use std::path::{Path, PathBuf};

use dagon_format::header::{image_kind, slot_binding, target_tag, HEADER_SIZE, MAGIC};
use dagon_format::{sha256, Version};
use dagon_sign::image::{self, ImageSpec};
use dagon_sign::provision;
use dagon_sign::release;
use dagon_sign::to_hex;
use dagon_topology::defs::{McuDef, Registry, Writer};
use dagon_topology::schema::{Role, Topology};
use ed25519_compact::KeyPair;

use crate::cli::{
    FirstInstallOpts, ImageSignArgs, ProvisionArgs, ReleaseBuildArgs, RescueArgs, TopoCompileArgs,
};
use crate::config::{load_key, read_file, read_topology, write_file, Config};

type Res = Result<(), String>;

/// Header fields chosen on the command line or in `dagon.toml`.
pub(crate) struct SignOpts {
    pub version: Version,
    pub release_seq: u32,
    pub min_bootcore: Version,
    pub proto: u32,
    pub bits: Option<(u32, u32)>,
}

pub(crate) fn version(s: &str) -> Result<Version, String> {
    image::parse_version(s).ok_or_else(|| format!("bad version '{s}' (want major.minor.patch)"))
}

fn number(name: &str, s: &str) -> Result<u32, String> {
    s.parse().map_err(|_| format!("--{name} must be a number"))
}

fn slot_arg(s: &str) -> Result<u8, String> {
    match s {
        "a" | "A" => Ok(slot_binding::A),
        "b" | "B" => Ok(slot_binding::B),
        _ => Err("--slot must be a or b".into()),
    }
}

/// Signs `input` (ELF or raw payload) for `node`, or, when `input` already is
/// a signed image, verifies it and checks it belongs to `node`/`kind`/`slot`.
/// Slot geometry comes from the MCU definitions of `reg`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn image_for_node(
    reg: &Registry,
    t: &Topology,
    node: &str,
    kind: u8,
    slot: u8,
    input: &[u8],
    opts: Option<&SignOpts>,
    key: &KeyPair,
) -> Result<Vec<u8>, String> {
    let n = t
        .node(node)
        .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?;
    let geometry = image::slot_with(reg, &n.mcu, kind, slot).ok_or_else(|| {
        format!(
            "node '{node}' ({}) has no such slot for this image kind",
            n.mcu
        )
    })?;
    if input.starts_with(&MAGIC) {
        let h = image::verify_image(input, &key.pk).map_err(|e| e.to_string())?;
        let ok = h.target_tag == target_tag(&n.tag)
            && h.image_kind == kind
            && h.slot_binding == geometry.slot_binding;
        return if ok {
            Ok(input.to_vec())
        } else {
            Err(format!(
                "signed image does not match node '{node}', kind and slot"
            ))
        };
    }
    let o = opts.ok_or("a version is needed to sign an unsigned input")?;
    let (flags, caps) = o.bits.unwrap_or_else(|| image::default_bits(kind));
    let spec = ImageSpec {
        image_kind: kind,
        port_kind: image::port_kind_of(&n.port).ok_or("unknown port kind")?,
        mcu_id: reg.mcu(&n.mcu).ok_or("unknown MCU")?.wire_id,
        slot: geometry,
        version: o.version,
        build_flags: flags,
        release_seq: o.release_seq,
        min_bootcore_version: o.min_bootcore,
        target_tag: target_tag(&n.tag),
        proto_version: o.proto,
        capabilities: caps,
    };
    let payload = image::payload_from_file(input, geometry.load_addr).map_err(|e| e.to_string())?;
    image::sign_image(&spec, &payload, key).map_err(|e| e.to_string())
}

fn describe(path: &Path, img: &[u8]) {
    let raw: [u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().expect("signed image");
    let h = dagon_format::ImageHeader::parse(&raw).expect("signed image");
    println!(
        "{}: {} bytes, version {}, header sha256 {}",
        path.display(),
        img.len(),
        h.version,
        to_hex(&dagon_format::header::header_digest(&raw))
    );
}

pub fn image_sign(a: &ImageSignArgs) -> Res {
    let cfg = Config::load(a.config.as_deref())?;
    let t = read_topology(&cfg, &cfg.topology_path(a.topology.as_deref())?)?;
    let kind = image::image_kind_of(&a.kind).ok_or("--kind must be app, bootcore or agent")?;
    let slot = slot_arg(&a.slot)?;
    let bits = match (a.flags.as_deref(), a.caps.as_deref()) {
        (None, None) => None,
        (f, c) => {
            let (df, dc) = image::default_bits(kind);
            let f = f.map_or(Some(df), |s| image::parse_bits(s, image::BUILD_FLAG_NAMES));
            let c = c.map_or(Some(dc), |s| image::parse_bits(s, image::CAPABILITY_NAMES));
            Some((
                f.ok_or("unknown name in --flags")?,
                c.ok_or("unknown name in --caps")?,
            ))
        }
    };
    let opts = SignOpts {
        version: version(&a.version)?,
        release_seq: number("seq", &a.seq)?,
        min_bootcore: a
            .min_bootcore
            .as_deref()
            .map_or(Ok(Version::default()), version)?,
        proto: number("proto", &a.proto)?,
        bits,
    };
    let out = PathBuf::from(&a.out);
    let key = load_key(a.key.as_deref())?;
    let input_bytes = read_file(Path::new(&a.input))?;
    let img = image_for_node(
        &crate::defs::registry(&cfg)?,
        &t,
        &a.node,
        kind,
        slot,
        &input_bytes,
        Some(&opts),
        &key,
    )?;
    write_file(&out, &img)?;
    describe(&out, &img);
    Ok(())
}

fn image_files(paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in paths.iter().map(PathBuf::from) {
        if p.is_dir() {
            let mut imgs: Vec<PathBuf> = std::fs::read_dir(&p)
                .map_err(|e| format!("cannot read {}: {e}", p.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.extension().is_some_and(|x| x == "img"))
                .collect();
            imgs.sort();
            out.extend(imgs);
        } else {
            out.push(p);
        }
    }
    Ok(out)
}

pub fn release_build(a: &ReleaseBuildArgs) -> Res {
    let cfg = Config::load(a.config.as_deref())?;
    let t = read_topology(&cfg, &cfg.topology_path(a.topology.as_deref())?)?;
    let reg = crate::defs::registry(&cfg)?;
    let topo_bin = match &a.topology_bin {
        Some(p) => read_file(Path::new(p))?,
        None => dagon_topology::compile_with(&reg, &t).map_err(|e| e.to_string())?,
    };
    let out = a
        .out
        .as_ref()
        .map(PathBuf::from)
        .or(cfg.release.clone())
        .ok_or("no output: pass --out <dgb> or set release in dagon.toml")?;
    let name = &a.name;
    let images = image_files(&a.inputs)?
        .iter()
        .map(|p| read_file(p))
        .collect::<Result<Vec<_>, _>>()?;
    let key = load_key(a.key.as_deref())?;
    let r = release::build(&reg, &t, &topo_bin, &images, name, &key).map_err(|e| e.to_string())?;
    write_file(&out, &r.bundle)?;
    println!(
        "{}: {} images, {} bytes, topology sha256 {}",
        out.display(),
        images.len(),
        r.bundle.len(),
        to_hex(&sha256(&topo_bin))
    );
    Ok(())
}

pub fn topo_compile(a: &TopoCompileArgs) -> Res {
    let path = a.yaml.as_str();
    // Without --config: the dagon.toml here, if any, else the builtin set.
    let cfg = Config::load(a.config.as_deref())?;
    let t = read_topology(&cfg, Path::new(path))?;
    let bin = dagon_topology::compile_with(&crate::defs::registry(&cfg)?, &t)
        .map_err(|e| e.to_string())?;
    let out = a
        .out
        .as_ref()
        .map_or_else(|| Path::new(path).with_extension("bin"), PathBuf::from);
    write_file(&out, &bin)?;
    println!(
        "{}: {} bytes, sha256 {}",
        out.display(),
        bin.len(),
        to_hex(&sha256(&bin))
    );
    Ok(())
}

pub fn provision(a: &ProvisionArgs) -> Res {
    provision_node(&a.files, &a.node)
}

/// `rescue hub`: the coordinator's first-install files.
pub fn rescue(a: &RescueArgs) -> Res {
    let o = &a.files;
    let cfg = Config::load(o.config.as_deref())?;
    let t = read_topology(&cfg, &cfg.topology_path(o.topology.as_deref())?)?;
    let hub = t
        .nodes
        .iter()
        .find(|n| n.role == Role::Coordinator)
        .ok_or("topology has no coordinator")?;
    let id = hub.id.clone();
    provision_node(o, &id)
}

fn provision_node(o: &FirstInstallOpts, node: &str) -> Res {
    let cfg = Config::load(o.config.as_deref())?;
    let t = read_topology(&cfg, &cfg.topology_path(o.topology.as_deref())?)?;
    if t.node(node).is_none() {
        return Err(format!("no updatable node '{node}' in the topology"));
    }
    let inputs = cfg
        .provision
        .get(node)
        .ok_or_else(|| format!("no [provision.{node}] in dagon.toml"))?;
    let dir = o
        .out
        .as_ref()
        .map_or_else(|| cfg.provision_dir().join(node), PathBuf::from);
    let key = load_key(o.key.as_deref())?;
    let reg = crate::defs::registry(&cfg)?;
    let w = provision_files(&reg, &t, node, inputs, &dir, &key, &cfg.base)?;
    let mcu = node_mcu(&reg, &t, node)?;
    let via = &mcu.connection.label;
    match (w, &mcu.writer) {
        (
            FirstWrite::ProbeRs {
                bin,
                bytes,
                commands,
            },
            _,
        ) => println!(
            "{}: {bytes} bytes\nwrite with {via}:\n{}",
            bin.display(),
            command_lines("probe-rs", &commands)
        ),
        (FirstWrite::Uf2 { file, blocks }, Writer::Uf2Drive { board_id, .. }) => println!(
            "{}: {blocks} blocks\ncopy the file to the {board_id} drive ({via})",
            file.display()
        ),
        _ => return Err("provision makes no files for this writer; use `dagon flash`".into()),
    }
    Ok(())
}

/// How the first-install files reach the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FirstWrite {
    /// Writer `probe-rs`: `flash.bin` from the flash base; `commands` are the
    /// probe-rs argument lists, in order, without the probe (chosen when
    /// writing).
    ProbeRs {
        bin: PathBuf,
        bytes: usize,
        commands: Vec<Vec<String>>,
    },
    /// Writer `uf2-drive`: `rescue.uf2` copied to the boot ROM's drive.
    Uf2 { file: PathBuf, blocks: usize },
    /// Writer `espflash`: `files` are (flash offset, file) and `commands`
    /// the espflash argument lists without the port (chosen when writing).
    Espflash {
        files: Vec<(u32, PathBuf)>,
        commands: Vec<Vec<String>>,
    },
}

/// One line per command, `program` then its arguments.
pub(crate) fn command_lines(program: &str, commands: &[Vec<String>]) -> String {
    commands
        .iter()
        .map(|args| format!("{program} {}\n", args.join(" ")))
        .collect()
}

/// probe-rs argument lists that erase `chip` and write `bin` from `base`
/// (the start of its flash), then reset it.
pub(crate) fn probe_rs_commands(chip: &str, base: u32, bin: &Path) -> Vec<Vec<String>> {
    let cmd = |verb: &str, rest: &[String]| {
        let mut c = vec![verb.to_string(), "--chip".into(), chip.to_string()];
        c.extend_from_slice(rest);
        c
    };
    let download = [
        "--binary-format".into(),
        "bin".into(),
        "--base-address".into(),
        format!("{base:#010x}"),
        bin.display().to_string(),
    ];
    vec![
        cmd("erase", &[]),
        cmd("download", &download),
        cmd("reset", &[]),
    ]
}

/// The MCU definition of `node`.
pub(crate) fn node_mcu<'r>(
    reg: &'r Registry,
    t: &Topology,
    node: &str,
) -> Result<&'r McuDef, String> {
    let n = t
        .node(node)
        .ok_or_else(|| format!("no updatable node '{node}' in the topology"))?;
    reg.mcu(&n.mcu)
        .ok_or_else(|| format!("no MCU definition '{}' ('{node}')", n.mcu))
}

/// stage0's bytes in a node's provision folder (an MCU with a stage0).
pub(crate) const STAGE0_FILE: &str = "stage0.bin";

/// Signs `inputs` for `node` and writes the first-install files into `dir`
/// (bootcore.img, app-a.img, stage0.bin where the MCU has a stage0, then
/// flash.bin and commands.txt or rescue.uf2).
pub(crate) fn provision_files(
    reg: &Registry,
    t: &Topology,
    node: &str,
    inputs: &crate::config::ProvisionInputs,
    dir: &Path,
    key: &KeyPair,
    base: &Path,
) -> Result<FirstWrite, String> {
    // Below the work area (the default provision folder) the files are
    // DAGON's outputs and go through genfs; elsewhere the user named the
    // folder (--out, provision_dir).
    let out = crate::genfs::Out::of(base, dir)?;
    let write_file = |p: &Path, b: &[u8]| {
        let name = p.file_name().ok_or("no file name")?.to_string_lossy();
        crate::genfs::write_out(base, &out.join(&name)?, b)
    };
    let mcu = node_mcu(reg, t, node)?;
    if let Writer::Espflash { .. } = mcu.writer {
        return Err(format!(
            "provision does not make {} files; use `dagon flash`",
            mcu.name
        ));
    }
    let app_v = inputs.version.as_deref().map(version).transpose()?;
    let boot_v = inputs
        .bootcore_version
        .as_deref()
        .map(version)
        .transpose()?
        .or(app_v);
    let sign_opts = |v: Option<Version>, min_bootcore| {
        v.map(|version| SignOpts {
            version,
            release_seq: inputs.release_seq.unwrap_or(1),
            min_bootcore,
            proto: inputs.proto_version.unwrap_or(1),
            bits: None,
        })
    };
    let boot = image_for_node(
        reg,
        t,
        node,
        image_kind::BOOTCORE,
        slot_binding::A,
        &read_file(&inputs.bootcore)?,
        sign_opts(boot_v, Version::default()).as_ref(),
        key,
    )?;
    let app = image_for_node(
        reg,
        t,
        node,
        image_kind::APP,
        slot_binding::A,
        &read_file(&inputs.app)?,
        sign_opts(app_v, boot_v.unwrap_or_default()).as_ref(),
        key,
    )?;
    let boot_slot =
        image::slot_with(reg, &mcu.id, image_kind::BOOTCORE, slot_binding::A).expect("checked");
    let app_slot =
        image::slot_with(reg, &mcu.id, image_kind::APP, slot_binding::A).expect("checked");
    let boot_path = dir.join("bootcore.img");
    let app_path = dir.join("app-a.img");
    write_file(&boot_path, &boot)?;
    write_file(&app_path, &app)?;
    describe(&boot_path, &boot);
    describe(&app_path, &app);

    let stage0 = stage0_payload(mcu, node, inputs)?;
    if let Some(s) = &stage0 {
        // The ROM path's full rewrite (`dagon deploy` of a silent node) writes it again.
        let p = dir.join(STAGE0_FILE);
        write_file(&p, s)?;
        println!("{}: {} bytes", p.display(), s.len());
    }
    let file = provision::first_install_file(
        mcu,
        stage0.as_deref(),
        (&boot_slot, &boot),
        (&app_slot, &app),
    )?;
    match &mcu.writer {
        Writer::ProbeRs { chip } => {
            let bin_path = dir.join("flash.bin");
            write_file(&bin_path, &file)?;
            let commands = probe_rs_commands(chip, mcu.default_flash_map().base, &bin_path);
            write_file(
                &dir.join("commands.txt"),
                command_lines("probe-rs", &commands).as_bytes(),
            )?;
            Ok(FirstWrite::ProbeRs {
                bin: bin_path,
                bytes: file.len(),
                commands,
            })
        }
        Writer::Uf2Drive { .. } => {
            let uf2_path = dir.join("rescue.uf2");
            write_file(&uf2_path, &file)?;
            Ok(FirstWrite::Uf2 {
                file: uf2_path,
                blocks: file.len() / 512,
            })
        }
        Writer::Espflash { .. } => unreachable!("refused above"),
    }
}

/// stage0's payload when `mcu`'s layout has a stage0 region (it must fit).
fn stage0_payload(
    mcu: &McuDef,
    node: &str,
    inputs: &crate::config::ProvisionInputs,
) -> Result<Option<Vec<u8>>, String> {
    let Some(region) = mcu.layout.stage0 else {
        return Ok(None);
    };
    let path = inputs
        .stage0
        .as_ref()
        .ok_or_else(|| format!("[provision.{node}] needs stage0 for an {}", mcu.id))?;
    let stage0 = image::payload_from_file(&read_file(path)?, region.at)
        .map_err(|e| format!("stage0: {e}"))?;
    if stage0.len() > region.size as usize {
        return Err(format!(
            "stage0 is {} bytes, the region holds {}",
            stage0.len(),
            region.size
        ));
    }
    Ok(Some(stage0))
}
