#![forbid(unsafe_code)]
//! Topology YAML: schema, validation rules and the supported-board table.
//! Shared by the CLI and the browser build so both apply the same rules.

mod check;
mod compile;
pub mod defs;
pub mod links;
pub mod project;
mod reach;
mod report;
pub mod schema;

pub use reach::{cut_with, reach_root, updatable, Cut, Skip};

pub use check::{
    check_str_with, check_with, default_port, effective_tiers, pin_fault, rom_forced_unavailable,
    uart0_choices, SCHEMA_VERSION,
};
pub use compile::{
    compile, compile_str, compile_with, mcu_id_of, parse_str, parse_version, port_kind_of,
    recovery_peer_mcu, CompileError,
};
pub use report::{Issue, Report, Severity, CHECK_JSON_FORMAT, RULES};
pub use schema::Topology;
