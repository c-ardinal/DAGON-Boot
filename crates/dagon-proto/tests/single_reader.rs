//! One field reader and one command table for the whole tree: no source
//! outside dagon-proto looks into a frame's payload bytes, reads a varint,
//! or keeps its own list of command numbers or resend rules.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !matches!(
                name.as_str(),
                "target" | "tests" | "benches" | "node_modules" | ".git"
            ) {
                rust_sources(&p, out);
            }
        } else if name.ends_with(".rs") && !name.starts_with("tests") {
            out.push(p);
        }
    }
}

/// Shipped source of every crate: test folders, test files and the
/// `#[cfg(test)]` tail of a file are left out.
fn shipped() -> Vec<(String, String)> {
    let mut files = Vec::new();
    for top in ["crates", "targets", "samples"] {
        rust_sources(&root().join(top), &mut files);
    }
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap().to_string();
            let name = p
                .strip_prefix(root())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (name, code)
        })
        .collect()
}

fn offenders(found: impl Fn(&str, &str) -> bool) -> Vec<String> {
    shipped()
        .iter()
        .flat_map(|(name, code)| {
            code.lines()
                .enumerate()
                .filter(|(_, l)| !l.trim_start().starts_with("//") && found(name, l))
                .map(|(i, l)| format!("{name}:{}: {}", i + 1, l.trim()))
                .collect::<Vec<_>>()
        })
        .collect()
}

const PROTO: &str = "crates/dagon-proto/src/";

/// A frame's `payload` goes whole to a dagon-proto reader (or on to the
/// next link); nothing indexes, measures or splits it.
#[test]
fn nothing_outside_dagon_proto_looks_into_a_payload() {
    const INTO: [&str; 9] = [
        ".payload[",
        ".payload.get(",
        ".payload.first(",
        ".payload.last(",
        ".payload.split",
        ".payload.is_empty(",
        ".payload.iter(",
        ".payload.chunks",
        ".payload.starts_with(",
    ];
    // Only files that use dagon-proto hold frames.
    let framed: Vec<String> = shipped()
        .into_iter()
        .filter(|(name, code)| !name.starts_with(PROTO) && code.contains("dagon_proto"))
        .map(|(name, _)| name)
        .collect();
    let hits =
        offenders(|name, l| framed.iter().any(|f| f == name) && INTO.iter().any(|p| l.contains(p)));
    assert!(
        hits.is_empty(),
        "payload bytes read by hand:\n{}",
        hits.join("\n")
    );
}

#[test]
fn one_varint_reader_and_one_field_reader() {
    let varints = offenders(|_, l| l.contains("fn varint") || l.contains("fn read_varint"));
    assert_eq!(varints.len(), 1, "{varints:?}");
    assert!(varints[0].starts_with("crates/dagon-proto/src/field.rs"));
    // The typed payloads hold no cursor of their own.
    let cursors = offenders(|name, l| {
        name.starts_with(PROTO) && !name.ends_with("field.rs") && l.contains("split_at(")
    });
    assert!(cursors.is_empty(), "{cursors:?}");
    let leb = offenders(|name, l| !name.starts_with(PROTO) && l.to_lowercase().contains("leb128"));
    assert!(leb.is_empty(), "{leb:?}");
}

#[test]
fn command_numbers_and_resend_rules_come_from_the_table() {
    let lists = offenders(|name, l| {
        l.contains("NEVER_RESENT")
            || (!name.starts_with(PROTO)
                && l.contains("const CMD_")
                && l.contains(": u8")
                && l.contains("0x"))
    });
    assert!(lists.is_empty(), "{lists:?}");
    // The only Cmd enum, and it is the table's.
    let enums = offenders(|_, l| l.contains("enum Cmd "));
    assert_eq!(enums.len(), 1, "{enums:?}");
    assert!(enums[0].starts_with("crates/dagon-proto/src/table.rs"));
}

fn web_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "pkg" {
                web_sources(&p, out);
            }
        } else if (name.ends_with(".ts") || name.ends_with(".svelte"))
            && !name.ends_with(".test.ts")
            && !name.ends_with(".d.ts")
        {
            out.push(p);
        }
    }
}

/// Words that make a name one of the wire's: a command, a status, a field,
/// a key, the wire or protocol version, or a value of one of its fields
/// (HELLO's role, mode and port kind, a STATUS slot's state and the
/// factory slot, SHOW's phase), in any case and spelling.
const WIRE_WORDS: [&str; 17] = [
    "slot_",
    "port_",
    "state",
    "mode",
    "role",
    "phase",
    "portkind",
    "factory",
    "cmd",
    "command",
    "status",
    "field",
    "key",
    "wire",
    "protover",
    "proto_ver",
    "protocolver",
];

/// All-capitals entries of the tables the wire files keep (`HELLO: 0x01`).
const ENTRY_WORDS: [&str; 7] = [
    "HELLO",
    "RELAY",
    "XFER",
    "REBOOT",
    "CONFIRM",
    "SHOW",
    "SET_PENDING",
];

/// The words of [`WIRE_WORDS`] shipped Rust may not give a number: there
/// `field`, `key` and `wire` name many things that are not on the wire.
const RUST_WIRE_WORDS: [&str; 3] = ["cmd", "command", "status"];

fn is_wire_name(name: &str, words: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    words.iter().any(|w| lower.contains(w)) || name.starts_with("F_")
}

fn starts_with_number(s: &str) -> bool {
    s.trim_start().starts_with(|c: char| c.is_ascii_digit())
}

fn ends_with_number(s: &str) -> bool {
    let s = s.trim_end();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let start = s.rfind(|c: char| !word(c)).map_or(0, |i| i + 1);
    s[start..].starts_with(|c: char| c.is_ascii_digit())
}

const COMPARES: [&str; 4] = ["===", "!==", "==", "!="];

/// Whether one line gives a wire name a number: a declaration, assignment
/// or entry (`name = N`, `name: N`), or a comparison of it with a literal
/// on either side. `entries`: also the all-capitals entries of the files
/// that speak the wire. `words`: what makes a name the wire's.
fn line_names_a_wire_number(line: &str, entries: bool, words: &[&str]) -> bool {
    let b = line.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while i < b.len() {
        if !word(b[i]) || (i > 0 && word(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && word(b[i]) {
            i += 1;
        }
        let name = &line[start..i];
        let entry = entries
            && name.bytes().all(|c| c.is_ascii_uppercase() || c == b'_')
            && ENTRY_WORDS.iter().any(|p| name.starts_with(p));
        if !(is_wire_name(name, words) || entry) || name.as_bytes()[0].is_ascii_digit() {
            continue;
        }
        let rest = line[i..].trim_start();
        if let Some(op) = COMPARES.iter().find(|op| rest.starts_with(**op)) {
            if starts_with_number(&rest[op.len()..]) {
                return true;
            }
            continue;
        }
        // `name = N`, `name: N`, and the typed `name: Type = N`.
        fn set(v: &str) -> Option<&str> {
            v.strip_prefix('=').filter(|v| !v.starts_with(['=', '>']))
        }
        let typed = rest.strip_prefix(':').and_then(|v| {
            set(v.trim_start_matches(|c: char| {
                c.is_ascii_alphanumeric() || "_ <>[]|.&'".contains(c)
            }))
        });
        let value = set(rest).or(typed).or(rest.strip_prefix(':'));
        if value.is_some_and(starts_with_number) {
            return true;
        }
        // A table of them: `name = { first: N, ... }`.
        let first = value
            .and_then(|v| v.trim_start().strip_prefix('{'))
            .and_then(|v| v.split_once(':'));
        if first.is_some_and(|(_, n)| starts_with_number(n)) {
            return true;
        }
        // The literal first: `0x01 === frame.cmd`.
        let before = line[..start].trim_end().trim_end_matches('.');
        let before = before.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        let before = before.trim_end_matches('.').trim_end();
        if let Some(op) = COMPARES.iter().find(|op| before.ends_with(**op)) {
            if ends_with_number(&before[..before.len() - op.len()]) {
                return true;
            }
        }
    }
    false
}

/// The lines (from 1) of `text` that write a wire number: what
/// [`line_names_a_wire_number`] finds, and a `case N:` inside a `switch`
/// over a wire name. Comment lines are left out.
fn wire_numbers(text: &str, entries: bool) -> Vec<usize> {
    let mut hits = Vec::new();
    // Brace depth inside a switch over a wire name; 0 outside one.
    let mut depth = 0usize;
    for (i, l) in text.lines().enumerate() {
        let code = l.trim_start();
        if code.starts_with("//") || code.starts_with('*') || code.starts_with("/*") {
            continue;
        }
        let mut hit = line_names_a_wire_number(l, entries, &WIRE_WORDS);
        if depth > 0 {
            let case = code.strip_prefix("case ").is_some_and(starts_with_number);
            hit |= case;
            depth += l.matches('{').count();
            depth = depth.saturating_sub(l.matches('}').count());
        } else if let Some(at) = l.find("switch") {
            let head = l[at..].split('{').next().unwrap_or("");
            if is_wire_name(head, &WIRE_WORDS) {
                depth = l[at..].matches('{').count().max(1);
                depth = depth.saturating_sub(l[at..].matches('}').count());
            }
        }
        if hit {
            hits.push(i + 1);
        }
    }
    hits
}

/// Files of the page that talk HTTP to `dagon serve` and never the wire:
/// there, `status` compared with a three-digit number is an HTTP response
/// status. Nothing else in them is let through.
const HTTP_FILES: [&str; 2] = [
    "web/src/app/agent.svelte.ts",
    "web/src/app/define/cli.svelte.ts",
];

/// `line` with every `status === NNN` / `status !== NNN` (an HTTP response
/// status: exactly three digits) taken out.
fn without_http_status(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("status") {
        let after = &rest[at + "status".len()..];
        let op = ["===", "!=="]
            .iter()
            .find(|op| after.trim_start().starts_with(**op));
        let number = op.map(|op| after.trim_start()[op.len()..].trim_start());
        let digits = number.map_or(0, |n| n.bytes().take_while(u8::is_ascii_digit).count());
        match number {
            Some(n) if digits == 3 => {
                out.push_str(&rest[..at]);
                rest = &n[digits..];
            }
            _ => {
                out.push_str(&rest[..at + "status".len()]);
                rest = after;
            }
        }
    }
    out + rest
}

/// `text` with the lines of each `<style>` block of a component left
/// empty, so that line numbers stay.
fn without_style(text: &str) -> String {
    let mut inside = false;
    let mut out = String::new();
    for l in text.lines() {
        if l.trim_start().starts_with("<style") {
            inside = true;
        }
        if !inside {
            out.push_str(l);
        }
        if l.trim_start().starts_with("</style>") {
            inside = false;
        }
        out.push('\n');
    }
    out
}

/// The page takes every command, status and field number from dagon-proto
/// through dagon-wasm: its source writes none, in any spelling, and names
/// no field at all.
#[test]
fn the_page_holds_no_command_status_or_field_number() {
    // The rule itself: what it catches and what it lets through.
    for planted in [
        "const CMD_XFER_BEGIN = 0x10;",
        "  HELLO: 0x01,",
        "const STATUS_ARG = 3;",
        "const F_SLOT = 1;",
        "export const CMD = { STATUS: 2 };",
        "const cmdHello = 1;",
        "const cmdHello: number = 1;",
        "let status: number = 3;",
        "export const CMD_XFER: Readonly<number> = 0x10;",
        "const fieldSlot = 1;",
        "let wireVersion = 2;",
        "  protoVer: 2,",
        "const slotKey = 0x03;",
        "if (frame.cmd === 0x01) return;",
        "  if (f.payload[0] !== 0 && resp.status == 0x03) {",
        "if (0x50 === req.cmd) {",
        "  {#if node.status === 4}",
        "switch (frame.cmd) {\n  case 0x02:\n    break;\n}",
        "switch (status) {\n  default:\n    return;\n  case 5: {\n  }\n}",
        "const SLOT_CONFIRMED = 4;",
        "  if (s.state === 0) return;",
        "const FACTORY = 2;",
        "export const PHASE = { transfer: 1 };",
        "if (h.mode === 3) {",
        "  role: 0,",
        "if (l.hello?.portKind === 2) {",
        "const PORT_VENDOR_STAGE2 = 2;",
    ] {
        assert!(
            !wire_numbers(planted, true).is_empty(),
            "not caught: {planted}"
        );
    }
    // An HTTP response status is let through only as `status === NNN`.
    let http = without_http_status("if (r.status === 404 || frame.status === 0x03) return;");
    assert!(!wire_numbers(&http, false).is_empty(), "{http}");
    let http = without_http_status("return r.status === 409 ? 'busy' : r.status !== 200;");
    assert!(wire_numbers(&http, false).is_empty(), "{http}");
    for fine in [
        "    get HELLO() {",
        "    case CMD.HELLO:",
        "    if (frame.cmd === CMD.REBOOT && body[0] === STATUS.OK) {",
        "const HDR_VERSION_AT = 0x018;",
        "  const cmd: number = frame.cmd;",
        "  status: (status: number): Uint8Array => response_status(status),",
        "switch (kind) {\n  case 2:\n    break;\n}",
        "switch (frame.cmd) {\n  case CMD.SHOW:\n    break;\n}\nswitch (n) {\n  case 1:\n}",
        "if (s.state === SLOT_STATE.empty) return;",
        "if (ws.mode === 'define') {",
    ] {
        assert!(wire_numbers(fine, true).is_empty(), "caught: {fine}");
    }
    // A component's stylesheet is not read: `--i-state: 16px` is CSS.
    let svelte = "<script>\n  const ok = 1;\n</script>\n<style>\n  .a { --i-state: 16px; }\n</style>\n{#if s.state === 4}{/if}";
    assert_eq!(wire_numbers(&without_style(svelte), true), [7]);
    let mut files = Vec::new();
    web_sources(&root().join("web/src"), &mut files);
    assert!(files.len() > 20, "the page's source was not found");
    let mut hits = Vec::new();
    for p in files {
        let path = p.strip_prefix(root()).unwrap().to_string_lossy();
        let name = path.replace(std::path::MAIN_SEPARATOR, "/");
        let speaks_wire = name.contains("/serial/") || name.contains("/wasm/");
        let text = without_style(&std::fs::read_to_string(&p).unwrap());
        let lines: Vec<&str> = text.lines().collect();
        let field_code = lines.iter().enumerate().filter_map(|(i, l)| {
            [
                "payload_read",
                "PayloadWriter",
                "readPayload",
                "writePayload",
            ]
            .iter()
            .any(|w| l.contains(w))
            .then_some(i + 1)
        });
        let http = HTTP_FILES.contains(&name.as_str());
        for n in wire_numbers(&text, speaks_wire)
            .into_iter()
            .chain(field_code)
        {
            let code = lines[n - 1].trim();
            if http && wire_numbers(&without_http_status(code), speaks_wire).is_empty() {
                continue;
            }
            hits.push(format!("{name}:{n}: {code}"));
        }
    }
    assert!(
        hits.is_empty(),
        "wire numbers written in the page:\n{}",
        hits.join("\n")
    );
}

/// The lines (from 1) of Rust `text` that are an arm, directly inside a
/// `match` over a command or status, whose pattern starts with a number
/// (`0x01 => ...`, `1 | 2 => ...`), each with the line of its `match`.
/// Comment lines are left out.
fn rust_match_arms(text: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    // Brace depth inside a match over a wire name; 0 outside one.
    let mut depth = 0usize;
    let mut head_at = 0usize;
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    for (i, l) in text.lines().enumerate() {
        let code = l.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if depth > 0 {
            if depth == 1 && starts_with_number(code) && code.contains("=>") {
                hits.push((i + 1, head_at));
            }
            depth += l.matches('{').count();
            depth = depth.saturating_sub(l.matches('}').count());
            continue;
        }
        let at = l.match_indices("match").find(|(at, _)| {
            let before = l[..*at].chars().next_back();
            let after = l[at + "match".len()..].chars().next();
            !before.is_some_and(word) && after.is_some_and(char::is_whitespace)
        });
        if let Some((at, _)) = at {
            let rest = &l[at..];
            let head = rest.split('{').next().unwrap_or("");
            if rest.contains('{') && is_wire_name(head, &RUST_WIRE_WORDS) {
                head_at = i + 1;
                depth = rest.matches('{').count();
                depth = depth.saturating_sub(rest.matches('}').count());
            }
        }
    }
    hits
}

/// Shipped Rust outside dagon-proto compares no command or status with a
/// literal on either side, matches none against a literal, and gives no
/// command or status name a number of its own: every crate, target and
/// sample is read, whether it names dagon_proto or not.
#[test]
fn shipped_rust_writes_no_command_or_status_number() {
    let rust = |l: &str| line_names_a_wire_number(l, false, &RUST_WIRE_WORDS);
    for planted in [
        "const CMD_HELLO: u8 = 1;",
        "pub(crate) const CMD_XFER_BEGIN: u8 = 0x10;",
        "static STATUS_BUSY: u8 = 4;",
        "    if 0x01 == frame.cmd {",
        "    if frame.cmd != 0x02 {",
        "    let status: u8 = 3;",
    ] {
        assert!(rust(planted), "not caught: {planted}");
    }
    for fine in [
        "    if frame.cmd == Cmd::Hello as u8 {",
        "fn reply(status: u8) -> Frame {",
        "    let cmd: u8 = frame.cmd;",
        "const HDR_LEN: usize = 16;",
    ] {
        assert!(!rust(fine), "caught: {fine}");
    }
    for planted in [
        "match frame.cmd {\n    0x01 => hello(),\n    _ => {}\n}",
        "let r = match status {\n    Cmd::Hello => 0,\n    3 | 4 => 1,\n    _ => 2,\n};",
        "match req.cmd {\n    x if x == 2 => {}\n    0x10 => {\n        go();\n    }\n    _ => {}\n}",
    ] {
        assert!(!rust_match_arms(planted).is_empty(), "not caught: {planted}");
    }
    for fine in [
        "match Cmd::from(frame.cmd) {\n    Some(Cmd::Hello) => {}\n    _ => {}\n}",
        "match frame.cmd {\n    c => match n {\n        1 => {}\n        _ => {}\n    },\n}",
        "match kind {\n    1 => {}\n    _ => {}\n}\nlet cmd = 1u8;",
        "// match frame.cmd {\n// 0x01 => {}",
    ] {
        assert!(rust_match_arms(fine).is_empty(), "caught: {fine}");
    }
    // (file, the exact code, why its number is not the wire's)
    const NOT_WIRE: [(&str, &str, &str); 3] = [
        (
            "crates/dagon-agent/src/lib.rs",
            "emits StatusAsked = 0x32 {}",
            "the agent's own event-log code",
        ),
        (
            "crates/dagon-cli/src/serve/http.rs",
            "status: 200,",
            "dagon serve's HTTP response status",
        ),
        (
            "samples/esp32s3-app/src/display.rs",
            "const OP_COMMAND: u8 = 0x02;",
            "the CO5300 display's SPI opcode",
        ),
    ];
    let mut hits = offenders(|name, l| {
        let allowed = NOT_WIRE.iter().any(|(f, c, _)| *f == name && l.contains(c));
        !name.starts_with(PROTO) && !allowed && rust(l)
    });
    // (file, the exact `match`, why its arms' numbers are not the wire's)
    const NOT_WIRE_MATCH: [(&str, &str, &str); 1] = [(
        "crates/dagon-cli/src/serve/http.rs",
        "match status {",
        "dagon serve's HTTP response status",
    )];
    for (name, code) in shipped() {
        if name.starts_with(PROTO) {
            continue;
        }
        let lines: Vec<&str> = code.lines().collect();
        for (n, head) in rust_match_arms(&code) {
            let head = lines[head - 1].trim();
            if NOT_WIRE_MATCH
                .iter()
                .any(|(f, m, _)| *f == name && head == *m)
            {
                continue;
            }
            hits.push(format!("{name}:{n}: {}", lines[n - 1].trim()));
        }
    }
    assert!(hits.is_empty(), "{hits:?}");
}
