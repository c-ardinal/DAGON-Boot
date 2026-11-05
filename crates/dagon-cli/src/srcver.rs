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
/// the entry changed; refuses a lock in which the entry is not certain.
pub(crate) fn lock_own_version(dir: &Path) -> Result<Option<String>, String> {
    let lock = dir.join("Cargo.lock");
    let text = match std::fs::read_to_string(&lock) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", lock.display())),
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
    let Some(i) = own_entry(entries, name).map_err(|e| format!("{}: {e}", lock.display()))? else {
        return Ok(None);
    };
    let own = entries.get_mut(i).expect("index from own_entry");
    if own.get("version").and_then(toml_edit::Item::as_str) == Some(version) {
        return Ok(None);
    }
    own["version"] = toml_edit::value(version);
    let new = line_endings_of(&text, doc.to_string());
    let rel = crate::genfs::Rel::new("Cargo.lock")?;
    crate::genfs::write(dir, &rel, new.as_bytes())?;
    Ok(Some(version.to_string()))
}

/// The index of crate `name`'s own entry among a lock's packages: one
/// without a `source` (a registry or git package has one) that no other
/// entry depends on (a path dependency of the same name is depended on).
fn own_entry(entries: &toml_edit::ArrayOfTables, name: &str) -> Result<Option<usize>, String> {
    let str_of = |t: &toml_edit::Table, k: &str| {
        t.get(k).and_then(toml_edit::Item::as_str).map(String::from)
    };
    // Dependencies as (name, version when the lock names one).
    let used: Vec<(String, Option<String>)> = entries
        .iter()
        .filter_map(|t| t.get("dependencies").and_then(toml_edit::Item::as_array))
        .flatten()
        .filter_map(|d| d.as_str())
        .map(|d| {
            let mut w = d.split_whitespace();
            (
                w.next().unwrap_or_default().to_string(),
                w.next().map(String::from),
            )
        })
        .collect();
    let own: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, t)| str_of(t, "name").as_deref() == Some(name) && !t.contains_key("source"))
        .filter(|(_, t)| {
            let v = str_of(t, "version");
            !used
                .iter()
                .any(|(n, uv)| n == name && (uv.is_none() || *uv == v))
        })
        .map(|(i, _)| i)
        .collect();
    match own[..] {
        [] => Ok(None),
        [i] => Ok(Some(i)),
        _ => Err(format!(
            "{} entries of package {name:?} could each be this crate's own; \
             give the crate or its path dependency another name",
            own.len()
        )),
    }
}

/// `text` with the line endings of `original` (CRLF when it has them):
/// an edit through toml_edit writes LF.
pub(crate) fn line_endings_of(original: &str, text: String) -> String {
    if original.contains("\r\n") {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text
    }
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

    /// A crate `x` 1.0.0 with a path dependency also named `x` (0.5.0):
    /// both lock entries have no source; only the root's is referenced by
    /// no other entry.
    const SAME_NAME_LOCK: &str =
        "version = 4\n\n[[package]]\nname = \"x\"\nversion = \"0.5.0\"\n\n\
         [[package]]\nname = \"x\"\nversion = \"1.0.0\"\ndependencies = [\n \"x 0.5.0\",\n]\n";

    fn raised_crate(name: &str, lock: &str) -> std::path::PathBuf {
        let d = dir(name);
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"1.1.0\"\n",
        )
        .unwrap();
        std::fs::write(d.join("Cargo.lock"), lock).unwrap();
        d
    }

    #[test]
    fn the_crates_own_lock_entry_is_raised_not_a_same_named_path_dependency() {
        let d = raised_crate("samename", SAME_NAME_LOCK);
        assert_eq!(lock_own_version(&d).unwrap().as_deref(), Some("1.1.0"));
        assert_eq!(
            std::fs::read_to_string(d.join("Cargo.lock")).unwrap(),
            SAME_NAME_LOCK.replace("\"1.0.0\"", "\"1.1.0\"")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_ambiguous_own_lock_entry_is_refused_and_the_lock_left_alone() {
        // Two same-named source-less entries, neither depended on.
        let lock = SAME_NAME_LOCK.replace("dependencies = [\n \"x 0.5.0\",\n]\n", "");
        let d = raised_crate("ambiguous", &lock);
        let e = lock_own_version(&d).unwrap_err();
        assert!(e.contains("Cargo.lock") && e.contains("\"x\""), "{e}");
        assert_eq!(std::fs::read_to_string(d.join("Cargo.lock")).unwrap(), lock);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_lock_with_crlf_line_endings_keeps_them() {
        let lock = SAME_NAME_LOCK.replace('\n', "\r\n");
        let d = raised_crate("crlf", &lock);
        assert!(lock_own_version(&d).unwrap().is_some());
        assert_eq!(
            std::fs::read_to_string(d.join("Cargo.lock")).unwrap(),
            lock.replace("\"1.0.0\"", "\"1.1.0\"")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unreadable_lock_is_an_error_not_an_absent_one() {
        let d = dir("unreadable");
        std::fs::write(
            d.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"1.1.0\"\n",
        )
        .unwrap();
        std::fs::write(d.join("Cargo.lock"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(lock_own_version(&d).unwrap_err().contains("Cargo.lock"));
        std::fs::remove_file(d.join("Cargo.lock")).unwrap();
        assert_eq!(lock_own_version(&d), Ok(None), "no lock yet");
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
