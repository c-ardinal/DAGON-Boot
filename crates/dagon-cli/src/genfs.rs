//! The only way `dagon gen`, `dagon new` and `dagon init` write or remove
//! files in a work area. A path whose final part or any folder between the
//! work area and it is a symbolic link or a junction is refused with the
//! reason: its target is the user's, so nothing is written or removed
//! through it. Clippy's `disallowed-methods` (crates/dagon-cli/clippy.toml)
//! refuses every other file write or removal in dagon-cli outside the
//! modules main.rs exempts with a reason.

use std::path::{Component, Path, PathBuf};

use crate::paths::{contained, write_atomic};

/// The first part of `rel` (from the work area down) that is a link, as a
/// `/`-separated relative path; `None` when there is none. Parts that do
/// not exist yet are not links.
pub fn link_in(base: &Path, rel: &str) -> Result<Option<String>, String> {
    let mut at = PathBuf::new();
    for c in Path::new(rel).components() {
        let Component::Normal(name) = c else {
            return Err(format!("'{rel}' is not a plain relative path"));
        };
        at.push(name);
        match std::fs::symlink_metadata(base.join(&at)) {
            Ok(m) if m.file_type().is_symlink() => {
                return Ok(Some(at.to_string_lossy().replace('\\', "/")))
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(None)
}

/// Why a link refuses `rel`.
pub fn link_reason(link: &str) -> String {
    format!(
        "{link} is a link (symbolic link or junction); dagon writes and removes nothing through it"
    )
}

/// `rel` inside `base`, refused when a link is on the way.
pub fn checked(base: &Path, rel: &str) -> Result<PathBuf, String> {
    if let Some(link) = link_in(base, rel)? {
        return Err(link_reason(&link));
    }
    contained(base, Path::new(rel))
}

/// Writes `bytes` to `rel` (atomically, folders made as needed).
pub fn write(base: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    write_atomic(&checked(base, rel)?, bytes)
}

/// Removes the file `rel`; `Ok(false)` when there is none.
pub fn remove(base: &Path, rel: &str) -> Result<bool, String> {
    let path = checked(base, rel)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {rel}: {e}")),
    }
}

/// Removes the folder `rel` when it is empty; whether it did.
pub fn remove_empty_dir(base: &Path, rel: &str) -> bool {
    checked(base, rel).is_ok_and(|p| std::fs::remove_dir(p).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dagon-genfs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("user")).unwrap();
        std::fs::write(d.join("user/Cargo.lock"), "user\n").unwrap();
        d
    }

    fn junction(link: &Path, target: &Path) -> bool {
        if !cfg!(windows) {
            return false;
        }
        let p = |x: &Path| x.components().collect::<PathBuf>();
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(p(link))
            .arg(p(target))
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn dir_symlink(link: &Path, target: &Path) -> bool {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        return std::os::windows::fs::symlink_dir(target, link).is_ok();
    }

    /// `nodes/n` a link to `user/`: nothing is written or removed there.
    fn refuses_through(name: &str, make: fn(&Path, &Path) -> bool) -> bool {
        let base = scratch(name);
        std::fs::create_dir_all(base.join("nodes")).unwrap();
        if !make(&base.join("nodes/n"), &base.join("user")) {
            let _ = std::fs::remove_dir_all(&base);
            return false;
        }
        let e = write(&base, "nodes/n/Cargo.toml", b"x").unwrap_err();
        assert!(e.contains("nodes/n is a link"), "{e}");
        let e = remove(&base, "nodes/n/Cargo.lock").unwrap_err();
        assert!(e.contains("nodes/n is a link"), "{e}");
        assert!(!remove_empty_dir(&base, "nodes/n"));
        assert_eq!(
            std::fs::read_to_string(base.join("user/Cargo.lock")).unwrap(),
            "user\n"
        );
        assert!(!base.join("user/Cargo.toml").exists());
        let _ = std::fs::remove_dir_all(&base);
        true
    }

    #[test]
    fn nothing_goes_through_a_junction_or_a_symlink() {
        let j = refuses_through("junction", junction);
        assert_eq!(j, cfg!(windows), "the junction case runs on Windows");
        if !refuses_through("symlink", dir_symlink) {
            eprintln!("directory symlinks are not allowed here: the symlink case did not run");
        }
    }

    #[test]
    fn plain_paths_are_written_and_removed() {
        let base = scratch("plain");
        write(&base, "nodes/n/a.rs", b"x").unwrap();
        assert!(remove(&base, "nodes/n/a.rs").unwrap());
        assert!(!remove(&base, "nodes/n/a.rs").unwrap());
        assert!(remove_empty_dir(&base, "nodes/n"));
        assert!(checked(&base, "../x").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
