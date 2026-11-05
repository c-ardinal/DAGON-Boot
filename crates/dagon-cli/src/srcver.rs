//! A node's version comes from its source: the `version` of the node
//! crate's Cargo.toml for its app, of its boot crate's for its boot core.
//! Build, release and deploy read it there; a version given anywhere else
//! (`--version`, `version` in `[build.<id>]`) must be the same or the
//! command is refused.

use std::path::Path;

use dagon_format::Version;

/// `package.version` of the crate in `dir` (`version.workspace = true`
/// takes `workspace.package.version` of the nearest workspace above it).
pub(crate) fn crate_version(dir: &Path) -> Result<Version, String> {
    let manifest = dir.join("Cargo.toml");
    let doc = read_toml(&manifest)?;
    let v = doc.get("package").and_then(|p| p.get("version"));
    let text = match v {
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Table(t))
            if t.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            workspace_version(dir)?
        }
        _ => {
            return Err(format!(
                "{}: no [package] version (the node's version comes from it)",
                manifest.display()
            ))
        }
    };
    crate::commands::version(&text).map_err(|e| format!("{}: version: {e}", manifest.display()))
}

fn read_toml(path: &Path) -> Result<toml::Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// `workspace.package.version` of the nearest Cargo.toml above `dir` with a
/// `[workspace]`.
fn workspace_version(dir: &Path) -> Result<String, String> {
    for up in dir.ancestors().skip(1) {
        let path = up.join("Cargo.toml");
        if !path.is_file() {
            continue;
        }
        let doc = read_toml(&path)?;
        let Some(ws) = doc.get("workspace") else {
            continue;
        };
        return ws
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(toml::Value::as_str)
            .map(String::from)
            .ok_or_else(|| format!("{}: no [workspace.package] version", path.display()));
    }
    Err(format!(
        "{}: version.workspace = true but no workspace above it",
        dir.join("Cargo.toml").display()
    ))
}

/// Brings the crate's own entry in `dir`'s Cargo.lock to the `version`
/// its Cargo.toml says, so a version the developer raised builds with no
/// step of theirs. Only that entry changes, offline: every other package
/// stays as locked, and the `--locked` build after it still stops at a
/// dependency that was added or changed. Returns the version written, if
/// the entry changed.
pub(crate) fn lock_own_version(dir: &Path) -> Result<Option<String>, String> {
    let lock = dir.join("Cargo.lock");
    let Ok(text) = std::fs::read_to_string(&lock) else {
        return Ok(None);
    };
    let manifest = read_toml(&dir.join("Cargo.toml"))?;
    let package = manifest.get("package");
    let field = |k: &str| package.and_then(|p| p.get(k)).and_then(toml::Value::as_str);
    // An inherited version is the workspace's, whose lock is not here.
    let (Some(name), Some(version)) = (field("name"), field("version")) else {
        return Ok(None);
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", lock.display()))?;
    let Some(entries) = doc
        .get_mut("package")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    else {
        return Ok(None);
    };
    // A registry or git package has a `source`; the crate itself has none.
    let own = entries.iter_mut().find(|t| {
        t.get("name").and_then(toml_edit::Item::as_str) == Some(name) && !t.contains_key("source")
    });
    let Some(own) = own else {
        return Ok(None);
    };
    if own.get("version").and_then(toml_edit::Item::as_str) == Some(version) {
        return Ok(None);
    }
    own["version"] = toml_edit::value(version);
    let rel = crate::genfs::Rel::new("Cargo.lock")?;
    crate::genfs::write(dir, &rel, doc.to_string().as_bytes())?;
    Ok(Some(version.to_string()))
}

/// `source` (read from `what`), refused when `given` names another version.
pub(crate) fn agreed(source: Version, given: Option<&str>, what: &str) -> Result<Version, String> {
    let Some(g) = given else {
        return Ok(source);
    };
    let g = crate::commands::version(g)?;
    if g != source {
        return Err(format!(
            "version {g} was given, but {what} says {source}: the version comes from the \
             source; raise it there"
        ));
    }
    Ok(source)
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dagon-srcver-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_version_is_the_crate_s_own() {
        let d = dir("own");
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"seg\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();
        assert_eq!(crate_version(&d).unwrap(), Version::new(1, 2, 3));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_inherited_version_comes_from_the_workspace_above() {
        let d = dir("ws");
        std::fs::write(
            d.join("Cargo.toml"),
            "[workspace]\nmembers = [\"nodes/seg\"]\n[workspace.package]\nversion = \"2.0.1\"\n",
        )
        .unwrap();
        let c = d.join("nodes/seg");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(
            c.join("Cargo.toml"),
            "[package]\nname = \"seg\"\nversion.workspace = true\n",
        )
        .unwrap();
        assert_eq!(crate_version(&c).unwrap(), Version::new(2, 0, 1));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_crate_without_a_usable_version_is_refused_with_where() {
        let d = dir("none");
        assert!(crate_version(&d).unwrap_err().contains("Cargo.toml"));
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"seg\"\n").unwrap();
        assert!(crate_version(&d).unwrap_err().contains("version"));
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"seg\"\nversion = \"1.2\"\n",
        )
        .unwrap();
        assert!(crate_version(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn another_version_given_is_refused_with_both_and_the_source() {
        let v = Version::new(1, 1, 0);
        assert_eq!(agreed(v, None, "nodes/seg/Cargo.toml").unwrap(), v);
        assert_eq!(agreed(v, Some("1.1.0"), "nodes/seg/Cargo.toml").unwrap(), v);
        let e = agreed(v, Some("1.2.0"), "nodes/seg/Cargo.toml").unwrap_err();
        assert!(
            e.contains("1.2.0") && e.contains("1.1.0") && e.contains("nodes/seg/Cargo.toml"),
            "{e}"
        );
        assert!(agreed(v, Some("one"), "x").is_err());
    }
}
