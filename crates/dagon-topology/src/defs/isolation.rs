//! What an MCU definition says about protecting DAGON from extensions:
//! `[isolation]` (the protection hardware kind, its capability record, the
//! level DAGON enforces), every register block (`[[peripheral]]`), every
//! core (`[[core]]`), the watchdog (`[watchdog]`), the boot record and
//! lockup fuse placement (`[boot.record]`), and the checks a definition
//! must pass for a level above `none`. The kinds and their rules are one
//! table here ([`KIND_RULES`]); a new kind is one row and one generator
//! module in `dagon-extension`.

use serde::Deserialize;

use super::{ImageKind, McuDef, PortKind, Region};

/// The level of protection DAGON enforces on an MCU.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// Time measured, events recorded; no hardware denial.
    #[default]
    None,
    /// A memory protection unit or RISC-V PMP.
    Mpu,
    /// A permission controller with worlds.
    Pms,
    /// The Arm Security Extension.
    Tz,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Mpu => "mpu",
            Self::Pms => "pms",
            Self::Tz => "tz",
        }
    }
}

/// The protection hardware kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Armv6mMpu,
    Armv7mMpu,
    Armv8mMpu,
    Armv8mTz,
    RiscvPmp,
    RiscvEpmp,
    EspPms,
    #[default]
    None,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        rule(self).name
    }
}

/// How a non-preemptible part masks the sandbox timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CriticalSection {
    Primask,
    Basepri,
    MstatusMie,
    XtensaLevel,
}

/// How a call that ended abnormally is unwound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unwind {
    CortexMException,
    RiscvTrap,
    XtensaWorld,
    Armv8mTz,
}

impl Unwind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CortexMException => "cortex-m-exception",
            Self::RiscvTrap => "riscv-trap",
            Self::XtensaWorld => "xtensa-world",
            Self::Armv8mTz => "armv8m-tz",
        }
    }
}

/// The timer that preempts a sandboxed call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Timer {
    Systick,
    RiscvMtime,
    TimerGroup,
}

/// How the reset-time contract reinitialises what a call could touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResetCleanup {
    PeripheralReset,
    Reinit,
}

/// The MCU's protection of its root image, as `dagon provision` offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootProtection {
    /// STM32 readout protection level 1: undone by a mass erase.
    Rdp1,
    /// STM32 readout protection level 2: permanent.
    Rdp2,
    /// STM32 sector write protection: undone through the option bytes.
    Wrp,
    /// Espressif secure boot eFuses: permanent.
    EspSecureBoot,
    /// Espressif flash encryption eFuses: permanent.
    EspFlashEncryption,
}

impl RootProtection {
    /// Whether the level can be undone.
    pub fn reversible(self) -> bool {
        matches!(self, Self::Rdp1 | Self::Wrp)
    }
}

/// Caches in front of the core (window and bounce-buffer attributes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cache {
    #[default]
    None,
    /// An instruction cache.
    ICache,
    /// Instruction and data caches.
    IdCache,
}

/// One entry of an `armv8m-tz` implementation-defined attribution table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Idau {
    pub base: u32,
    pub size: u32,
    pub attribution: Attribution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Attribution {
    Secure,
    NonSecure,
    NonSecureCallable,
}

/// The kind's capability record: what the protection unit implements,
/// read back from the hardware at boot and in the conformance build. Each
/// kind uses its own fields ([`KIND_RULES`]); the others stay out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    // armv6m-mpu, armv7m-mpu, armv8m-mpu
    pub mpu_regions: Option<u32>,
    pub min_region: Option<u32>,
    pub subregions_from: Option<u32>,
    // armv8m-tz
    pub sau_regions: Option<u32>,
    pub s_mpu_regions: Option<u32>,
    pub ns_mpu_regions: Option<u32>,
    #[serde(default)]
    pub idau: Vec<Idau>,
    // riscv-pmp, riscv-epmp
    pub pmp_entries: Option<u32>,
    pub grain: Option<u32>,
    pub na4: Option<bool>,
    pub tor: Option<bool>,
    // esp-pms
    pub worlds: Option<u32>,
    pub memory_regions: Option<u32>,
    pub peripheral_slots: Option<u32>,
    pub gdma_channels: Option<u32>,
    // every kind
    pub critical_section: Option<CriticalSection>,
    pub np_max_us: Option<u32>,
    pub dma_stop_max_us: Option<u32>,
    pub reset_cleanup: Option<ResetCleanup>,
    pub background_privileged: Option<bool>,
    pub unwind: Option<Unwind>,
    pub timer: Option<Timer>,
    pub fault_address: Option<bool>,
}

/// `[isolation]`. Left out, the MCU is `kind = "none"`, `level = "none"`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Isolation {
    #[serde(default)]
    pub level: Level,
    #[serde(default)]
    pub kind: Kind,
    /// The MCU has a DAGON boot stage that runs extensions.
    #[serde(default)]
    pub boot_extensions: bool,
    /// Most RAM all sandboxed extensions of one image may take (data,
    /// bss and stacks).
    pub ext_ram_max: Option<u32>,
    #[serde(default)]
    pub root_protection: Vec<RootProtection>,
    #[serde(default)]
    pub cache: Cache,
    #[serde(default)]
    pub tcm: Vec<Region>,
    #[serde(default)]
    pub capabilities: Capabilities,
}

/// Why a register block is never an extension's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    Dma,
    Flash,
    Watchdog,
    Clock,
    Reset,
    IrqRouting,
    Pinmux,
    Power,
    Rom,
    CoreControl,
    Protection,
    Shared,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dma => "dma",
            Self::Flash => "flash",
            Self::Watchdog => "watchdog",
            Self::Clock => "clock",
            Self::Reset => "reset",
            Self::IrqRouting => "irq-routing",
            Self::Pinmux => "pinmux",
            Self::Power => "power",
            Self::Rom => "rom",
            Self::CoreControl => "core-control",
            Self::Protection => "protection",
            Self::Shared => "shared",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(Why),
    Many(Vec<Why>),
}

fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Why>, D::Error> {
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(w) => vec![w],
        OneOrMany::Many(v) => v,
    })
}

/// One register block of the MCU (`[[peripheral]]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripheral {
    pub id: String,
    pub base: u32,
    pub size: u32,
    /// Self-contained: cannot write memory, route interrupts or clocks,
    /// and shares nothing with DAGON. Only these may be a window.
    #[serde(default)]
    pub window_eligible: bool,
    /// Can write memory.
    #[serde(default)]
    pub bus_master: bool,
    /// Why it stays privileged (one class or a list).
    #[serde(default, deserialize_with = "one_or_many")]
    pub privileged_why: Vec<Why>,
}

impl Peripheral {
    pub fn end(&self) -> u64 {
        u64::from(self.base) + u64::from(self.size)
    }
}

/// One processor core (`[[core]]`); DAGON runs on the first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Core {
    pub id: String,
}

/// The watchdog's driver kind (the `WatchdogDriver` port's default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WatchdogKind {
    Iwdg,
    RpWatchdog,
    RtcWdt,
}

impl WatchdogKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Iwdg => "iwdg",
            Self::RpWatchdog => "rp-watchdog",
            Self::RtcWdt => "rtc-wdt",
        }
    }
}

/// `[watchdog]`: the periods DAGON arms on trial and once confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watchdog {
    pub kind: WatchdogKind,
    pub trial_ms: u32,
    pub confirmed_ms: u32,
}

impl Watchdog {
    /// The longest an app may delay a requested reboot: half the shorter
    /// period.
    pub fn reboot_delay_max_ms(&self) -> u32 {
        self.trial_ms.min(self.confirmed_ms) / 2
    }
}

/// Where the boot record lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordKind {
    /// RAM no image initialises.
    NoinitRam,
    /// Registers that keep their value over a reset.
    RetainedRegisters,
}

impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoinitRam => "noinit-ram",
            Self::RetainedRegisters => "retained-registers",
        }
    }
}

/// What the record's bytes are after a power loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Survives {
    /// Random (SRAM).
    ResetRandom,
    /// Zero (registers cleared at power-on).
    ResetCleared,
    /// Kept (battery-backed).
    Power,
}

impl Survives {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResetRandom => "reset-random",
            Self::ResetCleared => "reset-cleared",
            Self::Power => "power",
        }
    }
}

/// One writer's ring of the boot record, in boot order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ring {
    pub writer: ImageKind,
    pub at: u32,
    pub size: u32,
}

/// One lockup fuse cell: the image kind whose sandboxed calls it guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FuseCell {
    pub image: ImageKind,
    pub at: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fuse {
    pub cells: Vec<FuseCell>,
}

/// `[boot.record]`: the boot record region, its rings and the fuse cells.
/// Their byte layout is the `boot-record` contract's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordPlacement {
    pub kind: RecordKind,
    pub at: u32,
    pub size: u32,
    pub survives: Survives,
    pub rings: Vec<Ring>,
    pub fuse: Fuse,
    /// Addresses inside the region the record never stores to (registers
    /// another part of the MCU uses).
    #[serde(default)]
    pub reserved: Vec<u32>,
}

/// `[boot]` of an MCU definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McuBoot {
    pub record: Option<RecordPlacement>,
}

/// What a boot action program may use on this MCU besides pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BootCapability {
    I2c,
}

/// Largest boot record region (VERSION pages address it with a u16).
pub const RECORD_MAX: u32 = 4096;
/// Most writers a boot record holds.
pub const RECORD_WRITERS_MAX: usize = 16;

/// Fields of the capability record, by name, for the kind table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    MpuRegions,
    MinRegion,
    SubregionsFrom,
    SauRegions,
    SMpuRegions,
    NsMpuRegions,
    PmpEntries,
    Grain,
    Na4,
    Tor,
    Worlds,
    MemoryRegions,
    PeripheralSlots,
    GdmaChannels,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Self::MpuRegions => "mpu_regions",
            Self::MinRegion => "min_region",
            Self::SubregionsFrom => "subregions_from",
            Self::SauRegions => "sau_regions",
            Self::SMpuRegions => "s_mpu_regions",
            Self::NsMpuRegions => "ns_mpu_regions",
            Self::PmpEntries => "pmp_entries",
            Self::Grain => "grain",
            Self::Na4 => "na4",
            Self::Tor => "tor",
            Self::Worlds => "worlds",
            Self::MemoryRegions => "memory_regions",
            Self::PeripheralSlots => "peripheral_slots",
            Self::GdmaChannels => "gdma_channels",
        }
    }

    fn given(self, c: &Capabilities) -> bool {
        match self {
            Self::MpuRegions => c.mpu_regions.is_some(),
            Self::MinRegion => c.min_region.is_some(),
            Self::SubregionsFrom => c.subregions_from.is_some(),
            Self::SauRegions => c.sau_regions.is_some(),
            Self::SMpuRegions => c.s_mpu_regions.is_some(),
            Self::NsMpuRegions => c.ns_mpu_regions.is_some(),
            Self::PmpEntries => c.pmp_entries.is_some(),
            Self::Grain => c.grain.is_some(),
            Self::Na4 => c.na4.is_some(),
            Self::Tor => c.tor.is_some(),
            Self::Worlds => c.worlds.is_some(),
            Self::MemoryRegions => c.memory_regions.is_some(),
            Self::PeripheralSlots => c.peripheral_slots.is_some(),
            Self::GdmaChannels => c.gdma_channels.is_some(),
        }
    }

    const ALL: [Field; 14] = [
        Self::MpuRegions,
        Self::MinRegion,
        Self::SubregionsFrom,
        Self::SauRegions,
        Self::SMpuRegions,
        Self::NsMpuRegions,
        Self::PmpEntries,
        Self::Grain,
        Self::Na4,
        Self::Tor,
        Self::Worlds,
        Self::MemoryRegions,
        Self::PeripheralSlots,
        Self::GdmaChannels,
    ];
}

/// One protection kind's rules: what its capability record holds and the
/// values the hardware allows.
#[derive(Debug, Clone, Copy)]
pub struct KindRule {
    pub kind: Kind,
    pub name: &'static str,
    /// The level a definition of this kind may claim besides `none`.
    pub level: Level,
    pub required: &'static [Field],
    pub optional: &'static [Field],
    /// `mpu_regions` / `pmp_entries` values the architecture allows.
    pub unit_counts: &'static [u32],
    /// Smallest `min_region` the architecture allows (power of two).
    pub min_region_floor: u32,
    pub critical_section: Option<CriticalSection>,
    pub unwind: Option<Unwind>,
    pub timers: &'static [Timer],
    /// Catalog bound of a service's non-preemptible part; a definition may
    /// only lower it.
    pub np_max_us: u32,
    /// Catalog bound of a DMA stop; a definition may only lower it.
    pub dma_stop_max_us: u32,
    /// Register blocks a definition of this kind must list besides the
    /// classes every kind needs ([`REQUIRED_CLASSES`]).
    pub extra_classes: &'static [Why],
}

/// Classes every definition whose kind is not `none` must list.
pub const REQUIRED_CLASSES: [Why; 5] = [
    Why::Flash,
    Why::Watchdog,
    Why::Clock,
    Why::Reset,
    Why::IrqRouting,
];

const MPU_FIELDS: &[Field] = &[Field::MpuRegions, Field::MinRegion, Field::SubregionsFrom];
const PMP_FIELDS: &[Field] = &[Field::PmpEntries, Field::Grain];
const PMP_OPTIONAL: &[Field] = &[Field::Na4, Field::Tor];
const NP_MAX_US: u32 = 50;
const DMA_STOP_MAX_US: u32 = 20;

const fn cortex(
    kind: Kind,
    name: &'static str,
    unit_counts: &'static [u32],
    min_region_floor: u32,
    critical_section: CriticalSection,
    required: &'static [Field],
) -> KindRule {
    KindRule {
        kind,
        name,
        level: Level::Mpu,
        required,
        optional: &[],
        unit_counts,
        min_region_floor,
        critical_section: Some(critical_section),
        unwind: Some(Unwind::CortexMException),
        timers: &[Timer::Systick],
        np_max_us: NP_MAX_US,
        dma_stop_max_us: DMA_STOP_MAX_US,
        extra_classes: &[],
    }
}

/// Every protection kind, one row each.
pub const KIND_RULES: &[KindRule] = &[
    cortex(
        Kind::Armv6mMpu,
        "armv6m-mpu",
        &[0, 8],
        256,
        CriticalSection::Primask,
        MPU_FIELDS,
    ),
    cortex(
        Kind::Armv7mMpu,
        "armv7m-mpu",
        &[8, 16],
        32,
        CriticalSection::Basepri,
        MPU_FIELDS,
    ),
    cortex(
        Kind::Armv8mMpu,
        "armv8m-mpu",
        &[0, 4, 8, 12, 16],
        32,
        CriticalSection::Basepri,
        &[Field::MpuRegions],
    ),
    KindRule {
        kind: Kind::Armv8mTz,
        name: "armv8m-tz",
        level: Level::Tz,
        required: &[Field::SauRegions, Field::SMpuRegions, Field::NsMpuRegions],
        optional: &[],
        unit_counts: &[0, 4, 8, 12, 16],
        min_region_floor: 32,
        critical_section: Some(CriticalSection::Basepri),
        unwind: Some(Unwind::Armv8mTz),
        timers: &[Timer::Systick],
        np_max_us: NP_MAX_US,
        dma_stop_max_us: DMA_STOP_MAX_US,
        extra_classes: &[],
    },
    KindRule {
        kind: Kind::RiscvPmp,
        name: "riscv-pmp",
        level: Level::Mpu,
        required: PMP_FIELDS,
        optional: PMP_OPTIONAL,
        unit_counts: &[0, 16, 64],
        min_region_floor: 8,
        critical_section: Some(CriticalSection::MstatusMie),
        unwind: Some(Unwind::RiscvTrap),
        timers: &[Timer::RiscvMtime],
        np_max_us: NP_MAX_US,
        dma_stop_max_us: DMA_STOP_MAX_US,
        extra_classes: &[],
    },
    KindRule {
        kind: Kind::RiscvEpmp,
        name: "riscv-epmp",
        level: Level::Mpu,
        required: PMP_FIELDS,
        optional: PMP_OPTIONAL,
        unit_counts: &[0, 16, 64],
        min_region_floor: 8,
        critical_section: Some(CriticalSection::MstatusMie),
        unwind: Some(Unwind::RiscvTrap),
        timers: &[Timer::RiscvMtime],
        np_max_us: NP_MAX_US,
        dma_stop_max_us: DMA_STOP_MAX_US,
        extra_classes: &[Why::Protection],
    },
    KindRule {
        kind: Kind::EspPms,
        name: "esp-pms",
        level: Level::Pms,
        required: &[Field::Worlds, Field::GdmaChannels],
        optional: &[Field::MemoryRegions, Field::PeripheralSlots],
        unit_counts: &[],
        min_region_floor: 0,
        critical_section: Some(CriticalSection::XtensaLevel),
        unwind: Some(Unwind::XtensaWorld),
        timers: &[Timer::TimerGroup],
        np_max_us: NP_MAX_US,
        dma_stop_max_us: DMA_STOP_MAX_US,
        extra_classes: &[Why::Protection],
    },
    KindRule {
        kind: Kind::None,
        name: "none",
        level: Level::None,
        required: &[],
        optional: &[],
        unit_counts: &[],
        min_region_floor: 0,
        critical_section: None,
        unwind: None,
        timers: &[],
        np_max_us: NP_MAX_US,
        dma_stop_max_us: DMA_STOP_MAX_US,
        extra_classes: &[],
    },
];

/// The rule row of `kind`.
pub fn rule(kind: Kind) -> &'static KindRule {
    KIND_RULES
        .iter()
        .find(|r| r.kind == kind)
        .expect("every kind has a rule row")
}

impl McuDef {
    /// `[isolation]`, or the `none` record when the definition leaves it out.
    pub fn isolation(&self) -> &Isolation {
        static NONE: Isolation = Isolation {
            level: Level::None,
            kind: Kind::None,
            boot_extensions: false,
            ext_ram_max: None,
            root_protection: Vec::new(),
            cache: Cache::None,
            tcm: Vec::new(),
            capabilities: Capabilities {
                mpu_regions: None,
                min_region: None,
                subregions_from: None,
                sau_regions: None,
                s_mpu_regions: None,
                ns_mpu_regions: None,
                idau: Vec::new(),
                pmp_entries: None,
                grain: None,
                na4: None,
                tor: None,
                worlds: None,
                memory_regions: None,
                peripheral_slots: None,
                gdma_channels: None,
                critical_section: None,
                np_max_us: None,
                dma_stop_max_us: None,
                reset_cleanup: None,
                background_privileged: None,
                unwind: None,
                timer: None,
                fault_address: None,
            },
        };
        self.isolation.as_ref().unwrap_or(&NONE)
    }

    /// The service non-preemptible bound on this MCU (the catalog's,
    /// unless the definition lowers it).
    pub fn np_max_us(&self) -> u32 {
        let i = self.isolation();
        i.capabilities.np_max_us.unwrap_or(rule(i.kind).np_max_us)
    }

    /// The DMA stop bound on this MCU (the catalog's, unless lowered).
    pub fn dma_stop_max_us(&self) -> u32 {
        let i = self.isolation();
        i.capabilities
            .dma_stop_max_us
            .unwrap_or(rule(i.kind).dma_stop_max_us)
    }

    pub fn peripheral(&self, id: &str) -> Option<&Peripheral> {
        self.peripherals.iter().find(|p| p.id == id)
    }

    /// The boot record placement, when the MCU has one.
    pub fn record(&self) -> Option<&RecordPlacement> {
        self.boot.record.as_ref()
    }
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

fn overlaps(a: (u32, u32), b: (u32, u32)) -> bool {
    let end = |(at, size): (u32, u32)| u64::from(at) + u64::from(size);
    u64::from(a.0) < end(b) && u64::from(b.0) < end(a)
}

fn inside(inner: (u32, u32), outer: (u32, u32)) -> bool {
    inner.0 >= outer.0
        && u64::from(inner.0) + u64::from(inner.1) <= u64::from(outer.0) + u64::from(outer.1)
}

/// The capability record against its kind's row.
pub(super) fn capabilities(m: &McuDef) -> Result<(), String> {
    let i = m.isolation();
    let r = rule(i.kind);
    let c = &i.capabilities;
    let lv = |l: Level| {
        fail(i.level != Level::None && i.level != l, || {
            format!(
                "isolation level {} is not one kind {} gives (none or {})",
                i.level.as_str(),
                r.name,
                r.level.as_str()
            )
        })
    };
    lv(r.level)?;
    if r.kind == Kind::None {
        return fail(*c != Capabilities::default(), || {
            "kind none has no capability record".into()
        });
    }
    for f in Field::ALL {
        let mine = r.required.contains(&f) || r.optional.contains(&f);
        fail(r.required.contains(&f) && !f.given(c), || {
            format!(
                "isolation.capabilities.{} is required for kind {}",
                f.name(),
                r.name
            )
        })?;
        fail(!mine && f.given(c), || {
            format!(
                "isolation.capabilities.{} is not part of kind {}",
                f.name(),
                r.name
            )
        })?;
    }
    fail(!c.idau.is_empty() && r.kind != Kind::Armv8mTz, || {
        format!("isolation.capabilities.idau is not part of kind {}", r.name)
    })?;
    let units = [
        c.mpu_regions,
        c.s_mpu_regions,
        c.ns_mpu_regions,
        c.pmp_entries,
    ];
    if let Some(n) = units
        .into_iter()
        .flatten()
        .find(|n| !r.unit_counts.contains(n))
    {
        return Err(format!(
            "kind {} implements {:?} regions or entries, not {n}",
            r.name, r.unit_counts
        ));
    }
    fail(
        c.sau_regions.is_some_and(|n| ![0, 4, 8].contains(&n)),
        || "isolation.capabilities.sau_regions must be 0, 4 or 8".into(),
    )?;
    let no_units = c.mpu_regions == Some(0) || c.pmp_entries == Some(0);
    fail(no_units && i.level != Level::None, || {
        format!("kind {} with 0 regions can only be level none", r.name)
    })?;
    if let Some(min) = c.min_region.or(c.grain) {
        fail(!min.is_power_of_two() || min < r.min_region_floor, || {
            format!(
                "smallest region {min} must be a power of two of at least {} on kind {}",
                r.min_region_floor, r.name
            )
        })?;
    }
    fail(
        c.subregions_from
            .is_some_and(|s| s != 0 && (!s.is_power_of_two() || s < 256)),
        || "isolation.capabilities.subregions_from must be 0 or a power of two from 256".into(),
    )?;
    fail(c.critical_section != r.critical_section, || {
        format!(
            "isolation.capabilities.critical_section must be {:?} on kind {}",
            r.critical_section, r.name
        )
    })?;
    fail(c.unwind != r.unwind, || {
        format!(
            "isolation.capabilities.unwind must be {} on kind {}",
            r.unwind.map_or("absent", Unwind::as_str),
            r.name
        )
    })?;
    fail(!c.timer.is_some_and(|t| r.timers.contains(&t)), || {
        format!(
            "isolation.capabilities.timer must be one of {:?} on kind {}",
            r.timers, r.name
        )
    })?;
    fail(
        c.reset_cleanup.is_none() || c.background_privileged.is_none() || c.fault_address.is_none(),
        || {
            "isolation.capabilities needs reset_cleanup, background_privileged and fault_address"
                .into()
        },
    )?;
    fail(
        c.np_max_us.is_some_and(|v| v == 0 || v > r.np_max_us),
        || {
            format!(
                "isolation.capabilities.np_max_us may only lower the catalog's {}",
                r.np_max_us
            )
        },
    )?;
    fail(
        c.dma_stop_max_us
            .is_some_and(|v| v == 0 || v > r.dma_stop_max_us),
        || {
            format!(
                "isolation.capabilities.dma_stop_max_us may only lower the catalog's {}",
                r.dma_stop_max_us
            )
        },
    )?;
    fail(i.ext_ram_max.is_none_or(|v| v == 0), || {
        format!(
            "isolation.ext_ram_max is required (and above 0) for kind {}",
            r.name
        )
    })?;
    for t in &i.tcm {
        fail(
            t.size == 0 || t.at.checked_add(t.size - 1).is_none(),
            || {
                format!(
                    "isolation.tcm {:#x}+{:#x} is empty or past 2^32",
                    t.at, t.size
                )
            },
        )?;
    }
    fail(i.boot_extensions && m.port != PortKind::SelfBoot, || {
        "isolation.boot_extensions needs a DAGON boot stage (port self-boot)".into()
    })
}

/// Every register block: unique, non-empty, not overlapping; bus masters
/// never window-eligible; every other block says why it stays privileged;
/// the classes the kind needs are present.
pub(super) fn peripherals(m: &McuDef) -> Result<(), String> {
    for (i, p) in m.peripherals.iter().enumerate() {
        let at = format!("peripheral '{}'", p.id);
        fail(p.id.is_empty(), || "a peripheral has an empty id".into())?;
        fail(p.size == 0 || p.end() > 1 << 32, || {
            format!("{at} ({:#x}+{:#x}) is empty or past 2^32", p.base, p.size)
        })?;
        if let Some(o) = m.peripherals[..i].iter().find(|o| o.id == p.id) {
            return Err(format!("{at} is defined twice ({:#x})", o.base));
        }
        if let Some(o) = m.peripherals[..i]
            .iter()
            .find(|o| overlaps((o.base, o.size), (p.base, p.size)))
        {
            return Err(format!("{at} overlaps peripheral '{}'", o.id));
        }
        let memories = m
            .ram
            .map(|r| ("ram", (r.at, r.size)))
            .into_iter()
            .chain(
                m.flash_maps
                    .iter()
                    .map(|f| ("a flash map", (f.base, f.bytes))),
            )
            .chain(m.slots.iter().map(|s| ("an image slot", (s.start, s.size))));
        for (what, range) in memories {
            fail(overlaps((p.base, p.size), range), || {
                format!("{at} overlaps {what}: register blocks and memory never share addresses")
            })?;
        }
        fail(p.bus_master && p.window_eligible, || {
            format!("{at} is a bus master and can never be window-eligible")
        })?;
        fail(p.window_eligible && !p.privileged_why.is_empty(), || {
            format!("{at} is window-eligible yet names privileged_why")
        })?;
        fail(!p.window_eligible && p.privileged_why.is_empty(), || {
            format!("{at} is not window-eligible: say why (privileged_why)")
        })?;
        fail(
            p.bus_master
                && !p.privileged_why.contains(&Why::Dma)
                && !p.privileged_why.contains(&Why::Shared),
            || format!("{at} is a bus master: privileged_why names dma or shared"),
        )?;
    }
    let i = m.isolation();
    for (n, c) in m.cores.iter().enumerate() {
        fail(
            c.id.is_empty() || m.cores[..n].iter().any(|o| o.id == c.id),
            || format!("core '{}' is empty or defined twice", c.id),
        )?;
    }
    if i.kind == Kind::None {
        return Ok(());
    }
    fail(m.cores.is_empty(), || {
        "a definition whose isolation kind is not none lists its [[core]]s".into()
    })?;
    let mut classes: Vec<Why> = REQUIRED_CLASSES.to_vec();
    classes.extend_from_slice(rule(i.kind).extra_classes);
    if m.cores.len() > 1 {
        classes.push(Why::CoreControl);
    }
    for class in classes {
        fail(
            !m.peripherals
                .iter()
                .any(|p| p.privileged_why.contains(&class)),
            || {
                format!(
                    "no [[peripheral]] with privileged_why = \"{}\": kind {} needs every block of that class listed",
                    class.as_str(),
                    i.kind.as_str()
                )
            },
        )?;
    }
    Ok(())
}

/// `[watchdog]` periods.
pub(super) fn watchdog(m: &McuDef) -> Result<(), String> {
    let Some(w) = &m.watchdog else { return Ok(()) };
    fail(w.trial_ms == 0 || w.confirmed_ms == 0, || {
        "watchdog.trial_ms and confirmed_ms must be above 0".into()
    })
}

/// `ram` and `[boot.record]`: the record and its fuse cells inside RAM
/// (noinit-ram) or a declared block (retained-registers), rings and cells
/// inside their region and apart, within [`RECORD_MAX`].
pub(super) fn record(m: &McuDef) -> Result<(), String> {
    if let Some(r) = m.ram {
        fail(
            r.size == 0 || r.at.checked_add(r.size - 1).is_none(),
            || format!("ram {:#x}+{:#x} is empty or past 2^32", r.at, r.size),
        )?;
    }
    let Some(rec) = m.record() else { return Ok(()) };
    fail(m.port != PortKind::SelfBoot, || {
        "boot.record needs a DAGON boot stage (port self-boot)".into()
    })?;
    fail(rec.size == 0 || rec.size > RECORD_MAX, || {
        format!(
            "boot.record.size {:#x} must be 1..={RECORD_MAX:#x}",
            rec.size
        )
    })?;
    let region = (rec.at, rec.size);
    let host = match rec.kind {
        RecordKind::NoinitRam => {
            let ram = m
                .ram
                .ok_or("boot.record kind noinit-ram needs the MCU's ram")?;
            (ram.at, ram.size)
        }
        RecordKind::RetainedRegisters => m
            .peripherals
            .iter()
            .find(|p| inside(region, (p.base, p.size)))
            .map(|p| (p.base, p.size))
            .ok_or(
                "boot.record kind retained-registers must lie inside a declared [[peripheral]]",
            )?,
    };
    fail(!inside(region, host), || {
        format!(
            "boot.record {:#x}+{:#x} is outside its memory",
            rec.at, rec.size
        )
    })?;
    fail(
        rec.rings.is_empty() || rec.rings.len() > RECORD_WRITERS_MAX,
        || format!("boot.record needs 1..={RECORD_WRITERS_MAX} rings"),
    )?;
    for (n, ring) in rec.rings.iter().enumerate() {
        let what = format!("boot.record ring {n} ({:?})", ring.writer);
        fail(
            ring.size == 0 || !inside((ring.at, ring.size), region),
            || format!("{what} is empty or outside the record"),
        )?;
        fail(
            rec.rings[..n]
                .iter()
                .any(|o| o.writer == ring.writer || overlaps((o.at, o.size), (ring.at, ring.size))),
            || format!("{what} repeats a writer or overlaps another ring"),
        )?;
        fail(
            rec.reserved
                .iter()
                .any(|&a| inside((a, 1), (ring.at, ring.size))),
            || format!("{what} holds a reserved address"),
        )?;
        let stage = match ring.writer {
            ImageKind::Stage0 => m.layout.stage0.is_some(),
            ImageKind::Bootcore => m.slots.iter().any(|s| s.image == ImageKind::Bootcore),
            _ => false,
        };
        fail(!stage, || format!("{what}: the MCU has no such boot stage"))?;
    }
    for (n, c) in rec.fuse.cells.iter().enumerate() {
        let what = format!("boot.record fuse cell {:?}", c.image);
        fail(c.size == 0 || !inside((c.at, c.size), host), || {
            format!("{what} is empty or outside the record's memory")
        })?;
        fail(overlaps((c.at, c.size), region), || {
            format!("{what} overlaps the record")
        })?;
        fail(
            rec.fuse.cells[..n]
                .iter()
                .any(|o| o.image == c.image || overlaps((o.at, o.size), (c.at, c.size))),
            || format!("{what} repeats an image or overlaps another cell"),
        )?;
        fail(
            !matches!(c.image, ImageKind::Bootcore | ImageKind::App),
            || format!("{what}: cells guard the boot core or the app"),
        )?;
    }
    Ok(())
}
