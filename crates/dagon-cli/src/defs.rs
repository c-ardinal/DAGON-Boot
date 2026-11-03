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
