//! `topo compile`: validated YAML model -> topology canonical binary.
//!
//! The byte layout is owned by dagon-format so the hub firmware shares it.
//! Until dagon-format exposes its encoder, `compile` validates and then fails
//! with [`CompileError::EncoderUnavailable`].

use std::fmt;

use crate::{check, Report, Topology};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The YAML did not parse.
    Parse(String),
    /// `topo check` reported errors.
    Invalid(Report),
    EncoderUnavailable,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "topology does not parse: {e}"),
            Self::Invalid(r) => write!(
                f,
                "topology has {} error(s); run `dagon topo check`",
                r.errors().count()
            ),
            Self::EncoderUnavailable => f.write_str("topology binary encoder not available yet"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Canonical binary of a topology that passes `check` (warnings allowed).
pub fn compile(t: &Topology) -> Result<Vec<u8>, CompileError> {
    let report = check(t);
    if !report.is_pushable() {
        return Err(CompileError::Invalid(report));
    }
    encode(t)
}

pub fn parse_str(yaml: &str) -> Result<Topology, CompileError> {
    yaml_serde::from_str(yaml).map_err(|e| CompileError::Parse(e.to_string()))
}

pub fn compile_str(yaml: &str) -> Result<Vec<u8>, CompileError> {
    compile(&parse_str(yaml)?)
}

fn encode(_t: &Topology) -> Result<Vec<u8>, CompileError> {
    Err(CompileError::EncoderUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desk_demo_validates_then_waits_for_encoder() {
        let yaml = include_str!("../../../topology/desk-demo.yaml");
        assert_eq!(compile_str(yaml), Err(CompileError::EncoderUnavailable));
    }

    #[test]
    fn invalid_topology_is_rejected_before_encoding() {
        let yaml = include_str!("../../../topology/desk-demo.yaml").replace("dagon: 1", "dagon: 2");
        assert!(matches!(compile_str(&yaml), Err(CompileError::Invalid(_))));
    }
}
