//! Protection kinds: one generator and checker per kind of protection
//! hardware, behind [`ProtectionKind`]. Adding an MCU of a known kind is a
//! definition; adding a kind is one module here and one row of the
//! definition schema's kind table.

mod armv6m;
mod armv7m;
mod pmsa;

use std::fmt;

use dagon_topology::defs::isolation::{Kind, Level, Unwind};
use dagon_topology::defs::McuDef;

pub use armv6m::Armv6mMpu;
pub use armv7m::Armv7mMpu;

/// One sandboxed extension as the planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtLayout {
    pub id: String,
    pub text_bytes: u32,
    pub ram_bytes: u32,
    pub stack_bytes: u32,
    /// MCU `[[peripheral]]` ids the extension gets as windows.
    pub windows: Vec<String>,
}

/// Where an image's extension regions may go: the image's flash body and
/// the RAM below what DAGON keeps for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub flash_at: u32,
    pub flash_size: u32,
    pub ram_at: u32,
    pub ram_size: u32,
}

/// Access an unprivileged handler has to a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "ro",
            Self::ReadWrite => "rw",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// `.ext.text`, `.ext.rodata` and the data load image.
    Text,
    /// `.ext.data` and `.ext.bss`.
    Data,
    /// The stack, its lowest part a guard.
    Stack,
    /// A declared peripheral.
    Window(String),
}

impl fmt::Display for Purpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => f.write_str("text"),
            Self::Data => f.write_str("data"),
            Self::Stack => f.write_str("stack"),
            Self::Window(p) => write!(f, "window {p}"),
        }
    }
}

/// One protection region loaded for an extension's calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub number: u8,
    pub base: u32,
    pub size: u32,
    /// Subregions disabled (bit n: the n-th eighth), 0 for none.
    pub disabled: u8,
    pub access: Access,
    pub execute: bool,
    pub purpose: Purpose,
}

impl Region {
    pub fn end(&self) -> u64 {
        u64::from(self.base) + u64::from(self.size)
    }

    /// The enabled address ranges (all of it, less disabled subregions).
    pub fn enabled(&self) -> Vec<(u64, u64)> {
        if self.disabled == 0 {
            return vec![(u64::from(self.base), self.end())];
        }
        let sub = u64::from(self.size / 8);
        (0..8)
            .filter(|i| self.disabled & (1 << i) == 0)
            .map(|i| {
                let at = u64::from(self.base) + i * sub;
                (at, at + sub)
            })
            .collect()
    }
}

/// The regions of one extension, loaded at each of its calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtMap {
    pub id: String,
    pub regions: Vec<Region>,
}

/// A planned map: every sandboxed extension's regions, and what is left
/// of the image's flash body and RAM for DAGON and the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    pub kind: Kind,
    pub exts: Vec<ExtMap>,
    /// The image's own flash and RAM after the extension regions.
    pub image_flash: (u32, u32),
    pub image_ram: (u32, u32),
}

/// Why a set of extensions has no map on this MCU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A rule a map breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub String);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a level guarantees on this kind, and what it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guarantees {
    pub level: Level,
    pub guaranteed: &'static [&'static str],
    pub not_guaranteed: &'static [&'static str],
}

/// A register the MCU package reads at boot and in the conformance build,
/// and the value the definition's capability record says it holds; a
/// mismatch turns the level to none (`boot.isolation_mismatch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadBack {
    pub register: &'static str,
    pub addr: u32,
    pub mask: u32,
    pub shift: u8,
    pub expect: u32,
}

/// One kind of protection hardware.
pub trait ProtectionKind {
    fn kind(&self) -> Kind;

    /// Places each extension's code, data and stack where the kind's
    /// alignment rules allow and assigns its windows, or refuses with a
    /// message that names what does not fit.
    fn plan(&self, def: &McuDef, space: &Space, exts: &[ExtLayout]) -> Result<Map, Refusal>;

    /// Every rule `map` breaks against `def` (empty when it holds):
    /// alignment, overlap, attributes, privileged blocks outside every
    /// allowed range, the fixed regions present.
    fn check(&self, def: &McuDef, map: &Map) -> Vec<Violation>;

    fn unwind(&self) -> Unwind;

    fn guarantees(&self) -> Guarantees;

    /// The registers that confirm `def`'s capability record.
    fn read_back(&self, def: &McuDef) -> Vec<ReadBack>;
}

/// The generator of `kind`, when DAGON has one.
pub fn for_kind(kind: Kind) -> Option<&'static dyn ProtectionKind> {
    match kind {
        Kind::Armv6mMpu => Some(&Armv6mMpu),
        Kind::Armv7mMpu => Some(&Armv7mMpu),
        _ => None,
    }
}

/// The MPU guarantees (`mpu` level) shared by the PMSA kinds.
pub(crate) const MPU_GUARANTEED: &[&str] = &[
    "unprivileged execution of extension handlers",
    "DAGON's code and data denied",
    "flash controller, watchdog, state log and boot record denied",
    "bus masters, clock, reset and interrupt routing denied",
    "service calls validated",
    "only declared windows reachable",
    "timer preemption and unwinding",
];
