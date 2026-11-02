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
        c.release.as_mut().map(rel);
        c.provision_dir.as_mut().map(rel);
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

    pub fn provision_dir(&self) -> PathBuf {
        self.provision_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PROVISION_DIR))
    }
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

/// Drops the extended-length prefix from a drive path so it prints and
/// passes to other tools as usual.
fn plain_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(VERBATIM) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
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

pub fn read_topology(path: &Path) -> Result<dagon_topology::Topology, String> {
    let yaml = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let t = dagon_topology::parse_str(&yaml).map_err(|e| format!("{}: {e}", path.display()))?;
    if !dagon_topology::check(&t).is_pushable() {
        return Err(format!(
            "{}: topology has errors; run `dagon topo check`",
            path.display()
        ));
    }
    Ok(t)
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

pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
