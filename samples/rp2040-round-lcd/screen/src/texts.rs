// The words on the screens. tools/gen_glyphs.py renders the characters of
// the string literals in this file (and only those).

// The boot screen: one per step.

/// The watchdog is armed, the state log is read.
pub const STARTING: &str = "起動しています";
/// Before the boot core decides (it checks the image's signature and hash).
pub const CHECKING: &str = "ファームウェアを検証しています";
/// A confirmed image is chosen.
pub const CONFIRMED: &str = "確定したバージョンで起動します";
/// A new image is chosen on trial.
pub const TRIAL: &str = "新規バージョンを試行します";

// The app's home screen: the state of this node, one word each.

/// The app has started; DAGON has not opened the slot store yet.
pub const HOME_STARTING: &str = "起動中";
/// The running image is confirmed.
pub const HOME_CONFIRMED: &str = "確定";
/// A new image is being written to this node.
pub const HOME_WRITING: &str = "書込中";
/// The written image is being checked.
pub const HOME_VERIFYING: &str = "検証中";
/// The new image is written and checked.
pub const HOME_WRITTEN: &str = "書込済";
/// The new image starts at the next reboot.
pub const HOME_PENDING: &str = "再起動待ち";
/// The node reboots into the new image.
pub const HOME_REBOOTING: &str = "再起動";
/// The running image is on trial.
pub const HOME_TRIAL: &str = "試行中";
/// The trial failed; the previous image runs again.
pub const HOME_ROLLED_BACK: &str = "ロールバック";
/// A write was refused, failed its checks or was dropped.
pub const HOME_FAILED: &str = "失敗";

/// Every word above, in order.
pub const WORDS: &[&str] = &[
    STARTING,
    CHECKING,
    CONFIRMED,
    TRIAL,
    HOME_STARTING,
    HOME_CONFIRMED,
    HOME_WRITING,
    HOME_VERIFYING,
    HOME_WRITTEN,
    HOME_PENDING,
    HOME_REBOOTING,
    HOME_TRIAL,
    HOME_ROLLED_BACK,
    HOME_FAILED,
];
