//! DAGON extensions on the host side: the manifest (`dagon-extension.toml`)
//! and its checks against the port catalog, the event catalogs and the
//! decision points ([`manifest`]); a node's set of extensions checked
//! against its MCU and board definitions ([`node`]); the protection-kind
//! map generators behind one trait ([`kind`]); and the files `dagon gen`
//! writes from them ([`gen`]).
#![forbid(unsafe_code)]

pub mod gen;
pub mod kind;
pub mod manifest;
pub mod node;

use dagon_catalog::{Catalog, DecisionPoint, Layer, Port};
// Linked for its std implementation of the critical section.
use critical_section as _;

pub use manifest::{Manifest, ManifestError, MANIFEST_FILE, MANIFEST_FORMAT};

/// Longest a sandboxed handler call may run, in any layer: the decision
/// bound, which notifications share.
pub const EXT_CALL_MAX_US: u32 = dagon_catalog::DECISION_BOUND_MS * 1000;

/// Share of the trial watchdog period the boot extensions' calls may take
/// together (`1/BOOT_SHARE`).
pub const BOOT_SHARE: u32 = 4;

/// One event kind a manifest may observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventKind {
    pub layer: Layer,
    pub kind: &'static str,
    /// Its number within its layer (`EventRecord::kind`).
    pub code: u8,
    /// Delivered to extensions as it happens.
    pub live: bool,
    /// A boot action program may run on it.
    pub action: bool,
}

/// What manifests and boot programs are checked against: the event kinds
/// of every machine, the ports, the decision points.
#[derive(Debug, Clone)]
pub struct Known {
    pub kinds: Vec<EventKind>,
    pub ports: &'static [Port],
    pub decisions: &'static [DecisionPoint],
}

impl Known {
    /// DAGON's catalogs ([`dagon_catalog::machines`], `PORTS`, `DECISIONS`).
    pub fn dagon() -> Known {
        Known::from_catalogs(&dagon_catalog::machines())
    }

    pub fn from_catalogs(machines: &[Catalog]) -> Known {
        let kinds = machines
            .iter()
            .flat_map(|m| {
                m.entries
                    .iter()
                    .filter(|e| !e.flags.retired)
                    .map(|e| EventKind {
                        layer: m.layer,
                        kind: e.kind,
                        code: e.code,
                        live: e.observe.live,
                        action: e.flags.action,
                    })
            })
            .collect();
        Known {
            kinds,
            ports: dagon_catalog::PORTS,
            decisions: dagon_catalog::DECISIONS,
        }
    }

    /// The port named `id` (not the parts that are not ports).
    pub fn port(&self, id: &str) -> Option<&'static Port> {
        self.ports.iter().find(|p| p.id == id && p.trust.is_port())
    }

    pub fn decision(&self, id: &str) -> Option<&'static DecisionPoint> {
        self.decisions.iter().find(|d| d.id == id)
    }

    /// Kinds `pattern` names: `layer.kind`, `layer.prefix*` or `layer.*`.
    pub fn matching(&self, pattern: &str) -> Vec<&EventKind> {
        let Some((layer, kind)) = pattern.split_once('.') else {
            return Vec::new();
        };
        self.kinds
            .iter()
            .filter(|k| k.layer.name() == layer)
            .filter(|k| match kind.strip_suffix('*') {
                Some(prefix) => k.kind.starts_with(prefix),
                None => k.kind == kind,
            })
            .collect()
    }

    /// Checks one `observes` entry: a known layer with `.*`, or a pattern
    /// that names at least one kind of the catalogs.
    pub fn observable(&self, pattern: &str) -> Result<(), String> {
        let (layer, kind) = pattern.split_once('.').ok_or_else(|| {
            format!("observes '{pattern}': write layer.kind, layer.prefix* or layer.*")
        })?;
        if !Layer::ALL.iter().any(|l| l.name() == layer) {
            return Err(format!("observes '{pattern}': no layer '{layer}'"));
        }
        let star = kind.find('*');
        if star.is_some_and(|i| i + 1 != kind.len()) {
            return Err(format!("observes '{pattern}': '*' only at the end"));
        }
        if kind == "*" || !self.matching(pattern).is_empty() {
            return Ok(());
        }
        Err(format!(
            "observes '{pattern}': no event of the catalog matches"
        ))
    }
}

#[cfg(test)]
mod tests;
