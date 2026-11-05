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
