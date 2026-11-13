//! A project's overrides applied to the registry: each `[mcu_override.<id>]`
//! and `[board_override.<id>]` merged into the built-in or source-tree
//! definition of that id ([`super::merge`]), whose place it takes as a
//! project definition (made safe, its commands confirmed before they run)
//! that keeps its package, so its firmware and parts are found as before.

use std::fmt;

use super::load::{board_entry, err, file_refs, mcu_entry, DefError, DefSource, Pkg};
use super::merge::Def;
use super::{Origin, Registry};

/// Merges every override of a project's dagon.toml (`at`) into `reg`, MCUs
/// first, then boards, each definition on its own.
pub(super) fn apply(
    reg: &mut Registry,
    mcu: &toml::Table,
    board: &toml::Table,
    at: &impl fmt::Display,
) -> Result<(), DefError> {
    for (id, over) in mcu {
        let label = format!("{at} [mcu_override.{id}]");
        let ids: Vec<&str> = reg.mcus.iter().map(|m| m.id.as_str()).collect();
        let i = overridden(&ids, &reg.mcu_origins, id, "MCU", &label)?;
        let (text, files) = merged(Def::Mcu, &reg.mcu_sources[i], over, &label)?;
        let refs = file_refs(&files);
        let pkg = Pkg::Embedded {
            label: &label,
            files: &refs,
        };
        let (m, source) = mcu_entry(&text, &pkg, true)?;
        reg.mcus[i] = m;
        reg.mcu_sources[i] = source;
        reg.mcu_origins[i] = Origin::Overridden;
    }
    for (id, over) in board {
        let label = format!("{at} [board_override.{id}]");
        let ids: Vec<&str> = reg.boards.iter().map(|b| b.id.as_str()).collect();
        let i = overridden(&ids, &reg.board_origins, id, "board", &label)?;
        let (text, _) = merged(Def::Board, &reg.board_sources[i], over, &label)?;
        let pkg = Pkg::Embedded {
            label: &label,
            files: &[],
        };
        let (b, source) = board_entry(&text, &pkg, true)?;
        reg.boards[i] = b;
        reg.board_sources[i] = source;
        reg.board_origins[i] = Origin::Overridden;
    }
    Ok(())
}

/// Where the built-in or source-tree definition `id` an override names is.
fn overridden(
    ids: &[&str],
    origins: &[Origin],
    id: &str,
    kind: &str,
    label: &str,
) -> Result<usize, DefError> {
    let Some(i) = ids.iter().position(|d| *d == id) else {
        return err(label, format!("no {kind} '{id}' to override"));
    };
    if origins[i] == Origin::Project {
        return err(
            label,
            format!("{kind} '{id}' is the project's own definition: change it in its own text"),
        );
    }
    Ok(i)
}

/// The text of `over` merged into the definition read from `source`, and
/// the package files that definition names (the merged one keeps them).
fn merged(
    def: Def,
    source: &DefSource,
    over: &toml::Value,
    label: &str,
) -> Result<(String, Vec<(String, String)>), DefError> {
    let toml::Value::Table(over) = over else {
        return err(label, "an override is a table");
    };
    let base: toml::Table = toml::from_str(&source.toml).or_else(|e| err(label, e.to_string()))?;
    let table = super::merge::merge(def, &base, over).or_else(|e| err(label, e))?;
    let text = toml::to_string(&table).or_else(|e| err(label, e.to_string()))?;
    Ok((text, source.files.clone()))
}
