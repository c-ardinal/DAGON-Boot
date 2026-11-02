#![forbid(unsafe_code)]
//! Topology YAML: schema, validation rules and the supported-board table.
//! Shared by the CLI and the browser build so both apply the same rules.

pub mod boards;
mod check;
mod report;
pub mod schema;

pub use check::{
    check, check_str, effective_tiers, node_board, rom_forced_unavailable, SCHEMA_VERSION,
};
pub use report::{Issue, Report, Severity};
pub use schema::Topology;
