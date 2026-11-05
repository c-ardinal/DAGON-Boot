// The words on the screen, one per step. tools/gen_glyphs.py renders the
// characters of the string literals in this file (and only those).

/// The watchdog is armed, the state log is read.
pub const STARTING: &str = "起動しています";
/// Before the boot core decides (it checks the image's signature and hash).
pub const CHECKING: &str = "ファームウェアを確かめています";
/// A confirmed image is chosen.
pub const CONFIRMED: &str = "確定した版で起動します";
/// A new image is chosen on trial.
pub const TRIAL: &str = "新しい版を試します";
