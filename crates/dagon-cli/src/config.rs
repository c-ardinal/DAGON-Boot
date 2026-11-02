//! `dagon.toml`, option parsing and signing-key loading shared by commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ed25519_compact::KeyPair;
use serde::Deserialize;

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

/// Unknown tables (such as build settings) are ignored here.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub topology: Option<PathBuf>,
    pub release: Option<PathBuf>,
    pub provision_dir: Option<PathBuf>,
    #[serde(default)]
    pub provision: BTreeMap<String, ProvisionInputs>,
}

impl Config {
    /// `path`, else `dagon.toml` in the current directory if present. Paths
    /// inside are made relative to the file's directory.
    pub fn load(path: Option<&str>) -> Result<Config, String> {
        let path = match path {
            Some(p) => PathBuf::from(p),
            None if Path::new(CONFIG_FILE).exists() => PathBuf::from(CONFIG_FILE),
            None => return Ok(Config::default()),
        };
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut c: Config =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let rel = |p: &mut PathBuf| *p = dir.join(&*p);
        c.topology.as_mut().map(rel);
        c.release.as_mut().map(rel);
        c.provision_dir.as_mut().map(rel);
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
    dagon_sign::load_key_pair(&path).map_err(|e| e.to_string())
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
    fn config_ignores_unknown_tables() {
        let c: Config = toml::from_str(
            "topology = \"t.yaml\"\n[build.seg]\ncmd = \"x\"\n[provision.seg]\nbootcore = \"b\"\napp = \"a\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        assert_eq!(c.provision["seg"].version.as_deref(), Some("1.0.0"));
    }
}
