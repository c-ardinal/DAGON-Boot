//! Versions this dagon knows: its own, the DAGON components of the source
//! tree it was built from (`compat.toml`), and the format numbers it reads
//! and writes. Rules: docs/versioning.md.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The `dagon serve` HTTP API number (GET /v1/hello `api`).
pub const SERVE_API: u32 = 1;

const COMPAT: &str = include_str!("../../../compat.toml");

/// Each contract and the number this dagon reads and writes.
pub fn formats() -> [(&'static str, u32); 10] {
    use dagon_format::{bundle, header, manifest, statelog, topology};
    [
        ("wire", u32::from(dagon_proto::PROTO_VERSION)),
        ("image-header", u32::from(header::HDR_VERSION)),
        ("state-log", u32::from(statelog::FORMAT_VERSION)),
        ("manifest", u32::from(manifest::MF_VERSION)),
        ("bundle", u32::from(bundle::BN_VERSION)),
        ("topology-bin", u32::from(topology::TP_VERSION)),
        ("topology-yaml", dagon_topology::SCHEMA_VERSION),
        ("definitions", dagon_topology::defs::FORMAT),
        ("serve-api", SERVE_API),
        ("event", u32::from(dagon_catalog::EVENT_FORMAT)),
    ]
}

fn number(contract: &str) -> u32 {
    formats()
        .into_iter()
        .find(|(c, _)| *c == contract)
        .map(|(_, n)| n)
        .expect("a known contract")
}

/// `wire 1, image-header 1, ...`
pub fn formats_text() -> String {
    formats()
        .iter()
        .map(|(c, n)| format!("{c} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `dagon --version` without the banner.
pub fn version_lines() -> String {
    format!(
        "dagon {}\nformats (read and written): {}",
        env!("CARGO_PKG_VERSION"),
        formats_text()
    )
}

// Paths and packages are read by the tests and scripts/bump_check.py.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Compat {
    pub formats: BTreeMap<String, u32>,
    pub component: Vec<Component>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Component {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub packages: Vec<String>,
    pub paths: Vec<String>,
    #[serde(default)]
    pub formats: BTreeMap<String, Vec<u32>>,
}

/// The table this dagon was built with (its tests keep it parsing and
/// matching the code).
pub fn compat() -> Compat {
    toml::from_str(COMPAT).expect("compat.toml parses")
}

/// One line per component: `boot-rp2040 0.1.0`.
pub fn components_text() -> String {
    compat()
        .component
        .iter()
        .map(|c| format!("  {} {}", c.name, c.version))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The header `dagon status` and `dagon doctor` start with.
pub fn known_text() -> String {
    format!(
        "{}\nDAGON components of this dagon's release:\n{}",
        version_lines(),
        components_text()
    )
}

/// A warning when a device speaks a wire protocol the CLI's entry of the
/// table does not list.
pub fn wire_warning(who: &str, proto_ver: u32) -> Option<String> {
    let ours = number("wire");
    let cli = compat().component.into_iter().find(|c| c.name == "cli");
    let speaks = cli.is_some_and(|c| {
        c.formats
            .get("wire")
            .is_some_and(|n| n.contains(&proto_ver))
    });
    (!speaks).then(|| {
        format!(
            "warning: {who} speaks wire protocol {proto_ver}; this dagon {} speaks {ours}. \
             Use a dagon and boot images whose wire numbers match (compat.toml, docs/versioning.md)",
            env!("CARGO_PKG_VERSION")
        )
    })
}

/// Refuses a file of `contract` (magic + u16 format number at byte 4, as
/// every DAGON binary format starts) whose number this dagon does not read,
/// saying which numbers they are. Other faults are left to the parser.
pub fn check_number(contract: &str, what: &str, raw: &[u8]) -> Result<(), String> {
    use dagon_format::{bundle, header, manifest, statelog, topology};
    let magic = match contract {
        "image-header" => header::MAGIC,
        "bundle" => bundle::MAGIC,
        "manifest" => manifest::MAGIC,
        "state-log" => statelog::MAGIC,
        "topology-bin" => topology::MAGIC,
        _ => unreachable!("{contract} has no magic"),
    };
    let (Some(m), Some(b)) = (raw.get(0..4), raw.get(4..6)) else {
        return Ok(());
    };
    if m != magic {
        return Ok(());
    }
    let n = u32::from(u16::from_le_bytes([b[0], b[1]]));
    let ours = number(contract);
    if n == ours {
        return Ok(());
    }
    Err(format!(
        "{what} is {contract} format {n}; this dagon {} reads {contract} format {ours}. \
         Use a dagon release that reads it (docs/versioning.md)",
        env!("CARGO_PKG_VERSION")
    ))
}

#[cfg(test)]
mod tests;
