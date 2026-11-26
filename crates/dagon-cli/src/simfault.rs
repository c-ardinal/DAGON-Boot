//! `sim --fault NODE=KIND`: the faults the simulator injects into one node
//! of a topology, as the command line names them.

/// What `--fault` does to its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Power lost at the node's fourth flash operation of the write.
    PowerCut,
    /// The node's new app starts and never confirms.
    NoConfirm,
    /// One bit of the node's image flipped: header, payload, signature.
    Tamper,
    /// The node's flash fully erased, to rewrite through its ROM loader.
    Erased,
}

pub const FAULTS: [(&str, Fault); 4] = [
    ("power-cut", Fault::PowerCut),
    ("no-confirm", Fault::NoConfirm),
    ("tamper", Fault::Tamper),
    ("erased", Fault::Erased),
];

pub const FAULT_HELP: &str =
    "Fault to inject into a node, by its id; each runs on a fresh simulation \
     (repeatable). KIND: power-cut (power lost during the write), no-confirm (the new app never \
     confirms), tamper (one bit of its image flipped), erased (flash erased; rewritten through \
     its ROM loader)";

impl Fault {
    pub fn name(self) -> &'static str {
        FAULTS
            .iter()
            .find(|(_, f)| *f == self)
            .map_or("", |(n, _)| n)
    }
}

/// One `--fault NODE=KIND`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Injection {
    pub node: String,
    pub fault: Fault,
}

impl Injection {
    pub fn parse(s: &str) -> Result<Injection, String> {
        let known = || {
            FAULTS
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (node, kind) = s
            .split_once('=')
            .filter(|(n, _)| !n.is_empty())
            .ok_or_else(|| format!("expected NODE=KIND (KIND: {})", known()))?;
        let fault = FAULTS
            .iter()
            .find(|(n, _)| *n == kind)
            .map(|(_, f)| *f)
            .ok_or_else(|| format!("no fault `{kind}` (there is {})", known()))?;
        Ok(Injection {
            node: node.to_string(),
            fault,
        })
    }
}

/// Every scenario `dagon sim --scenario` takes: its ID and title, in run
/// order (`simrun` runs them).
pub const SCENARIOS: [(&str, &str); 4] = [
    ("S1", "update with a power cut"),
    ("S2", "trial app never confirms"),
    ("S3", "tampered release"),
    ("S4", "erased F401 via ROM loader"),
];
