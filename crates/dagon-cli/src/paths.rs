//! File-system rules for paths that come from dagon.toml or the browser:
//! containment in the work area after resolving symlinks and junctions,
//! Windows device names, and atomic replacement of files.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Names Windows maps to devices in every directory (with any extension).
pub fn device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or("").trim_end();
    let up = stem.to_ascii_uppercase();
    matches!(
        up.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((up.starts_with("COM") || up.starts_with("LPT"))
        && up.len() == 4
        && up.as_bytes()[3].is_ascii_digit())
}

/// `path` resolved through every symlink and junction, which must stay
/// inside `base` (also resolved). Parts that do not exist yet must be
/// plain names (no `..`, no device names). Returns the resolved path.
pub fn contained(base: &Path, path: &Path) -> Result<PathBuf, String> {
    let leave = || "path leaves the work area".to_string();
    let root = std::fs::canonicalize(base).map_err(|_| "work area not found".to_string())?;
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    // Only the part below the work area is checked: the work area itself
    // may sit anywhere.
    let below = full.strip_prefix(base).unwrap_or(&full);
    for c in below.components() {
        if let Component::Normal(n) = c {
            if device_name(&n.to_string_lossy()) {
                return Err("device names are not allowed in paths".into());
            }
        }
    }
    let mut existing = full.clone();
    let mut rest: Vec<OsString> = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        match existing.components().next_back() {
            Some(Component::Normal(n)) => rest.push(n.to_os_string()),
            _ => return Err(leave()),
        }
        if !existing.pop() {
            return Err(leave());
        }
    }
    let mut resolved = std::fs::canonicalize(&existing).map_err(|_| leave())?;
    resolved.extend(rest.iter().rev());
    if resolved.starts_with(&root) {
        Ok(resolved)
    } else {
        Err(leave())
    }
}

/// The bytes of `path` when it holds at most `max`; `Ok(None)` when it is
/// larger. Reads at most `max + 1` bytes whatever the file's size.
pub fn read_capped(path: &Path, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut out = Vec::new();
    std::fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut out)?;
    Ok((out.len() <= max).then_some(out))
}

static TMP: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` through a temporary file in the same directory
/// and a rename, so readers see the old or the new file, never a part.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let name = path.file_name().ok_or("no file name")?.to_string_lossy();
    let n = TMP.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace {}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn capped_reads_stop_past_the_limit() {
        let d = std::env::temp_dir().join(format!("dagon-capped-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("f");
        std::fs::write(&f, [7u8; 10]).unwrap();
        assert_eq!(super::read_capped(&f, 10).unwrap(), Some(vec![7u8; 10]));
        assert_eq!(super::read_capped(&f, 9).unwrap(), None);
        assert!(super::read_capped(&d.join("missing"), 9).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    use super::*;

    fn area(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dagon-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("in/sub")).unwrap();
        std::fs::create_dir_all(d.join("outside")).unwrap();
        d
    }

    #[test]
    fn device_names_are_refused() {
        for n in ["CON", "con", "nul.txt", "COM1", "lpt9.log", "aux", "CONIN$"] {
            assert!(device_name(n), "{n}");
        }
        for n in ["console", "com10", "seg", "nuls", "lpt"] {
            assert!(!device_name(n), "{n}");
        }
        let d = area("dev");
        assert!(contained(&d.join("in"), Path::new("sub/NUL")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn device_names_above_the_work_area_do_not_count() {
        // A work area under a folder named like a device is still usable:
        // only parts below the work area are checked. (Windows cannot make
        // such a folder, so the check is on the path text.)
        let base = PathBuf::from("/srv/aux.d/area");
        let full = base.join("out/x");
        let below = full.strip_prefix(&base).unwrap();
        assert!(below
            .components()
            .all(|c| !device_name(&c.as_os_str().to_string_lossy())));
        assert!(device_name("aux.d"));
    }

    #[test]
    fn paths_stay_in_the_work_area() {
        let d = area("contain");
        let base = d.join("in");
        assert!(contained(&base, Path::new("sub/new/file.txt")).is_ok());
        assert!(contained(&base, Path::new("sub")).is_ok());
        assert!(contained(&base, Path::new("../outside/x")).is_err());
        assert!(contained(&base, Path::new("sub/../../outside")).is_err());
        assert!(contained(&base, &d.join("outside")).is_err(), "absolute");
        assert!(contained(&base, Path::new("missing/../../x")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn links_out_of_the_work_area_are_refused() {
        let d = area("link");
        let base = d.join("in");
        let link = base.join("escape");
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(d.join("outside"))
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(d.join("outside"), &link).is_ok();
        assert!(made, "could not create the test link");
        assert!(contained(&base, Path::new("escape")).is_err());
        assert!(contained(&base, Path::new("escape/new.txt")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn atomic_write_replaces_whole_files() {
        let d = area("atomic");
        let f = d.join("in/x.toml");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        let leftovers = std::fs::read_dir(d.join("in"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
