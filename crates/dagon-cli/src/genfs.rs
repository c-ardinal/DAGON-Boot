//! The only way dagon writes or removes files it owns in a work area (the
//! generated crates, `out/`), and the writer for paths the user names.
//!
//! Paths inside the work area are [`Rel`]: plain relative names, joined to
//! the work area lexically. A `Rel` cannot be made from an absolute or an
//! already resolved path, so nothing reaches these calls after a link was
//! followed. Before every write, rename or removal each part of the path
//! below the work area is looked at again (`symlink_metadata`); a symbolic
//! link or a junction there is refused with the reason, never followed.
//!
//! Threat model: the links a user (or a tool of theirs) has made in the
//! work area. A process racing dagon with the user's own rights between
//! that check and the call is out of scope; the check sits right before
//! each call to keep that window small.
//!
//! Clippy's `disallowed-methods` (crates/dagon-cli/clippy.toml) refuses
//! every other file write or removal in dagon-cli, except a function or a
//! test module that names its reason.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::paths::{contained, write_atomic};

/// A path inside the work area: plain names joined by `/`, never absolute,
/// never `..`, never a resolved path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rel(String);

impl Rel {
    pub fn new(rel: impl AsRef<str>) -> Result<Rel, String> {
        let rel = rel.as_ref();
        let parts: Vec<String> = Path::new(rel)
            .components()
            .map(|c| match c {
                Component::Normal(n) => Ok(n.to_string_lossy().to_string()),
                _ => Err(format!("'{rel}' is not a plain relative path")),
            })
            .collect::<Result<_, _>>()?;
        if parts.is_empty() {
            return Err("an empty path".into());
        }
        Ok(Rel(parts.join("/")))
    }

    /// `self/name`.
    pub fn join(&self, name: impl AsRef<str>) -> Result<Rel, String> {
        Rel::new(format!("{}/{}", self.0, name.as_ref()))
    }

    /// The same path with `suffix` (letters, digits, `.`, `-`, `_`) added
    /// to its last name.
    pub(crate) fn with_suffix(&self, suffix: &str) -> Rel {
        assert!(
            suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
                && suffix != "."
                && suffix != "..",
            "suffix {suffix:?}"
        );
        Rel(format!("{}{suffix}", self.0))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last name.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }
}

impl fmt::Display for Rel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where an output goes: inside the work area (dagon's own, guarded at
/// every part), or a path the user named (refused only when it is a link
/// itself).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    Work(Rel),
    User(PathBuf),
}

impl Out {
    /// `path` as the caller wrote it (joined from the work area or named
    /// by the user, never resolved through a link): inside the work area,
    /// `Work`; elsewhere, `User`. The one bridge from a path to a [`Rel`];
    /// passing a resolved path here would skip the link checks, so callers
    /// never resolve first.
    pub fn of(base: &Path, path: &Path) -> Result<Out, String> {
        let at = crate::config::lexical(path);
        if let Ok(r) = at.strip_prefix(crate::config::lexical(base)) {
            return Ok(Out::Work(Rel::new(r.to_string_lossy())?));
        }
        // Outside the work area as written, yet under it once resolved:
        // a resolved path, which would skip the link checks. Refused.
        let plain = |p: &Path| crate::config::plain_path(p.to_path_buf());
        let resolved = std::fs::canonicalize(base).is_ok_and(|b| plain(&at).starts_with(plain(&b)));
        if resolved {
            return Err(format!(
                "{} is a resolved path inside the work area; dagon writes there only by the path as written",
                path.display()
            ));
        }
        Ok(Out::User(at))
    }

    pub fn join(&self, name: &str) -> Result<Out, String> {
        Ok(match self {
            Out::Work(r) => Out::Work(r.join(name)?),
            Out::User(p) => Out::User(p.join(name)),
        })
    }

    /// The path as written (for messages and tools), not resolved.
    pub fn path(&self, base: &Path) -> PathBuf {
        match self {
            Out::Work(r) => base.join(r.as_str()),
            Out::User(p) => p.clone(),
        }
    }
}

/// The first part of `rel` (from the work area down) that is a link;
/// `None` when there is none. Parts that do not exist yet are not links.
pub fn link_in(base: &Path, rel: &Rel) -> Option<String> {
    let mut at = PathBuf::new();
    for part in rel.as_str().split('/') {
        at.push(part);
        match std::fs::symlink_metadata(base.join(&at)) {
            Ok(m) if m.file_type().is_symlink() => {
                return Some(at.to_string_lossy().replace('\\', "/"))
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    None
}

/// Why a link refuses a path.
pub fn link_reason(link: &str) -> String {
    format!(
        "{link} is a link (symbolic link or junction); dagon writes and removes nothing through it"
    )
}

/// `rel` in `base` as written (no link resolved), refused when a link is on
/// the way or it would leave the work area. Call right before the file
/// operation it is for.
pub fn checked(base: &Path, rel: &Rel) -> Result<PathBuf, String> {
    if let Some(link) = link_in(base, rel) {
        return Err(link_reason(&link));
    }
    contained(base, Path::new(rel.as_str()))?;
    Ok(base.join(rel.as_str()))
}

/// Writes `bytes` to `rel` (atomically, folders made as needed).
pub fn write(base: &Path, rel: &Rel, bytes: &[u8]) -> Result<(), String> {
    write_atomic(&checked(base, rel)?, bytes)
}

/// Removes the file `rel`; `Ok(false)` when there is none.
pub fn remove(base: &Path, rel: &Rel) -> Result<bool, String> {
    match std::fs::remove_file(checked(base, rel)?) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {rel}: {e}")),
    }
}

/// Renames `from` to `to`, both in the work area.
pub fn rename(base: &Path, from: &Rel, to: &Rel) -> Result<(), String> {
    let (f, t) = (checked(base, from)?, checked(base, to)?);
    std::fs::rename(f, t).map_err(|e| format!("cannot rename {from} to {to}: {e}"))
}

/// Removes the folder `rel` when it is empty; whether it did.
pub fn remove_empty_dir(base: &Path, rel: &Rel) -> bool {
    checked(base, rel).is_ok_and(|p| std::fs::remove_dir(p).is_ok())
}

/// Removes the folder `rel` and everything in it; `Ok(false)` when there is
/// none. Links inside it are removed as links, never followed.
pub fn remove_tree(base: &Path, rel: &Rel) -> Result<bool, String> {
    match std::fs::remove_dir_all(checked(base, rel)?) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot clear {rel}: {e}")),
    }
}

/// Copies the file `from` (anywhere) to `rel`, folders made as needed.
pub fn copy(base: &Path, from: &Path, rel: &Rel) -> Result<(), String> {
    if let Some(parent) = rel.as_str().rsplit_once('/').map(|(d, _)| d) {
        let dir = checked(base, &Rel::new(parent)?)?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::copy(from, checked(base, rel)?)
        .map(|_| ())
        .map_err(|e| format!("cannot copy {} to {rel}: {e}", from.display()))
}

/// A path the user named, refused when it is itself a link.
fn user_path(path: &Path) -> Result<PathBuf, String> {
    let link = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    if link {
        return Err(link_reason(&path.display().to_string()));
    }
    Ok(path.to_path_buf())
}

/// Writes a path the user named (a command's output, a report folder).
pub fn write_user(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic(&user_path(path)?, bytes)
}

/// The path a tool may write `out` to, checked as the writes here are.
pub fn check_out(base: &Path, out: &Out) -> Result<PathBuf, String> {
    match out {
        Out::Work(r) => checked(base, r),
        Out::User(p) => user_path(p),
    }
}

/// Writes an output, checked as [`check_out`] does.
pub fn write_out(base: &Path, out: &Out, bytes: &[u8]) -> Result<(), String> {
    match out {
        Out::Work(r) => write(base, r, bytes),
        Out::User(p) => write_user(p, bytes),
    }
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dagon-genfs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("user")).unwrap();
        std::fs::write(d.join("user/Cargo.lock"), "user\n").unwrap();
        d
    }

    pub(crate) fn junction(link: &Path, target: &Path) -> bool {
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

    fn r(s: &str) -> Rel {
        Rel::new(s).unwrap()
    }

    /// `nodes/n` a link to `user/`: nothing is written or removed there.
    fn refuses_through(name: &str, make: fn(&Path, &Path) -> bool) -> bool {
        let base = scratch(name);
        std::fs::create_dir_all(base.join("nodes")).unwrap();
        if !make(&base.join("nodes/n"), &base.join("user")) {
            let _ = std::fs::remove_dir_all(&base);
            return false;
        }
        let e = write(&base, &r("nodes/n/Cargo.toml"), b"x").unwrap_err();
        assert!(e.contains("nodes/n is a link"), "{e}");
        let e = remove(&base, &r("nodes/n/Cargo.lock")).unwrap_err();
        assert!(e.contains("nodes/n is a link"), "{e}");
        let e = rename(&base, &r("nodes/n/Cargo.lock"), &r("x")).unwrap_err();
        assert!(e.contains("nodes/n is a link"), "{e}");
        assert!(remove_tree(&base, &r("nodes/n")).is_err());
        assert!(!remove_empty_dir(&base, &r("nodes/n")));
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
    fn plain_paths_are_written_renamed_and_removed() {
        let base = scratch("plain");
        write(&base, &r("nodes/n/a.rs"), b"x").unwrap();
        rename(&base, &r("nodes/n/a.rs"), &r("nodes/n/b.rs")).unwrap();
        assert!(remove(&base, &r("nodes/n/b.rs")).unwrap());
        assert!(!remove(&base, &r("nodes/n/b.rs")).unwrap());
        assert!(remove_empty_dir(&base, &r("nodes/n")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_rel_is_never_absolute_or_upward() {
        for bad in ["../x", "/x", "a/../b", "", "C:/x", "C:\\x"] {
            if cfg!(windows) || !bad.starts_with("C:") {
                assert!(Rel::new(bad).is_err(), "{bad}");
            }
        }
        assert_eq!(r("a/b").join("c.img").unwrap().as_str(), "a/b/c.img");
        assert_eq!(r("a/b.img").with_suffix(".new").file_name(), "b.img.new");
    }

    #[test]
    fn out_of_classifies_as_written_and_refuses_a_resolved_path() {
        let base = scratch("out-of");
        let inside = Out::of(&base, &base.join("out/provision/seg")).unwrap();
        assert_eq!(inside, Out::Work(r("out/provision/seg")));
        let outside = Out::of(&base, &base.join("../shared/seg")).unwrap();
        assert!(matches!(outside, Out::User(_)), "{outside:?}");
        // The same folder in its resolved form (the Windows verbatim prefix):
        // under the work area once resolved, outside it as written.
        let canon = std::fs::canonicalize(&base).unwrap();
        if canon.strip_prefix(&base).is_err() {
            let e = Out::of(&base, &canon.join("out/sign/x.img")).unwrap_err();
            assert!(e.contains("resolved path"), "{e}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
