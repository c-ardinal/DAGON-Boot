#![forbid(unsafe_code)]
//! Topology YAML: schema, validation rules and the supported-board table.
//! Shared by the CLI and the browser build so both apply the same rules.

mod check;
mod compile;
pub mod defs;
pub mod links;
mod reach;
mod report;
pub mod schema;

pub use reach::{cut_with, updatable, Cut, Skip};

pub use check::{
    check, check_str, check_str_with, check_with, effective_tiers, rom_forced_unavailable,
    SCHEMA_VERSION,
};
pub use compile::{
    compile, compile_str, compile_with, mcu_id_of, parse_str, parse_version, port_kind_of,
    CompileError,
};
pub use report::{Issue, Report, Severity};
pub use schema::Topology;
