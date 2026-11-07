//! Check results and their JSON form.

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Blocks `topo push`.
    Error,
    /// Reported only; does not block `topo push`.
    Warning,
    /// A valid shape told as it is (V-23: a part updating skips);
    /// never blocks.
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub rule: &'static str,
    pub severity: Severity,
    /// YAML path such as `links[0].control.nrst`; empty when the document
    /// could not be parsed (the message then carries line and column).
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Issue> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
    }

    /// True when `topo push` may proceed (warnings never block).
    pub fn is_pushable(&self) -> bool {
        self.errors().next().is_none()
    }

    pub fn to_json(&self) -> String {
        let mut out = format!(
            "{{\"ok\":{},\"errors\":{},\"warnings\":{},\"issues\":[",
            self.is_pushable(),
            self.errors().count(),
            self.warnings().count()
        );
        for (i, issue) in self.issues.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(
                out,
                "{{\"rule\":{},\"severity\":{},\"path\":{},\"message\":{}}}",
                json_str(issue.rule),
                json_str(issue.severity.as_str()),
                json_str(&issue.path),
                json_str(&issue.message)
            );
        }
        out.push_str("]}");
        out
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escapes_strings() {
        assert_eq!(json_str("a\"b\\c\n\u{1}é"), "\"a\\\"b\\\\c\\n\\u0001é\"");
    }
}
