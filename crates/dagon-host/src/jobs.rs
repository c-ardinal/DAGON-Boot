//! The jobs `dagon serve` runs for the page, as its API names them: their
//! kinds and states. The page shows its own words for each and reads these
//! lists (through dagon-wasm), so a kind or state added here fails its
//! checks until it has its words; `dagon serve`'s checks hold it to them.

/// Every job kind (`kind` of a job).
pub const KINDS: &[&str] = &[
    "build",
    "sign",
    "release",
    "conform",
    "gen",
    "flash",
    "doctor",
    "ai_port",
    "ai_topology",
];

/// Every job state (`state` of a job), in the order a job goes through
/// them; the last three end it.
pub const STATES: &[&str] = &[
    "queued",
    "running",
    "question",
    "succeeded",
    "failed",
    "cancelled",
];
