//! A node's extensions for one image, checked against the node's MCU and
//! board definitions and planned into protection maps.

use dagon_catalog::{Layer, Trust};
use dagon_topology::defs::isolation::Level;
use dagon_topology::defs::{Binding, BoardDef, ImageKind, McuDef};

use crate::kind::{self, ExtLayout, Map, Space};
use crate::{Known, Manifest, BOOT_SHARE};

/// One extension of a node's chain, in chain order.
#[derive(Debug, Clone)]
pub struct Ext<'a> {
    pub manifest: &'a Manifest,
    /// The Rust crate (library name) that provides it.
    pub krate: String,
    /// The SHA-256 of its built artifact the project accepts (trusted
    /// extensions only), 64 hex digits.
    pub sha256: Option<String>,
}

/// The image a set of extensions is linked into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Image {
    /// The boot core (layer `boot`).
    Bootcore,
    /// The app (layers `app` and, on a coordinator, `relay`).
    App { relay: bool },
}

impl Image {
    pub fn kind(self) -> ImageKind {
        match self {
            Image::Bootcore => ImageKind::Bootcore,
            Image::App { .. } => ImageKind::App,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Image::Bootcore => "boot",
            Image::App { .. } => "app",
        }
    }

    fn takes(self, m: &Manifest) -> bool {
        match self {
            Image::Bootcore => m.in_layer(Layer::Boot),
            Image::App { relay } => m.in_layer(Layer::App) || (relay && m.in_layer(Layer::Relay)),
        }
    }
}

/// The plan for one image of a node.
#[derive(Debug, Clone)]
pub struct ImagePlan<'a> {
    pub image: Image,
    pub exts: Vec<Ext<'a>>,
    /// The level the MCU definition enforces.
    pub level: Level,
    /// One map per slot of the image (none at level `none` or without
    /// sandboxed extensions).
    pub maps: Vec<(Binding, Map)>,
    /// The image's flash body and RAM per slot after the extension
    /// regions (also without extensions), when the MCU defines its RAM.
    pub memory: Vec<(Binding, Space)>,
    /// What the user should know (unguarded extensions).
    pub notes: Vec<String>,
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

/// The RAM an image may use: the MCU's RAM below the boot record and the
/// fuse cells.
pub fn image_ram(def: &McuDef) -> Option<(u32, u32)> {
    let ram = def.ram?;
    let end = u64::from(ram.at) + u64::from(ram.size);
    let kept = def
        .record()
        .into_iter()
        .flat_map(|r| std::iter::once(r.at).chain(r.fuse.cells.iter().map(|c| c.at)))
        .map(u64::from)
        .filter(|&a| a >= u64::from(ram.at) && a < end)
        .min()
        .unwrap_or(end);
    Some((ram.at, (kept - u64::from(ram.at)) as u32))
}

/// The extension and board checks that hold at every level.
fn check_set(
    def: &McuDef,
    board: Option<&BoardDef>,
    image: Image,
    exts: &[Ext<'_>],
    known: &Known,
) -> Result<(), String> {
    if image == Image::Bootcore {
        fail(!def.isolation().boot_extensions && !exts.is_empty(), || {
            format!("{} has no DAGON boot stage that runs extensions", def.id)
        })?;
    }
    for (i, e) in exts.iter().enumerate() {
        let m = e.manifest;
        fail(!image.takes(m), || {
            format!(
                "extension {} is not for the {} image (layers: {})",
                m.id,
                image.name(),
                m.layers
                    .iter()
                    .map(|l| l.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        fail(exts[..i].iter().any(|o| o.manifest.id == m.id), || {
            format!("extension {} is listed twice", m.id)
        })?;
        for p in &m.provides {
            if let Some(o) = exts[..i].iter().find(|o| o.manifest.provides.contains(p)) {
                return Err(format!(
                    "port {p} is provided by both {} and {}",
                    o.manifest.id, m.id
                ));
            }
        }
        let trusted_port = m
            .provides
            .iter()
            .any(|p| known.port(p).is_some_and(|x| x.trust == Trust::TrustedOnly));
        if m.trusted || trusted_port {
            let ok = e
                .sha256
                .as_deref()
                .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
            fail(!ok, || {
                format!("trusted extension {} needs its artifact SHA-256 (64 hex digits) in the project's trusted list", m.id)
            })?;
        }
        capabilities(def, board, m)?;
    }
    Ok(())
}

fn capabilities(def: &McuDef, board: Option<&BoardDef>, m: &Manifest) -> Result<(), String> {
    let c = &m.capabilities;
    if c.peripherals.is_empty() && c.pins.is_empty() && c.buses.is_empty() {
        return Ok(());
    }
    let b = board.ok_or_else(|| {
        format!("extension {} declares capabilities, but the node names no board to declare them against", m.id)
    })?;
    for p in &c.peripherals {
        let bp = b.board_peripheral(p).ok_or_else(|| {
            format!(
                "extension {}: peripheral {p} is not in board {}'s [[peripheral]]",
                m.id, b.id
            )
        })?;
        let mp = def
            .peripheral(&bp.mcu)
            .ok_or_else(|| format!("board {}: {} is no block of {}", b.id, bp.mcu, def.id))?;
        fail(!m.trusted && !mp.window_eligible, || {
            format!(
                "extension {}: peripheral {p} ({} on {}) is not window-eligible",
                m.id, mp.id, def.id
            )
        })?;
    }
    for p in &c.pins {
        fail(b.pin(p).is_none(), || {
            format!(
                "extension {}: pin {p} is not in board {}'s [[pin]]",
                m.id, b.id
            )
        })?;
    }
    for x in &c.buses {
        fail(b.bus(x).is_none(), || {
            format!(
                "extension {}: bus {x} is not in board {}'s [[bus]]",
                m.id, b.id
            )
        })?;
    }
    Ok(())
}

/// Boot calls of `m`: its live boot events, one per provided port, at least 1.
fn boot_calls(m: &Manifest, known: &Known) -> u64 {
    let events = m
        .observes
        .iter()
        .flat_map(|o| known.matching(o))
        .filter(|k| k.layer == Layer::Boot && k.live)
        .count();
    (events + m.provides.len()).max(1) as u64
}

fn budgets(
    def: &McuDef,
    image: Image,
    sandboxed: &[&Manifest],
    known: &Known,
) -> Result<(), String> {
    let i = def.isolation();
    if i.level != Level::None {
        let need: u64 = sandboxed
            .iter()
            .map(|m| u64::from(m.limits.stack_bytes) + u64::from(m.limits.ram_bytes))
            .sum();
        let max = i.ext_ram_max.unwrap_or(0);
        fail(need > u64::from(max), || {
            format!(
                "extensions declare {need} bytes of stack and RAM; ext_ram_max of {} is {max}",
                def.id
            )
        })?;
    }
    if image != Image::Bootcore || sandboxed.is_empty() {
        return Ok(());
    }
    let w = def.watchdog.ok_or_else(|| {
        format!(
            "{} has no [watchdog]: boot extensions have no time budget",
            def.id
        )
    })?;
    let budget = u64::from(w.trial_ms) * 1000 / u64::from(BOOT_SHARE);
    let need: u64 = sandboxed
        .iter()
        .map(|m| u64::from(m.limits.max_run_us) * boot_calls(m, known))
        .sum();
    fail(need > budget, || {
        format!(
            "boot extensions may take {need} us per boot; 1/{BOOT_SHARE} of {}'s {} ms trial watchdog is {budget} us",
            def.id, w.trial_ms
        )
    })
}

/// The image's slots' spaces (flash body; RAM below what DAGON keeps).
fn spaces(def: &McuDef, image: Image) -> Vec<(Binding, Space)> {
    let Some((ram_at, ram_size)) = image_ram(def) else {
        return Vec::new();
    };
    def.slots
        .iter()
        .filter(|s| s.image == image.kind())
        .map(|s| {
            (
                s.binding,
                Space {
                    flash_at: s.load_addr,
                    flash_size: s.max_payload,
                    ram_at,
                    ram_size,
                },
            )
        })
        .collect()
}

/// Checks `exts` (chain order) for `image` of a node on `def` / `board`,
/// and plans the protection map of every slot the image has.
pub fn plan<'a>(
    def: &McuDef,
    board: Option<&BoardDef>,
    image: Image,
    exts: Vec<Ext<'a>>,
    known: &Known,
) -> Result<ImagePlan<'a>, String> {
    check_set(def, board, image, &exts, known)?;
    let sandboxed: Vec<&Manifest> = exts
        .iter()
        .map(|e| e.manifest)
        .filter(|m| m.sandboxed())
        .collect();
    budgets(def, image, &sandboxed, known)?;
    let level = def.isolation().level;
    let mut notes = Vec::new();
    let mut memory = spaces(def, image);
    let mut maps = Vec::new();
    if level == Level::None {
        if let Some(m) = sandboxed.first() {
            notes.push(format!(
                "extensions unguarded: {} isolates nothing (level none), so {} and the others run with DAGON's rights",
                def.id, m.id
            ));
        }
    } else if !sandboxed.is_empty() {
        let k = kind::for_kind(def.isolation().kind).ok_or_else(|| {
            format!(
                "{} declares level {} on kind {}, which has no map generator",
                def.id,
                level.as_str(),
                def.isolation().kind.as_str()
            )
        })?;
        let layouts = layouts(def, board, &sandboxed);
        for (binding, space) in &mut memory {
            let map = k.plan(def, space, &layouts).map_err(|r| r.0)?;
            let broken = k.check(def, &map);
            fail(!broken.is_empty(), || {
                format!(
                    "the planned map breaks its kind's rules: {}",
                    broken
                        .iter()
                        .map(|v| v.0.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            })?;
            *space = Space {
                flash_at: map.image_flash.0,
                flash_size: map.image_flash.1,
                ram_at: map.image_ram.0,
                ram_size: map.image_ram.1,
            };
            maps.push((*binding, map));
        }
    }
    Ok(ImagePlan {
        image,
        exts,
        level,
        maps,
        memory,
        notes,
    })
}

fn layouts(def: &McuDef, board: Option<&BoardDef>, sandboxed: &[&Manifest]) -> Vec<ExtLayout> {
    sandboxed
        .iter()
        .map(|m| ExtLayout {
            id: m.id.clone(),
            text_bytes: m.limits.text_bytes,
            ram_bytes: m.limits.ram_bytes,
            stack_bytes: m.limits.stack_bytes,
            windows: m
                .capabilities
                .peripherals
                .iter()
                .filter_map(|p| board?.board_peripheral(p))
                .filter(|bp| def.peripheral(&bp.mcu).is_some())
                .map(|bp| bp.mcu.clone())
                .collect(),
        })
        .collect()
}
