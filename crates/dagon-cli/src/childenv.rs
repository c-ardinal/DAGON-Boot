//! What a child process of dagon (cargo, an agent's command) is started
//! with: the variables it never sees, and plain output.

use std::process::Command;

/// Variables never passed to a child of dagon, even when a way lists
/// them: dagon reads no agent key from the environment, but a user may
/// keep one there for other tools, and Claude Code would use it ahead of
/// the user's own login.
const NEVER_PASSED: [&str; 1] = ["ANTHROPIC_API_KEY"];

/// The variable the Claude Agent SDK reads its key from. dagon reads it
/// nowhere: [`crate::ai::port`] sets it, from the OS credential store, for
/// the porting program alone.
pub(crate) const CLAUDE_KEY_ENV: &str = NEVER_PASSED[0];

/// Environment variables a child process of dagon never sees: the signing
/// key's and [`NEVER_PASSED`].
pub fn secret_env() -> Vec<String> {
    [
        crate::config::KEY_ENV,
        crate::config::KEY_COMMAND_ENV,
        crate::config::KEY_SEED_ENV,
    ]
    .into_iter()
    .chain(NEVER_PASSED.iter().copied())
    .map(str::to_string)
    .collect()
}

/// cargo's lines are read as text, on the page and in messages: a colour
/// setting in the caller's environment or cargo configuration stays out of them.
pub fn plain_output(cmd: &mut Command) {
    cmd.env("CARGO_TERM_COLOR", "never");
}
