//! The MCU and board definitions a project works with: those of its DAGON
//! source when it has a local clone (release mode: the builtin set, so
//! nothing is fetched just to read them), plus its dagon.toml's own boards
//! and registered MCU files.

use std::path::Path;

use dagon_topology::defs::{Registry, Source};

use crate::config::{Config, CONFIG_FILE};

/// Definitions for `cfg`'s project; `root` overrides the DAGON source.
pub(crate) fn registry_at(cfg: &Config, root: Option<&Path>) -> Result<Registry, String> {
    let clone = crate::dagonsrc::clone_root(&cfg.dagon, &cfg.base);
    let tree = root
        .map(Path::to_path_buf)
        .or(clone)
        .filter(|r| r.join("targets/mcu").is_dir());
    let project = cfg.base.join(CONFIG_FILE);
    let mut sources = Vec::new();
    if let Some(t) = &tree {
        sources.push(Source::Tree(t));
    }
    if project.is_file() {
        sources.push(Source::Project(&project));
    }
    Registry::load(&sources).map_err(|e| format!("definitions: {e}"))
}

/// [`registry_at`] with the project's own DAGON source.
pub(crate) fn registry(cfg: &Config) -> Result<Registry, String> {
    registry_at(cfg, None)
}

/// The topology at `path`, checked against `cfg`'s project definitions:
/// its updatable part, the one every update, deploy, flash and project
/// command works on. The nodes it leaves out are named on stderr with why.
pub fn read_topology(cfg: &Config, path: &Path) -> Result<dagon_topology::Topology, String> {
    let yaml = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let t = dagon_topology::parse_str(&yaml).map_err(|e| format!("{}: {e}", path.display()))?;
    let reg = registry(cfg)?;
    if !dagon_topology::check_with(&reg, &t).is_pushable() {
        return Err(format!(
            "{}: topology has errors; run `dagon topo check`",
            path.display()
        ));
    }
    let cut = dagon_topology::cut_with(&reg, &t);
    for line in crate::config::skipped_lines(&cut) {
        eprintln!("{line}");
    }
    cut.topology.ok_or_else(|| {
        let first = cut.report.errors().next().map_or(String::new(), |i| {
            format!(": {} {}: {}", i.rule, i.path, i.message)
        });
        format!(
            "{}: no part of this topology can be updated: the part updating would reach does not pass the checks on its own{first}",
            path.display()
        )
    })
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests;
