//! `dagon.toml`, option parsing and signing-key loading shared by commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ed25519_compact::KeyPair;
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "dagon.toml";
/// Environment variable holding the secret key file path.
pub const KEY_ENV: &str = "DAGON_SIGNING_KEY";
const DEFAULT_PROVISION_DIR: &str = "out/provision";

/// Inputs for `provision <node>`: ELF/raw binaries (signed here) or signed
/// `.img` files (checked and used as is).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionInputs {
    /// RP2040 only: boot2 + stage0, from the start of flash.
    pub stage0: Option<PathBuf>,
    pub bootcore: PathBuf,
    pub app: PathBuf,
    /// App version, needed when `app` is not a signed image.
    pub version: Option<String>,
    /// Defaults to `version`.
    pub bootcore_version: Option<String>,
    pub release_seq: Option<u32>,
    pub proto_version: Option<u32>,
}

/// How `dagon build` builds one node's application: a cargo package built
/// for slot A and, when `slot_b_feature` is set, again for slot B.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildInputs {
    /// Directory of the node's cargo package, relative to dagon.toml.
    pub package: PathBuf,
    /// Binary target; defaults to the package name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Feature that links the application for slot B.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_b_feature: Option<String>,
    /// Version the built application is signed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl BuildInputs {
    /// Checks the values a browser may write: a relative package path that
    /// stays inside the work area, plain names, a valid version.
    pub fn validate(&self) -> Result<(), String> {
        let inside = self.package.is_relative()
            && self
                .package
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        if !inside || self.package.as_os_str().is_empty() {
            return Err("package must be a relative path inside the work area".into());
        }
        self.validate_names()
    }

    /// Plain names (bin, features) and a valid version.
    pub fn validate_names(&self) -> Result<(), String> {
        let plain = |s: &str| {
            !s.is_empty()
                && !s.starts_with('-')
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        };
        let names = self
            .bin
            .iter()
            .chain(&self.features)
            .chain(&self.slot_b_feature);
        if let Some(bad) = names.into_iter().find(|n| !plain(n)) {
            return Err(format!("'{bad}' is not a plain name"));
        }
        if let Some(v) = &self.version {
            crate::commands::version(v)?;
        }
        Ok(())
    }
}

/// Unknown tables are ignored here.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Directory of the dagon.toml read (the work area); empty when none.
    #[serde(skip)]
    pub base: PathBuf,
    pub topology: Option<PathBuf>,
    pub release: Option<PathBuf>,
    pub provision_dir: Option<PathBuf>,
    /// Public key file `apply` verifies releases with.
    pub pubkey: Option<PathBuf>,
    /// Secret key file `dagon serve` signs this project's images with.
    pub signing_key: Option<PathBuf>,
    /// Folders besides the work area that `dagon serve` takes key files
    /// from (the key folders); filled by serve, empty elsewhere.
    #[serde(skip)]
    pub key_roots: Vec<PathBuf>,
    #[serde(default)]
    pub provision: BTreeMap<String, ProvisionInputs>,
    /// `dagon build` settings per node id; `package` stays relative here.
    #[serde(default)]
    pub build: BTreeMap<String, BuildInputs>,
    /// Where DAGON comes from (`[dagon]`: git + tag, or path relative to
    /// the work area); flash builds the boot images from it.
    #[serde(default)]
    pub dagon: crate::scaffold::DagonSource,
}

impl Config {
    /// `path`, else `dagon.toml` in the current directory if present. Paths
    /// inside are made relative to the file's directory.
    pub fn load(path: Option<&str>) -> Result<Config, String> {
        let path = match path {
            Some(p) => PathBuf::from(p),
            None if Path::new(CONFIG_FILE).exists() => PathBuf::from(CONFIG_FILE),
            None => {
                return Ok(Config {
                    base: base_of(Path::new(CONFIG_FILE)),
                    ..Config::default()
                })
            }
        };
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut c: Config =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = base_of(&path);
        c.base = dir.clone();
        let rel = |p: &mut PathBuf| *p = dir.join(&*p);
        c.topology.as_mut().map(rel);
        // Output folders and files: `..` taken lexically, so a path outside
        // the work area reads as outside (genfs::Out::of), never as a
        // work-area path with `..` in it.
        let out = |p: &mut PathBuf| *p = lexical(&dir.join(&*p));
        c.release.as_mut().map(out);
        c.provision_dir.as_mut().map(out);
        c.pubkey.as_mut().map(rel);
        c.signing_key.as_mut().map(rel);
        for inputs in c.provision.values_mut() {
            inputs.stage0.as_mut().map(rel);
            rel(&mut inputs.bootcore);
            rel(&mut inputs.app);
        }
        Ok(c)
    }

    pub fn topology_path(&self, arg: Option<&str>) -> Result<PathBuf, String> {
        arg.map(PathBuf::from)
            .or_else(|| self.topology.clone())
            .ok_or_else(|| {
                format!("no topology: pass --topology <yaml> or set it in {CONFIG_FILE}")
            })
    }

    /// Where `dagon provision` writes and recovery reads: `provision_dir`,
    /// else out/provision beside dagon.toml (like every other path in it).
    pub fn provision_dir(&self) -> PathBuf {
        self.provision_dir
            .clone()
            .unwrap_or_else(|| self.base.join(DEFAULT_PROVISION_DIR))
    }
}

/// `p` with `.` dropped and each `..` taking away the name before it,
/// without looking at the file system (links are not followed).
pub(crate) fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The work area of config file `path`: its directory as an absolute path,
/// so paths joined to it never depend on (or repeat) the current
/// directory. `dagon.toml` alone gives the current directory.
pub fn base_of(path: &Path) -> PathBuf {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    match std::fs::canonicalize(dir) {
        Ok(abs) => plain_path(abs),
        Err(_) => std::env::current_dir().map_or(dir.to_path_buf(), |c| c.join(dir)),
    }
}

/// Windows' extended-length path prefix, as `canonicalize` returns it.
const VERBATIM: &str = concat!('\\', '\\', '?', '\\');

/// Drops the extended-length prefix canonicalising adds on Windows, so the
/// path prints and passes to other tools as usual: `\\?\C:\x` becomes
/// `C:\x` and `\\?\UNC\server\share\x` becomes `\\server\share\x`.
/// Other verbatim paths (volume GUIDs, devices) stay as they are.
pub(crate) fn plain_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    let Some(rest) = s.strip_prefix(VERBATIM) else {
        return p;
    };
    if rest.as_bytes().get(1) == Some(&b':') {
        return PathBuf::from(rest);
    }
    let unc = rest
        .get(..4)
        .filter(|h| h.eq_ignore_ascii_case(concat!("UNC", '\\')))
        .map(|_| &rest[4..]);
    match unc {
        Some(share) if !share.is_empty() => {
            PathBuf::from(format!("{}{share}", concat!('\\', '\\')))
        }
        _ => p,
    }
}

/// `p` without the extended-length prefix, with forward slashes (for
/// TOML, the page and messages): `C:/x`, `//server/share/x`.
pub(crate) fn forward_path(p: &Path) -> String {
    plain_path(p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

/// `--name value` options after positional arguments. Every option takes a
/// value; options not in `allowed` are rejected.
pub struct Opts<'a> {
    pub pos: Vec<&'a str>,
    opts: Vec<(&'a str, &'a str)>,
}

impl<'a> Opts<'a> {
    pub fn parse(args: &[&'a str], allowed: &[&str]) -> Result<Self, String> {
        let mut out = Opts {
            pos: Vec::new(),
            opts: Vec::new(),
        };
        let mut it = args.iter();
        while let Some(&a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                if !allowed.contains(&name) {
                    return Err(format!("unknown option --{name}"));
                }
                let v = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
                out.opts.push((name, v));
            } else {
                out.pos.push(a);
            }
        }
        Ok(out)
    }

    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.opts
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
    }

    /// Every value of a repeatable option, in order.
    pub fn all<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'a str> + 's {
        self.opts
            .iter()
            .filter(move |(n, _)| *n == name)
            .map(|(_, v)| *v)
    }

    pub fn need(&self, name: &str) -> Result<&'a str, String> {
        self.get(name)
            .ok_or_else(|| format!("--{name} is required"))
    }
}

/// The topology at `path`, checked against `cfg`'s project definitions:
/// its updatable part, the one every update, deploy, flash and project
/// command works on. The nodes it leaves out are named on stderr with why.
pub fn read_topology(cfg: &Config, path: &Path) -> Result<dagon_topology::Topology, String> {
    let yaml = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let t = dagon_topology::parse_str(&yaml).map_err(|e| format!("{}: {e}", path.display()))?;
    let reg = crate::defs::registry(cfg)?;
    if !dagon_topology::check_with(&reg, &t).is_pushable() {
        return Err(format!(
            "{}: topology has errors; run `dagon topo check`",
            path.display()
        ));
    }
    let cut = dagon_topology::cut_with(&reg, &t);
    for line in skipped_lines(&cut) {
        eprintln!("{line}");
    }
    cut.topology.ok_or_else(|| {
        let first = cut.report.errors().next().map_or(String::new(), |i| {
            format!(": {} {}: {}", i.rule, i.path, i.message)
        });
        format!(
            "{}: no part of this topology can be updated yet; the part updating would reach does not pass the checks on its own{first}",
            path.display()
        )
    })
}

/// One line per node updating skips for now, with why (none for a 構成
/// updating reaches whole).
pub fn skipped_lines(cut: &dagon_topology::Cut) -> Vec<String> {
    cut.skipped
        .iter()
        .map(|(id, why)| {
            format!(
                "note: '{id}' is not updatable yet and is skipped: {}",
                why.reason()
            )
        })
        .collect()
}

/// Loads the signing key from `--key` or `DAGON_SIGNING_KEY`. Refuses key
/// files inside a git work tree. Error messages carry the path only.
pub fn load_key(arg: Option<&str>) -> Result<KeyPair, String> {
    let path = arg
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(KEY_ENV).map(PathBuf::from))
        .ok_or_else(|| format!("no signing key: pass --key <file> or set {KEY_ENV}"))?;
    let inside = dagon_sign::inside_git_work_tree(&path)
        .map_err(|e| format!("cannot check key path {}: {e}", path.display()))?;
    if inside {
        return Err(format!(
            "refusing to use a secret key stored inside a git work tree: {}",
            path.display()
        ));
    }
    let key = dagon_sign::load_key_pair(&path).map_err(|e| e.to_string())?;
    allow_key(&key.pk, test_key_allowed())?;
    Ok(key)
}

/// The project's signing key file: `signing_key` in dagon.toml, else the
/// secret beside its pubkey (where dagon new puts the pair), else
/// DAGON_SIGNING_KEY.
pub fn signing_key_path(cfg: &Config) -> Option<PathBuf> {
    cfg.signing_key
        .clone()
        .or_else(|| {
            let dir = cfg.pubkey.as_ref()?.parent()?;
            Some(dir.join(dagon_sign::SECRET_FILE)).filter(|p| p.is_file())
        })
        .or_else(|| std::env::var_os(KEY_ENV).map(PathBuf::from))
}

/// Every secret key file the project or the environment names.
fn known_secrets(cfg: &Config) -> Vec<PathBuf> {
    let beside = cfg
        .pubkey
        .as_ref()
        .and_then(|p| p.parent())
        .map(|d| d.join(dagon_sign::SECRET_FILE));
    let env = std::env::var_os(KEY_ENV).map(PathBuf::from);
    [cfg.signing_key.clone(), beside, env]
        .into_iter()
        .flatten()
        .filter(|p| p.is_file())
        .collect()
}

/// The project's public key from `path` (its pubkey file), for the page
/// and the firmware: a valid Ed25519 point, never a secret seed this
/// process knows of (whatever the file is called), and, with a signing key
/// known, its public half. Messages carry no paths.
pub fn checked_pubkey(cfg: &Config, path: &Path) -> Result<[u8; 32], String> {
    // Read as raw bytes: whether it is a secret seed is decided before
    // whether it is a valid point, so a secret always reports as one.
    // dagon_sign::load_public_key adds nothing more than the point check
    // check_pubkey_bytes makes.
    let text = std::fs::read_to_string(path).map_err(|_| "the pubkey file cannot be read")?;
    let k = key_hex(text.trim()).ok_or("the pubkey file is not a public key")?;
    check_pubkey_bytes(cfg, k)
}

/// [`checked_pubkey`] for key bytes already read: not a secret seed this
/// process knows of, a valid point, and the signing key's public half.
pub fn check_pubkey_bytes(cfg: &Config, k: [u8; 32]) -> Result<[u8; 32], String> {
    if known_secrets(cfg)
        .iter()
        .filter_map(|s| dagon_sign::load_key_pair(s).ok())
        .any(|kp| kp.sk.seed().as_ref() == k.as_slice())
    {
        return Err("the pubkey file holds a secret key".into());
    }
    ed25519_compact::PublicKey::new(k)
        .validate()
        .map_err(|_| "the pubkey is not a valid public key")?;
    if let Some(s) = signing_key_path(cfg) {
        let kp = dagon_sign::load_key_pair(&s).map_err(|_| "the signing key cannot be read")?;
        if *kp.pk != k {
            return Err("pubkey is not the public half of the project's signing key".into());
        }
    }
    allow_key(&k, test_key_allowed())?;
    Ok(k)
}

/// The key a firmware build embeds, checked: pubkey in dagon.toml, else
/// DAGON_PUBKEY from the environment (a key file or its 64 hex digits);
/// None when neither is set (the build then follows its test key rule).
pub fn build_pubkey(cfg: &Config) -> Result<Option<[u8; 32]>, String> {
    if let Some(p) = &cfg.pubkey {
        return checked_pubkey(cfg, p)
            .map(Some)
            .map_err(|e| format!("pubkey in dagon.toml: {e}"));
    }
    let Some(v) = std::env::var_os(crate::nodebuild::PUBKEY_ENV) else {
        return Ok(None);
    };
    let v = v.to_string_lossy().trim().to_string();
    let k = match key_hex(&v) {
        Some(k) => check_pubkey_bytes(cfg, k),
        None => checked_pubkey(cfg, Path::new(&v)),
    };
    k.map(Some)
        .map_err(|e| format!("{}: {e}", crate::nodebuild::PUBKEY_ENV))
}

/// 64 hex digits as 32 bytes.
fn key_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(k)
}

/// The key id of `public` as printed, marked when it is the published
/// RFC 8032 test key (anyone can sign for it).
pub fn key_id_text(public: &[u8; 32]) -> String {
    let id = dagon_sign::to_hex(&dagon_format::key_id(public));
    if *public == dagon_sign::image::TEST_PUBLIC_KEY {
        format!("{id} (TEST KEY: the published RFC 8032 key; anyone can sign for it)")
    } else {
        id
    }
}

/// Environment variable that permits the public RFC 8032 test key.
pub const ALLOW_TEST_KEY_ENV: &str = "DAGON_ALLOW_TEST_KEY";

pub fn test_key_allowed() -> bool {
    std::env::var(ALLOW_TEST_KEY_ENV).is_ok_and(|v| v == "1")
}

/// Refuses the public test key (its secret is published) unless `allowed`
/// (`DAGON_ALLOW_TEST_KEY=1`).
pub fn allow_key(public: &[u8; 32], allowed: bool) -> Result<(), String> {
    if *public == dagon_sign::image::TEST_PUBLIC_KEY && !allowed {
        return Err(format!(
            "this is the public RFC 8032 test key, whose secret is published; \
             set {ALLOW_TEST_KEY_ENV}=1 to use it for tests"
        ));
    }
    Ok(())
}

/// Writes a file the user named (a command's `--out`, the file next to an
/// input it names); never through a link at that path. Outputs dagon keeps
/// in the work area use genfs with a work-area path instead.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::genfs::write_user(path, bytes)
}

pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn output_folders_outside_the_work_area_read_as_outside() {
        let root = std::env::temp_dir().join(format!("dagon-cfg-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proj")).unwrap();
        std::fs::write(
            root.join("proj/dagon.toml"),
            "provision_dir = \"../shared/provision\"\nrelease = \"./out/../rel.dgb\"\n",
        )
        .unwrap();
        let c = Config::load(Some(&root.join("proj/dagon.toml").to_string_lossy())).unwrap();
        let p = c.provision_dir();
        assert!(
            !p.components().any(|x| x == std::path::Component::ParentDir),
            "{}",
            p.display()
        );
        assert_eq!(p, lexical(&c.base.join("..").join("shared/provision")));
        assert!(matches!(
            crate::genfs::Out::of(&c.base, &p.join("seg")).unwrap(),
            crate::genfs::Out::User(_)
        ));
        assert_eq!(c.release.unwrap(), c.base.join("rel.dgb"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_test_key_is_marked_wherever_its_id_is_printed() {
        let test = dagon_sign::image::TEST_PUBLIC_KEY;
        assert!(key_id_text(&test).contains("TEST KEY"));
        let other = *ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([3; 32])).pk;
        let t = key_id_text(&other);
        assert_eq!(t.len(), 16, "{t}");
    }

    #[test]
    fn verbatim_prefixes_are_dropped_for_drives_and_unc_shares() {
        let b = char::from(92).to_string();
        let w = |parts: &[&str]| PathBuf::from(parts.join(&b));
        let v = |rest: &[&str]| {
            let mut all = vec!["", "", "?"];
            all.extend_from_slice(rest);
            w(&all)
        };
        assert_eq!(
            plain_path(v(&["C:", "work", "proj"])),
            w(&["C:", "work", "proj"])
        );
        assert_eq!(
            plain_path(v(&["UNC", "server", "share", "proj"])),
            w(&["", "", "server", "share", "proj"])
        );
        assert_eq!(
            plain_path(v(&["unc", "server", "share"])),
            w(&["", "", "server", "share"])
        );
        assert_eq!(
            forward_path(&v(&["UNC", "srv", "share", "p"])),
            "//srv/share/p"
        );
        assert_eq!(forward_path(&v(&["C:", "a", "b"])), "C:/a/b");
        for keep in [
            v(&["Volume{0b1c}", "x"]),
            v(&["UNC"]),
            w(&["", "", "server", "share", "x"]),
            w(&["C:", "x"]),
            PathBuf::from("relative/x"),
        ] {
            assert_eq!(plain_path(keep.clone()), keep);
        }
    }

    #[test]
    fn the_provision_folder_is_beside_dagon_toml_by_default() {
        let dir = std::env::temp_dir().join(format!("dagon-provdir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let toml = dir.join("dagon.toml");
        std::fs::write(
            &toml,
            "topology = \"t.yaml\"
",
        )
        .unwrap();
        let c = Config::load(Some(&toml.to_string_lossy())).unwrap();
        assert_eq!(c.provision_dir(), c.base.join("out/provision"));
        assert!(c.provision_dir().is_absolute());
        std::fs::write(
            &toml,
            "provision_dir = \"p\"
",
        )
        .unwrap();
        let c = Config::load(Some(&toml.to_string_lossy())).unwrap();
        assert_eq!(c.provision_dir(), c.base.join("p"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opts_split_positional_and_named() {
        let o = Opts::parse(&["a", "--out", "x", "b"], &["out"]).unwrap();
        assert_eq!(o.pos, ["a", "b"]);
        assert_eq!(o.get("out"), Some("x"));
        assert!(Opts::parse(&["--bad", "x"], &["out"]).is_err());
        assert!(Opts::parse(&["--out"], &["out"]).is_err());
    }

    #[test]
    fn test_key_needs_explicit_opt_in() {
        let test = dagon_sign::image::TEST_PUBLIC_KEY;
        assert!(allow_key(&test, false).is_err());
        assert!(allow_key(&test, true).is_ok());
        assert!(allow_key(&[7; 32], false).is_ok());
    }

    #[test]
    fn the_work_area_is_the_config_files_absolute_directory() {
        let here = std::fs::canonicalize(".").unwrap();
        let alone = base_of(Path::new("dagon.toml"));
        assert!(alone.is_absolute());
        assert_eq!(std::fs::canonicalize(&alone).unwrap(), here);
        let sub = base_of(Path::new("src/dagon.toml"));
        assert_eq!(std::fs::canonicalize(&sub).unwrap(), here.join("src"));
        assert!(!sub.to_string_lossy().starts_with(VERBATIM));
    }

    #[test]
    fn config_paths_join_the_work_area_once() {
        let d = std::env::temp_dir().join(format!("dagon-cfg-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(
            d.join("sub/dagon.toml"),
            "topology = \"t.yaml\"
",
        )
        .unwrap();
        let c = Config::load(Some(&d.join("sub/dagon.toml").to_string_lossy())).unwrap();
        assert!(c.base.is_absolute());
        assert_eq!(c.topology.unwrap(), c.base.join("t.yaml"));
        assert!(crate::paths::contained(&c.base, &c.base.join("t.yaml")).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn config_ignores_unknown_tables() {
        let c: Config = toml::from_str(
            "topology = \"t.yaml\"\n[serve]\nx = 1\n[build.seg]\npackage = \"targets/f401-app\"\n[provision.seg]\nbootcore = \"b\"\napp = \"a\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        assert_eq!(c.provision["seg"].version.as_deref(), Some("1.0.0"));
    }
}
