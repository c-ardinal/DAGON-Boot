#![forbid(unsafe_code)]
//! Signing for DAGON: keys, signed images, release bundles and
//! first-install flash contents.
//!
//! The secret key file only ever goes to a directory outside every git work
//! tree, is never overwritten, and is never printed or logged.

pub mod image;
pub mod provision;
pub mod release;

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use ed25519_compact::{KeyPair, PublicKey, Seed};
use sha2::{Digest, Sha256};

pub const SECRET_FILE: &str = "dagon-signing.key";
pub const PUBLIC_FILE: &str = "dagon-signing.pub";

/// Key ID = first 8 bytes of SHA-256 over the 32-byte public key.
pub fn key_id(public_key: &[u8; 32]) -> [u8; 8] {
    let digest = Sha256::digest(public_key);
    let mut id = [0u8; 8];
    id.copy_from_slice(&digest[..8]);
    id
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[derive(Debug)]
pub enum KeyError {
    /// The output path is inside a git work tree.
    InsideRepo(PathBuf),
    /// The output path contains `..`, which would defeat the repository check.
    ParentComponent(PathBuf),
    /// A key file already exists; keys are never overwritten.
    Exists(PathBuf),
    /// The key file is not in the expected format.
    Malformed(PathBuf),
    Io(io::Error),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsideRepo(p) => write!(
                f,
                "refusing to write a secret key inside a git work tree: {}",
                p.display()
            ),
            Self::ParentComponent(p) => {
                write!(f, "output path must not contain '..': {}", p.display())
            }
            Self::Exists(p) => write!(
                f,
                "key file already exists, not overwriting: {}",
                p.display()
            ),
            Self::Malformed(p) => write!(f, "malformed key file: {}", p.display()),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for KeyError {}

impl From<io::Error> for KeyError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Result of `generate_to_dir`. Holds no secret material.
#[derive(Debug, Clone)]
pub struct GeneratedKey {
    pub public_key: [u8; 32],
    pub key_id: [u8; 8],
    pub secret_path: PathBuf,
    pub public_path: PathBuf,
}

/// True when `dir` (or, if it does not exist yet, its nearest existing
/// ancestor) lies inside a git work tree.
pub fn inside_git_work_tree(dir: &Path) -> io::Result<bool> {
    let abs = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(dir)
    };
    let mut existing = abs.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(p) => existing = p,
            None => return Ok(false),
        }
    }
    let canonical = existing.canonicalize()?;
    Ok(canonical.ancestors().any(|a| a.join(".git").exists()))
}

/// Generate a fresh key pair and store it in `dir` (created if missing).
pub fn generate_to_dir(dir: &Path) -> Result<GeneratedKey, KeyError> {
    if dir.components().any(|c| c == Component::ParentDir) {
        return Err(KeyError::ParentComponent(dir.to_path_buf()));
    }
    if inside_git_work_tree(dir)? {
        return Err(KeyError::InsideRepo(dir.to_path_buf()));
    }
    let secret_path = dir.join(SECRET_FILE);
    let public_path = dir.join(PUBLIC_FILE);
    for p in [&secret_path, &public_path] {
        if p.exists() {
            return Err(KeyError::Exists(p.clone()));
        }
    }
    fs::create_dir_all(dir)?;

    let pair = KeyPair::generate();
    let public_key = *pair.pk;
    write_new(
        &secret_path,
        &format!("{}\n", to_hex(pair.sk.seed().as_ref())),
        true,
    )?;
    write_new(&public_path, &format!("{}\n", to_hex(&public_key)), false)?;
    Ok(GeneratedKey {
        public_key,
        key_id: key_id(&public_key),
        secret_path,
        public_path,
    })
}

fn write_new(path: &Path, contents: &str, secret: bool) -> Result<(), KeyError> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = secret;
    let mut file = opts.open(path).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => KeyError::Exists(path.to_path_buf()),
        _ => KeyError::Io(e),
    })?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

/// Load a key pair from a secret key file written by `generate_to_dir`.
pub fn load_key_pair(secret_path: &Path) -> Result<KeyPair, KeyError> {
    let text = fs::read_to_string(secret_path)?;
    let seed =
        from_hex::<32>(&text).ok_or_else(|| KeyError::Malformed(secret_path.to_path_buf()))?;
    Ok(KeyPair::from_seed(Seed::new(seed)))
}

/// The Signer port: signs DAGON's signed structures (image headers,
/// release manifests). DAGON's default is an Ed25519 key pair in memory.
pub trait Signer {
    /// The public key a signature verifies under.
    fn public_key(&self) -> [u8; 32];
    /// The signature of `domain` followed by `message`.
    fn sign(&self, domain: &[u8], message: &[u8]) -> [u8; 64];
}

impl Signer for KeyPair {
    fn public_key(&self) -> [u8; 32] {
        *self.pk
    }

    /// Ed25519, deterministic.
    fn sign(&self, domain: &[u8], message: &[u8]) -> [u8; 64] {
        *self.sk.sign([domain, message].concat(), None)
    }
}

/// The KeyStore port: hands over the signing key from where the user
/// keeps it. DAGON's default is a key file ([`KeyFile`]); a command that
/// prints the key ([`KeyCommand`]) reaches any other store.
pub trait KeyStore {
    /// The key, or why it cannot be had (with no secret in the text).
    fn key(&self) -> Result<KeyPair, String>;
}

/// A secret key file written by [`generate_to_dir`]. Refused when it lies
/// inside a git work tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyFile(pub PathBuf);

impl KeyStore for KeyFile {
    fn key(&self) -> Result<KeyPair, String> {
        let path = &self.0;
        let inside = inside_git_work_tree(path)
            .map_err(|e| format!("cannot check key path {}: {e}", path.display()))?;
        if inside {
            return Err(format!(
                "refusing to use a secret key stored inside a git work tree: {}",
                path.display()
            ));
        }
        load_key_pair(path).map_err(|e| e.to_string())
    }
}

/// A command that prints the secret seed on its standard output, as a key
/// file holds it (64 hex digits): how a key kept in the OS key store, a
/// password manager or an environment variable is handed over without a
/// change to DAGON (`security find-generic-password -w ...`,
/// `op read op://...`, `printenv MY_KEY`). It runs through the platform
/// shell (`sh -c`, `cmd /C`) with the caller's terminal for its prompts;
/// what it prints never appears in an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyCommand(pub String);

impl KeyStore for KeyCommand {
    fn key(&self) -> Result<KeyPair, String> {
        use std::process::{Command, Stdio};
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        let child = Command::new(shell)
            .arg(flag)
            .arg(&self.0)
            .stdin(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("the key command cannot start: {}", e.kind()))?;
        let mut out = child
            .wait_with_output()
            .map_err(|e| format!("the key command cannot be read: {}", e.kind()))?;
        let seed = std::str::from_utf8(&out.stdout)
            .ok()
            .and_then(from_hex::<32>);
        out.stdout.fill(0);
        if !out.status.success() {
            return Err(format!("the key command failed ({})", out.status));
        }
        let seed = seed.ok_or("the key command printed no key (64 hex digits expected)")?;
        Ok(KeyPair::from_seed(Seed::new(seed)))
    }
}

/// Load a public key file written by `generate_to_dir`: 64 hex digits that
/// decode to an Ed25519 point of large order (not just any 32 bytes).
pub fn load_public_key(public_path: &Path) -> Result<[u8; 32], KeyError> {
    let text = fs::read_to_string(public_path)?;
    let pk = from_hex::<32>(&text).ok_or_else(|| KeyError::Malformed(public_path.to_path_buf()))?;
    PublicKey::new(pk)
        .validate()
        .map_err(|_| KeyError::Malformed(public_path.to_path_buf()))?;
    Ok(pk)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dagon-sign-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn key_gen_round_trip() {
        let dir = scratch("rt");
        let generated = generate_to_dir(&dir).unwrap();

        let pair = load_key_pair(&generated.secret_path).unwrap();
        assert_eq!(*pair.pk, generated.public_key);
        assert_eq!(
            load_public_key(&generated.public_path).unwrap(),
            generated.public_key
        );
        assert_eq!(generated.key_id, key_id(&generated.public_key));

        // Not a point, and a small-order point: not public keys.
        let bad = dir.join("bad.pub");
        for hex in ["ff".repeat(32), "00".repeat(32)] {
            fs::write(&bad, &hex).unwrap();
            assert!(load_public_key(&bad).is_err(), "{hex}");
        }
        fs::remove_file(&bad).unwrap();

        let sig = pair.sk.sign(b"dagon", None);
        PublicKey::new(generated.public_key)
            .verify(b"dagon", &sig)
            .unwrap();

        assert!(matches!(generate_to_dir(&dir), Err(KeyError::Exists(_))));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A command printing a seed gives that key; one that fails or prints
    /// something else is refused, and no error carries what it printed.
    #[test]
    fn a_key_command_hands_over_the_key_and_never_shows_it() {
        let seed = "4c".repeat(32);
        let want = KeyPair::from_seed(Seed::new([0x4c; 32]));
        let got = KeyCommand(format!("echo {seed}")).key().unwrap();
        assert_eq!(*got.pk, *want.pk);

        let then_fail = if cfg!(windows) {
            format!("echo {seed} & exit 3")
        } else {
            format!("echo {seed}; exit 3")
        };
        let not_a_key = format!("echo {}", "secret-words ".repeat(3));
        for (cmd, says) in [
            (then_fail, "failed"),
            (not_a_key, "no key"),
            (format!("echo {}", &seed[2..]), "no key"),
        ] {
            let e = KeyCommand(cmd.clone()).key().unwrap_err();
            assert!(e.contains(says), "{cmd}: {e}");
            assert!(!e.contains(&seed[..16]) && !e.contains("secret"), "{e}");
        }
    }

    #[test]
    fn key_id_is_sha256_prefix() {
        // SHA-256 of 32 zero bytes starts with 66687aadf862bd77.
        assert_eq!(to_hex(&key_id(&[0u8; 32])), "66687aadf862bd77");
    }

    #[test]
    fn refuses_inside_repo() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target-keys-test");
        assert!(matches!(
            generate_to_dir(&dir),
            Err(KeyError::InsideRepo(_))
        ));
        assert!(!dir.exists());
    }

    #[test]
    fn refuses_parent_components() {
        let dir = std::env::temp_dir().join("..").join("x");
        assert!(matches!(
            generate_to_dir(&dir),
            Err(KeyError::ParentComponent(_))
        ));
    }
}
