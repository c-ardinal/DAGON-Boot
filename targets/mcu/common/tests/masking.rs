//! The execution model's rule for DAGON's own code (docs/design/
//! lifecycle-events.md, "Execution model"): no flash operation runs inside
//! a critical section of DAGON's. Every MCU package's sources are read, and
//! the body of every `critical_section::with` / `interrupt::free` /
//! `cortex_m::interrupt::free` call is checked for a flash operation; a
//! source that implements the `Flash` port masks nothing at all (a closure
//! it masks around could be the flash operation itself).

use std::path::{Path, PathBuf};

/// Calls that mask interrupts for their closure.
const MASKING: &[&str] = &["critical_section::with(", "interrupt::free("];
/// What a flash operation looks like in a closure body.
const FLASH_OPS: &[&str] = &[
    ".erase(",
    ".write(",
    "blocking_erase",
    "blocking_write",
    "erase_sector",
    "flash",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        if p.is_dir() && !matches!(name.to_str(), Some("target" | "vendor")) {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The text from `open` (an index just past a `(`) to its matching `)`.
fn call_body(text: &str, open: usize) -> &str {
    let mut depth = 1;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..open + i];
                }
            }
            _ => {}
        }
    }
    &text[open..]
}

/// Every masking call in `text` whose body names a flash operation, and
/// every masking call at all when `text` implements the `Flash` port.
fn violations(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let implements_flash = text.contains("Flash for ");
    for m in MASKING {
        let mut from = 0;
        while let Some(at) = text[from..].find(m) {
            let open = from + at + m.len();
            let body = call_body(text, open);
            if let Some(op) = FLASH_OPS.iter().find(|op| body.contains(*op)) {
                out.push(format!("{m}..) holds `{op}`"));
            } else if implements_flash {
                out.push(format!("{m}..) in a source implementing Flash"));
            }
            from = open;
        }
    }
    out
}

#[test]
fn no_flash_operation_runs_in_a_critical_section_of_dagon() {
    let mcu = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    rust_files(&mcu, &mut files);
    assert!(
        files.len() > 20,
        "{} sources under {}",
        files.len(),
        mcu.display()
    );
    let found: Vec<String> = files
        .iter()
        .filter(|f| !f.ends_with("masking.rs"))
        .flat_map(|f| {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            violations(&text)
                .into_iter()
                .map(move |v| format!("{}: {v}", f.display()))
        })
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_check_sees_a_flash_operation_in_a_critical_section() {
    let bad = "fn f() { critical_section::with(|cs| { self.0.borrow_ref_mut(cs).erase(a, b) }) }";
    assert_eq!(violations(bad).len(), 1);
    let good =
        "fn f() { critical_section::with(|cs| self.state.borrow_ref_mut(cs).on_trial = t); }";
    assert!(violations(good).is_empty());
    // A Flash adapter that masks around whatever it is given.
    let adapter = "impl Flash for Shared { fn erase(&mut self) { self.with(|f| f.go()) } }
                   fn with(&self) { critical_section::with(|cs| g(cs)) }";
    assert_eq!(violations(adapter).len(), 1);
}
