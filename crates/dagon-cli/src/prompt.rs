//! Questions asked on the terminal.

use std::io::BufRead;

/// Shows `listing` and reads one line: `y` or `yes` continues.
pub(crate) fn confirm(listing: &str, input: &mut dyn BufRead) -> bool {
    print!("{listing}continue? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(n) if n > 0 => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
        _ => false,
    }
}
