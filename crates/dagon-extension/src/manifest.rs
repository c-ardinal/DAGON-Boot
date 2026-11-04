//! `dagon-extension.toml`: what a plug-in is, what it replaces, observes
//! and answers, and what it needs. Checked against the port catalog, the
//! event catalogs and the decision points.

use std::fmt;

use dagon_catalog::{Layer, Trust};
use serde::Deserialize;

use crate::Known;

/// The `extension-manifest` format number this reader reads.
pub const MANIFEST_FORMAT: u32 = 1;
/// The manifest's file name in an extension's package folder.
pub const MANIFEST_FILE: &str = "dagon-extension.toml";
/// Longest id (VERSION carries it in one TLV with room to spare).
pub const ID_MAX: usize = 32;
/// Smallest stack a sandboxed handler gets (the exception frame and a
/// call or two).
pub const STACK_MIN: u32 = 256;

/// What DAGON hands the extension, nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// Board `[[peripheral]]` ids: windows onto window-eligible blocks.
    #[serde(default)]
    pub peripherals: Vec<String>,
    /// Board `[[pin]]` names, reached through the pin service.
    #[serde(default)]
    pub pins: Vec<String>,
    /// Board `[[bus]]` ids, reached through the bus service.
    #[serde(default)]
    pub buses: Vec<String>,
    /// The time service.
    #[serde(default)]
    pub timer: bool,
}

/// Budgets: per call, and the memory the extension's regions hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Per handler call.
    pub max_run_us: u32,
    /// Required (at least [`STACK_MIN`]) for a sandboxed extension; a
    /// trusted one runs on DAGON's stack.
    #[serde(default)]
    pub stack_bytes: u32,
    /// `.ext.data` + `.ext.bss`.
    #[serde(default)]
    pub ram_bytes: u32,
    /// `.ext.text` + `.ext.rodata` + the `.ext.data` load image.
    #[serde(default)]
    pub text_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    format: u32,
    id: String,
    layers: Vec<String>,
    #[serde(default)]
    provides: Vec<String>,
    #[serde(default)]
    observes: Vec<String>,
    #[serde(default)]
    decides: Vec<String>,
    #[serde(default)]
    trusted: bool,
    #[serde(default)]
    capabilities: Capabilities,
    #[serde(default)]
    limits: Limits,
}

/// A manifest that parsed and passed every catalog check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub layers: Vec<Layer>,
    pub provides: Vec<String>,
    pub observes: Vec<String>,
    pub decides: Vec<String>,
    pub trusted: bool,
    pub capabilities: Capabilities,
    pub limits: Limits,
}

/// A manifest that does not load: which, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    pub at: String,
    pub msg: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.msg)
    }
}

impl std::error::Error for ManifestError {}

fn layer(name: &str) -> Option<Layer> {
    Layer::ALL.into_iter().find(|l| l.name() == name)
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

/// Whether `id` is `a-z 0-9 -`, 1 to [`ID_MAX`] bytes, not starting with `-`.
pub fn valid_id(id: &str) -> bool {
    (1..=ID_MAX).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Manifest {
    /// Parses and checks `text` (`at` names it in errors).
    pub fn parse(text: &str, at: &str, known: &Known) -> Result<Manifest, ManifestError> {
        let err = |msg: String| ManifestError {
            at: at.to_string(),
            msg,
        };
        // The number first: a newer format may have keys this one refuses.
        let format = toml::from_str::<toml::Table>(text)
            .map_err(|e| err(e.to_string()))?
            .get("format")
            .cloned();
        match format {
            Some(toml::Value::Integer(n)) if n == i64::from(MANIFEST_FORMAT) => {}
            Some(v) => {
                return Err(err(format!(
                    "extension manifest format {v}; this dagon reads format {MANIFEST_FORMAT}"
                )))
            }
            None => return Err(err("no `format` key".into())),
        }
        let raw: Raw = toml::from_str(text).map_err(|e| err(e.to_string()))?;
        Self::check(raw, known).map_err(err)
    }

    fn check(raw: Raw, known: &Known) -> Result<Manifest, String> {
        fail(!valid_id(&raw.id), || {
            format!("id '{}' must be 1..={ID_MAX} of a-z 0-9 -", raw.id)
        })?;
        let mut layers = Vec::new();
        for name in &raw.layers {
            let l = layer(name)
                .ok_or_else(|| format!("layer '{name}' is none of boot, app, relay, host"))?;
            fail(layers.contains(&l), || {
                format!("layer '{name}' is listed twice")
            })?;
            layers.push(l);
        }
        fail(layers.is_empty(), || "layers is empty".into())?;
        let device = layers.iter().any(|l| *l != Layer::Host);
        for p in &raw.provides {
            let port = known
                .port(p)
                .ok_or_else(|| format!("provides '{p}': no such port"))?;
            if let Some(l) = layers.iter().find(|l| !port.in_layer(**l)) {
                return Err(format!(
                    "provides '{p}': the port does not exist in layer {}",
                    l.name()
                ));
            }
            fail(port.trust == Trust::TrustedOnly && !raw.trusted, || {
                format!("provides '{p}': a trusted-only port; the manifest must say trusted = true")
            })?;
            fail(raw.provides.iter().filter(|q| *q == p).count() > 1, || {
                format!("provides '{p}' twice")
            })?;
        }
        for o in &raw.observes {
            known.observable(o)?;
        }
        for d in &raw.decides {
            let point = known
                .decision(d)
                .ok_or_else(|| format!("decides '{d}': no such decision point"))?;
            fail(!layers.contains(&point.layer), || {
                format!(
                    "decides '{d}': it is asked in layer {}, which the manifest does not list",
                    point.layer.name()
                )
            })?;
            fail(raw.trusted, || {
                format!(
                    "decides '{d}': a trusted extension is part of DAGON and answers no decision"
                )
            })?;
        }
        let l = raw.limits;
        // Every handler call on a device is timed against `max_run_us`, a
        // trusted observer's too; a trusted port is DAGON's own code path.
        if device && (!raw.trusted || !raw.observes.is_empty()) {
            fail(
                l.max_run_us == 0 || l.max_run_us > crate::EXT_CALL_MAX_US,
                || {
                    format!(
                        "limits.max_run_us {} must be 1..={}",
                        l.max_run_us,
                        crate::EXT_CALL_MAX_US
                    )
                },
            )?;
        }
        if device && !raw.trusted {
            fail(l.stack_bytes < STACK_MIN, || {
                format!("limits.stack_bytes {} is below {STACK_MIN}", l.stack_bytes)
            })?;
            fail(l.text_bytes == 0, || {
                "limits.text_bytes is required for a device extension".into()
            })?;
        }
        Ok(Manifest {
            id: raw.id,
            layers,
            provides: raw.provides,
            observes: raw.observes,
            decides: raw.decides,
            trusted: raw.trusted,
            capabilities: raw.capabilities,
            limits: raw.limits,
        })
    }

    pub fn in_layer(&self, layer: Layer) -> bool {
        self.layers.contains(&layer)
    }

    /// Whether it runs sandboxed (with its own map) where the level allows.
    pub fn sandboxed(&self) -> bool {
        !self.trusted
    }
}
