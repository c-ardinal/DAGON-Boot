//! MCU and board definitions (`dagon-mcu.toml` / `dagon-board.toml`) and the
//! [`Registry`] built from them. The format is described in
//! `targets/README.md`. Kinds (port, image, placement, writer, link, ROM
//! entry) are closed sets in code; everything else is data.

pub mod board_boot;
pub mod boot_folder;
pub mod codegen;
pub mod dma;
pub mod execution;
pub mod isolation;
mod load;
mod project_text;
pub mod requirements;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_isolation;
#[cfg(test)]
mod tests_requirements;

use std::sync::OnceLock;

use dagon_format::header::{image_kind, placement, port_kind, slot_binding};
use serde::Deserialize;

use crate::schema::{ControlKind, LinkKind, RomEntry, UartIface};

pub use board_boot::{BoardBoot, BoardBus, BoardDevice, BoardPeripheral, BoardPin};
pub use execution::{Conformance, Execution, Fact, FlashOp};
pub use isolation::{Core, Isolation, McuBoot, Peripheral, RecordPlacement, Watchdog};
pub use load::{DefError, DefSource, Source, BUILTIN_FILES};

/// The `dagon-mcu.toml` / `dagon-board.toml` format number this reader
/// reads (their `format` key; absent means this one).
pub const FORMAT: u32 = 1;

use requirements::HEADER;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PortKind {
    SelfBoot,
    VendorStage2,
}

impl PortKind {
    const ALL: [PortKind; 2] = [Self::SelfBoot, Self::VendorStage2];

    /// The kind a topology `port` names.
    pub fn from_name(name: &str) -> Option<PortKind> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// The kind of image header `port_kind` `wire`.
    pub fn from_wire(wire: u8) -> Option<PortKind> {
        Self::ALL.into_iter().find(|k| k.wire() == wire)
    }

    /// Image header `port_kind`.
    pub fn wire(self) -> u8 {
        match self {
            Self::SelfBoot => port_kind::SELF_BOOT,
            Self::VendorStage2 => port_kind::VENDOR_STAGE2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelfBoot => "self-boot",
            Self::VendorStage2 => "vendor-stage2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageKind {
    App,
    Bootcore,
    Agent,
    Stage0,
}

impl ImageKind {
    /// Image header `image_kind`.
    pub fn wire(self) -> u8 {
        match self {
            Self::App => image_kind::APP,
            Self::Bootcore => image_kind::BOOTCORE,
            Self::Agent => image_kind::AGENT,
            Self::Stage0 => image_kind::STAGE0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Binding {
    A,
    B,
    /// Runs from either slot (MMU-mapped); answers for slot A.
    Any,
}

impl Binding {
    /// Image header `slot_binding`.
    pub fn wire(self) -> u8 {
        match self {
            Self::A => slot_binding::A,
            Self::B => slot_binding::B,
            Self::Any => slot_binding::ANY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    /// Header in the first 512 bytes of the slot, payload after it.
    Inline,
    /// Payload from the slot start, header in the last 512 bytes.
    Trailer,
    /// Header elsewhere (`header_at` or `header_partition`).
    Detached,
}

impl Placement {
    /// Image header `placement`.
    pub fn wire(self) -> u8 {
        match self {
            Self::Inline => placement::INLINE,
            Self::Trailer => placement::TRAILER,
            Self::Detached => placement::DETACHED,
        }
    }
}

/// A named flash layout of an MCU and the flash it needs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashMapDef {
    pub name: String,
    /// Address of the first flash byte as images see it.
    pub base: u32,
    pub bytes: u32,
    /// `true`: the CPU reads (and executes) flash at `base`, so flash
    /// shares the address space with RAM and register blocks. `false`:
    /// `base` and the slots are flash offsets an MMU maps elsewhere.
    /// Required: no default.
    pub xip: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub at: u32,
    pub size: u32,
}

/// The two faces of a state log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogFaces {
    pub faces: [u32; 2],
    pub face_size: u32,
}

/// Fixed regions outside the image slots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// boot2 + stage0 from the start of flash.
    pub stage0: Option<Region>,
    /// Stage0 also checks a boot core header's signature under the
    /// project key built into it (the one authentication rule), not only
    /// the digest the log records; left out: digest only (a stage0 region
    /// too small for the verifier).
    #[serde(default)]
    pub stage0_verifies: bool,
    pub bootcore_log: Option<LogFaces>,
    pub app_log: Option<LogFaces>,
}

/// An image slot as written: `at` + `size`, or a `partition` of the MCU's
/// partition table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    pub image: ImageKind,
    pub binding: Binding,
    pub header: Placement,
    pub at: Option<u32>,
    pub size: Option<u32>,
    pub partition: Option<String>,
    /// Detached header: absolute address, or offset into a partition.
    pub header_at: Option<u32>,
    pub header_partition: Option<String>,
    #[serde(default)]
    pub header_offset: u32,
}

/// A slot resolved to addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub image: ImageKind,
    pub binding: Binding,
    pub placement: Placement,
    /// The region the slot occupies.
    pub start: u32,
    pub size: u32,
    /// Absolute address of the 512-byte header.
    pub header_addr: u32,
    pub load_addr: u32,
    /// Largest payload that fits.
    pub max_payload: u32,
}

/// One row of an ESP-IDF style partition table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

/// A DAGON part a first write builds besides the app. `sub` is the part's
/// folder inside the defining package ("" for the package itself); `bin`
/// the binary target (`None`: the part package's own name).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub name: String,
    #[serde(default)]
    pub sub: String,
    pub bin: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbHint {
    pub vid: u16,
    pub pid: u16,
}

/// How the first write reaches the MCU (the writer kind and its
/// parameters).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Writer {
    /// A UF2 file copied to the boot ROM's USB drive, recognised by its
    /// `info_file` (first line starts with `info_header`, a line
    /// `Board-ID: <board_id>`).
    Uf2Drive {
        family: u32,
        sector: u32,
        info_file: String,
        info_header: String,
        board_id: String,
    },
    ProbeRs {
        chip: String,
    },
    Espflash {
        chip: String,
        flash_size: String,
        table_at: u32,
        bootloader: String,
        checksums: String,
        usb_hint: Option<UsbHint>,
    },
}

/// What the coordinator rules check when the MCU is the coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinator {
    pub pio_state_machines: u8,
    /// State machines one pio-uart link takes.
    pub pio_uart_state_machines: u8,
    pub uart0_tx: Vec<u8>,
    pub uart0_rx: Vec<u8>,
}

/// How the UART the MCU package's firmware runs DAGON's UART links on makes
/// a bit rate: `clock_hz` divided by `clocks_per_bit` times a divider with
/// `frac_bits` fraction bits, the divider rounded down (as the HAL the
/// package builds with computes it). `bauds` lists the bit rates that
/// firmware runs its link at: a link's `baud` must be one of them on both
/// ends (V-26), and the Console offers only those, so no speed the
/// definition does not name can be set or built.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkUart {
    pub clock_hz: u32,
    pub clocks_per_bit: u32,
    pub frac_bits: u8,
    pub bauds: Vec<u32>,
}

/// How far a UART end may run from the link's `baud` (a fraction): each
/// end of a link must be within it.
pub const MAX_BAUD_ERROR: f64 = 0.02;

impl LinkUart {
    /// Whether `baud` is one this firmware runs its link at.
    pub fn runs(&self, baud: u32) -> bool {
        self.bauds.contains(&baud)
    }

    /// The bit rate this UART runs at when asked for `baud`; `None` when
    /// it cannot divide its clock down to it (0, or faster than the clock).
    pub fn actual_baud(&self, baud: u32) -> Option<f64> {
        let per_bit = u64::from(self.clocks_per_bit) * u64::from(baud);
        let steps = (u64::from(self.clock_hz) << self.frac_bits).checked_div(per_bit)?;
        if steps >> self.frac_bits == 0 {
            return None;
        }
        let divider = steps as f64 / f64::from(1u32 << self.frac_bits);
        Some(f64::from(self.clock_hz) / (f64::from(self.clocks_per_bit) * divider))
    }

    /// How far [`LinkUart::actual_baud`] is from `baud`, as a fraction
    /// (+0.0122 is 1.22 % fast).
    pub fn baud_error(&self, baud: u32) -> Option<f64> {
        self.actual_baud(baud).map(|a| a / f64::from(baud) - 1.0)
    }
}

/// The pins a board wires new links to when it is the 親機 (numbered
/// GPIOs, as the topology's `buses[]` and `links[].pins` take them).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubPins {
    pub i2c: HubI2c,
    pub pio_uart: HubUart,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubI2c {
    /// One of the board's `i2c` controllers.
    pub controller: String,
    pub sda: u8,
    pub scl: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubUart {
    pub tx: u8,
    pub rx: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I2cPair {
    pub controller: String,
    pub sda: String,
    pub scl: String,
}

/// The ROM I2C bootloader (ROM entry kind `stm32-i2c-rom`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RomI2c {
    /// What check messages call it (`STM32 ROM bootloader`).
    pub name: String,
    pub addr: u8,
    pub max_hz: u32,
    /// Flash sectors a first install through the ROM erases.
    #[serde(default)]
    pub erase_sectors: Vec<u16>,
    /// Pin pairs the ROM listens on.
    #[serde(default)]
    pub pairs: Vec<I2cPair>,
}

/// Link kinds by direction: `up` towards the PC or the 親機, `down` to 子機.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Links {
    #[serde(default)]
    pub up: Vec<LinkKind>,
    #[serde(default)]
    pub down: Vec<LinkKind>,
}

/// How the user connects the MCU for a write (UI copy).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub label: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Expect {
    /// The command runs and succeeds.
    Runs,
    /// Its output has this line (trimmed).
    Line(String),
    /// Its output has a line starting with this.
    Prefix(String),
}

/// A tool `dagon doctor` checks for this MCU. A project-supplied `fix`
/// must never run without the user's explicit confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorCheck {
    pub what: String,
    pub probe: Vec<String>,
    pub expect: Expect,
    pub fix: Option<Vec<String>>,
    #[serde(default)]
    pub hint: String,
    /// `fix` takes long enough that the user is told before it starts.
    #[serde(default)]
    pub slow: bool,
}

/// `dagon-mcu.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McuDef {
    /// Definition format number (`1`; left out means 1). A dagon that does not read the number
    /// refuses the file (site/src/content/docs/explanation/versioning.md).
    pub format: Option<u32>,
    /// MCU id used in topologies (`stm32f401re`).
    pub id: String,
    /// Display name (`STM32F401RE`).
    pub name: String,
    /// Image header `mcu_id`; unique.
    pub wire_id: u16,
    /// `self-boot` or `vendor-stage2`.
    pub port: PortKind,
    /// GPIO0..n-1 checked for nodes without a board; 0 when pins are named by port.
    #[serde(default)]
    pub gpio_count: u8,
    /// `name` (unique), `base` (address of flash byte 0), `bytes`, `xip` (required; `true`: the CPU
    /// reads flash at `base`, so flash shares the address space with RAM and register blocks;
    /// `false`: `base` and the slots are flash offsets an MMU maps elsewhere). The first is the
    /// default.
    #[serde(rename = "flash_map")]
    pub flash_maps: Vec<FlashMapDef>,
    /// ESP-IDF partition table CSV in the package (rows need explicit offset and size).
    pub partitions: Option<String>,
    /// Optional fixed regions: `stage0 = { at, size }`, `bootcore_log` / `app_log = { faces = [a,
    /// b], face_size }`; a `bootcore_log` equal to `app_log` is one log holding both the boot core
    /// and the APP records; `stage0_verifies = true`: stage0 also checks a boot core's signature
    /// (left out: digests only).
    #[serde(default)]
    pub layout: Layout,
    /// `image` (`app`, `bootcore`, `agent`, `stage0`), `binding` (`a`, `b`, `any`), `header`
    /// (`inline`, `trailer`, `detached`); the region is `at` + `size` or `partition`; a detached
    /// header is at `header_at` or `header_partition` + `header_offset`.
    #[serde(rename = "slot")]
    pub slot_defs: Vec<SlotDef>,
    /// First-write tool: `kind` and its parameters (below).
    pub writer: Writer,
    /// DAGON parts built from this package for a first write: `name`, `sub` (folder in the
    /// package), `bin`.
    #[serde(default, rename = "part")]
    pub parts: Vec<Part>,
    /// Only for an MCU that can be the coordinator: `pio_state_machines`, `pio_uart_state_machines`
    /// (per link), `uart0_tx`, `uart0_rx`. A topology may make the MCU its coordinator only when
    /// `[coordinator]` is defined (V-25).
    pub coordinator: Option<Coordinator>,
    /// How the UART the MCU package runs DAGON's UART links on makes a bit rate, as its HAL
    /// computes it: `clock_hz` divided by `clocks_per_bit` times a divider with `frac_bits`
    /// fraction bits, rounded down. `bauds` lists the bit rates the firmware runs its link at;
    /// the topology check (V-26) refuses a link `baud` that either end's list does not name or
    /// that either end makes more than 2 % off, and the Console offers only the speeds both
    /// ends name. Required on an MCU whose `[links]` has `uart`: without it, every UART link
    /// that MCU's firmware runs is refused, since no speed on it is known to work.
    pub link_uart: Option<LinkUart>,
    /// ROM I2C bootloader (`stm32-i2c-rom`): `name` (as check messages call it), `addr`, `max_hz`,
    /// `erase_sectors`, `pairs = [{ controller, sda, scl }]` it listens on.
    pub rom_i2c: Option<RomI2c>,
    /// Link kinds the MCU package's firmware implements (its `LINK_KINDS`).
    #[serde(default)]
    pub links: Links,
    /// How the user connects it for a write: `label`, `hint`.
    pub connection: Connection,
    /// Tool check: `what`, `probe` (command), `expect` (`"runs"`, `{ line = "..." }`, `{ prefix =
    /// "..." }`), `fix` (command), `hint`, `slow` (`true`: `dagon doctor --fix` says the install
    /// takes a while before it starts).
    #[serde(default)]
    pub doctor: Vec<DoctorCheck>,
    /// `{ at, size }`: all of the MCU's RAM. Images get the RAM below the boot record, the fuse
    /// cells and the extension regions.
    pub ram: Option<Region>,
    /// How DAGON keeps extensions away from itself (below). Left out: `kind = "none"`, `level =
    /// "none"`.
    pub isolation: Option<Isolation>,
    /// Every register block: `id`, `base`, `size`, `window_eligible` (an extension may get it as a
    /// window), `bus_master` (it can write memory), `privileged_why` (one or a list of `dma`,
    /// `flash`, `watchdog`, `clock`, `reset`, `irq-routing`, `pinmux`, `power`, `rom`,
    /// `core-control`, `protection`, `shared`), `dma = { streams, channels }` (a DMA controller, a
    /// bus master: its streams, each selecting one of `channels` requests), `dma_requests = [{
    /// signal, controller, stream, channel }]` (the block's rows of the MCU's DMA request mapping
    /// table: `signal` `a-z 0-9` such as `tx`, `rx`, `ch1`, `up`; one cell serves one block). Board
    /// packages and MCU templates take DMA streams from these (first row of the request on a free
    /// stream).
    #[serde(default, rename = "peripheral")]
    pub peripherals: Vec<Peripheral>,
    /// Every processor core, `id`; DAGON runs on the first.
    #[serde(default, rename = "core")]
    pub cores: Vec<Core>,
    /// `kind` (`iwdg`, `rp-watchdog`, `rtc-wdt`), `trial_ms` (armed on a trial boot),
    /// `confirmed_ms` (armed otherwise), `trial_attempts` (boots the boot stage gives a pending
    /// image before it rolls back: the boot core's limit, or the vendor second stage's). An app may
    /// delay a reboot by at most half the shorter period. The MCU package's build turns
    /// `trial_attempts` into the constant its boot core and its agent use. `flag` (required): the
    /// watchdog-reset flag the lockup fuse needs (set by a watchdog reset, kept over every later
    /// reset but power-on, cleared when a start acknowledges it), a fact of `none`, `register` (a
    /// reset-flag register; name it and its source in a comment) or `package` (the MCU package
    /// keeps it): `"register"`, `{ unverified = "package" }`, `"unmeasured"`. Without a flag with a
    /// source the MCU has no lockup fuse.
    pub watchdog: Option<Watchdog>,
    /// Hardware facts of the execution model
    /// (site/src/content/docs/en/explanation/lifecycle-events.md, "Execution model"): `masked_max_us`
    /// (longest DAGON's own code masks interrupts, 1..=1000), `flash_op` (what an erase or program
    /// stops: `fetch-stall`, `xip-off-masked`, `cache-off-masked`), `erase_unit` (largest unit
    /// DAGON erases in one operation, a power of two), `erase_unit_max_ms` (that erase's datasheet
    /// maximum; below `watchdog.confirmed_ms`). A fact is a number checked against its source (cite
    /// it in a comment), `{ unverified = N }` or `"unmeasured"` (no value; not for
    /// `erase_unit_max_ms`).
    pub execution: Option<Execution>,
    /// `scratch = { at, size }`: flash a conformance build erases and writes; apart from every
    /// slot, header, log face and stage0.
    pub conformance: Option<Conformance>,
    /// `[boot.record]`, for MCUs with a DAGON boot stage: where the boot record and
    /// the lockup fuse live. `kind`
    /// (`noinit-ram` inside `ram`, `retained-registers` inside a `[[peripheral]]`), `at`, `size`
    /// (at most 4096), `survives` (`reset-random`, `reset-cleared`, `power`: what a power loss
    /// leaves), `entropy = { source, min_bits }` (required; the start-up word a new record sequence
    /// mixes in: `source` one of `rng`, `ring-osc`, `osc-capture`, `adc-noise`; `min_bits` its
    /// min-entropy per word, a fact, at least 16 when it has a value), `rings = [{ writer, at, size
    /// }]` (one per boot stage, in boot order, inside the record), `fuse = { cells = [{ image, at,
    /// size }] }` (`bootcore`, `app`; outside the record), `reserved` (addresses the record never
    /// stores to). The byte layout inside is the `boot-record` contract's.
    #[serde(default)]
    pub boot: McuBoot,
    /// What a board's boot action program may use besides pins: `["i2c"]` allows `bus_write`.
    #[serde(default)]
    pub boot_capabilities: Vec<isolation::BootCapability>,
    /// Filled on load from `partitions`.
    #[serde(skip)]
    pub partition_table: Vec<Partition>,
    /// Filled on load from `slot_defs`.
    #[serde(skip)]
    pub slots: Vec<Slot>,
    /// The folder of a project's own MCU package (its parts are built
    /// there); `None` for builtin and source-tree definitions, found in the
    /// DAGON source by id.
    #[serde(skip)]
    pub package: Option<std::path::PathBuf>,
}

impl McuDef {
    pub fn default_flash_map(&self) -> &FlashMapDef {
        &self.flash_maps[0]
    }

    /// Whether flash map and slot addresses are CPU addresses (every flash
    /// map says `xip = true`); otherwise they are flash offsets and share
    /// no address space with RAM or register blocks.
    pub fn flash_is_cpu_mapped(&self) -> bool {
        self.flash_maps.iter().all(|f| f.xip)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderPin {
    pub pin: String,
    /// Printed header label; `None` until confirmed on the real board.
    pub label: Option<String>,
}

/// How a node board is wired to the hub unless the topology says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultLink {
    pub kind: LinkKind,
    pub iface: Option<UartIface>,
    pub addr: Option<u8>,
    pub baud: Option<u32>,
    pub control: ControlKind,
    pub rom_entry: Option<RomEntry>,
}

/// `dagon-board.toml`, or a `[board.<id>]` table of a project's dagon.toml.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDef {
    /// Definition format number (`1`; left out means 1). A dagon that does not read the number
    /// refuses the file (site/src/content/docs/explanation/versioning.md).
    pub format: Option<u32>,
    /// Board id used in `nodes[].board`.
    pub id: String,
    /// Product name.
    pub product: String,
    /// MCU id (must be defined).
    pub mcu: String,
    /// Flash size (must hold one of the MCU's flash maps).
    pub flash_bytes: u32,
    /// Whether the board can be an I2C target.
    #[serde(default)]
    pub i2c_target: bool,
    /// UI drawing.
    pub drawing: String,
    /// `true`: the board a node naming only its MCU is checked against (a project cannot replace
    /// the default of a builtin MCU that already has one: two marked boards do not load). At most
    /// one board of an MCU says so; an MCU's only board is its default without it; an MCU with
    /// several boards and none marked has no default (its nodes are checked without board facts).
    /// Definition order never decides it.
    #[serde(default)]
    pub default: bool,
    /// `[{ pin, label }]`; `label` left out when not confirmed on the board.
    #[serde(default)]
    pub header: Vec<HeaderPin>,
    /// `[{ controller, sda, scl }]` pairs usable from the header.
    #[serde(default, rename = "i2c")]
    pub i2c_pairs: Vec<I2cPair>,
    /// How a node board is wired unless the topology says otherwise: `kind` (`i2c`, `uart`, `can`,
    /// `ethernet`), `iface` (`hw-uart0`, `pio-uart`), `addr`, `baud`, `control` (`full`,
    /// `cooperative`), `rom_entry` (`force-download-boot`, `m5pm1`, `stm32-i2c-rom`). Left out for
    /// the coordinator.
    pub default_link: Option<DefaultLink>,
    /// Pins the page wires new links to when this board is the 親機: `i2c = { controller, sda, scl }`
    /// (a new I2C bus; `controller` is one of the board's `i2c`) and `pio_uart = { tx, rx }` (a new
    /// PIO UART link), numbered GPIOs below the MCU's `gpio_count`. Only on boards whose MCU has
    /// `[coordinator]`.
    pub coordinator: Option<HubPins>,
    /// Parts built from the board package itself (`name`, `sub`, `bin`).
    #[serde(default, rename = "part")]
    pub parts: Vec<Part>,
    /// The boot stage's board values: `led` (a `[[pin]]`), `led_active` (`high`, `low`), `halt = {
    /// pattern, period_ms }` (`blink`, `code`, `off`), `power_hold = { pin, level }`. Its boot
    /// action programs, `[[boot.action]]`: A boot action program (`boot-program` format 1): `on` (a
    /// boot event the event catalog marks action-capable) and `ops`: `{ op = "led", state }` (`on`,
    /// `off`, `toggle`), `{ op = "gpio", pin, level }` (a `[[pin]]` with `boot = true`; the power
    /// hold pin takes only its hold level), `{ op = "delay", ms }` (1 to 20), `{ op = "bus_write",
    /// device, reg, bytes }` (a `[[device]]` register in its `writable` list, 1 to 8 bytes, only on
    /// an MCU with `boot_capabilities = ["i2c"]`). At most 32 ops and 50 ms per event, a bus write
    /// counting 5 ms.
    pub boot: Option<BoardBoot>,
    /// The board's pins: `pin`, `name` (what the board package's code takes it by, `a-z 0-9 _`),
    /// `boot` (a boot program may drive it), `levels` (allowed levels; all when left out).
    #[serde(default, rename = "pin")]
    pub pins: Vec<BoardPin>,
    /// The board's buses: `id` (`a-z 0-9 - _`), `kind` (`i2c`, `spi`, `uart`, `can`), `controller`
    /// (an MCU `[[peripheral]]`), its pins by signal (`sda`, `scl` for i2c; `tx` and/or `rx` for
    /// uart; `tx`, `rx` for can; `sck` and `mosi` and/or `miso` for spi), `hz` (i2c, spi) or `baud`
    /// (uart, can; on a `dagon = true` UART bus the link speed of a build dagon does not give the
    /// topology's, see below), `dagon` (`true`: DAGON's own link runs on it, so neither the bus nor
    /// its controller is ever an extension's), `dma` (`true`: the board package opens or hands out
    /// the bus with DMA; i2c, spi, uart; its tx and rx streams come from the MCU's `dma_requests`,
    /// buses in order, and the board package's build writes them as `<Bus>TxDma`/`<Bus>RxDma`,
    /// `take_dma!` and `bind_<bus>_dma!`).
    #[serde(default, rename = "bus")]
    pub buses: Vec<BoardBus>,
    /// Board names for MCU blocks extensions may declare: `id`, `mcu` (the MCU `[[peripheral]]` id;
    /// never the controller of a `dagon = true` bus).
    #[serde(default, rename = "peripheral")]
    pub peripherals: Vec<BoardPeripheral>,
    /// A device on a board bus: `id` (`a-z 0-9 - _`), `bus`, `addr`, `max_transfer` (required: the
    /// most data bytes it takes in one transfer, from its datasheet; the bus port and boot programs
    /// keep to it), `regs = [{ name, reg }]` (its registers by name, `a-z 0-9 _`), `writable`
    /// (registers anything outside DAGON may write: numbers or names of `regs`).
    #[serde(default, rename = "device")]
    pub devices: Vec<BoardDevice>,
    /// As [`McuDef::package`] (inline project boards have none).
    #[serde(skip)]
    pub package: Option<std::path::PathBuf>,
}

/// What a project-written string becomes wherever it would leave this
/// process unchecked (the page, a serve response).
pub const PROJECT_TEXT: &str = "project-text";

/// The most a project-written string may be to reach the page as it is.
const PAGE_SAFE_MAX: usize = 40;

/// Whether a project-written string may reach the page as it is: at most
/// 40 characters of `A-Z a-z 0-9`, space, `.`, `_`, `-` (no path, no
/// markup, nothing a token or an address hides in comfortably).
pub fn page_safe(s: &str) -> bool {
    s.len() <= PAGE_SAFE_MAX
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'_' | b'-'))
}

/// Where an MCU definition was read from. Commands a [`Origin::Project`]
/// definition names (doctor probes and fixes) never run without the user's
/// explicit confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Embedded in this dagon.
    Builtin,
    /// The project's DAGON source tree.
    Tree,
    /// The project's own files (or definitions received as JSON).
    Project,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Tree => "source-tree",
            Self::Project => "project",
        }
    }
}

/// Every MCU and board known: builtin, a DAGON source tree, project
/// definitions, in definition order, which carries no meaning: an MCU's
/// default board is data ([`Registry::default_board`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    mcus: Vec<McuDef>,
    boards: Vec<BoardDef>,
    /// What each definition was read from, in `mcus` / `boards` order: the
    /// JSON form the browser loads ([`Registry::to_json`]).
    mcu_sources: Vec<DefSource>,
    board_sources: Vec<DefSource>,
    /// Where each definition came from, in `mcus` / `boards` order.
    mcu_origins: Vec<Origin>,
    board_origins: Vec<Origin>,
}

impl Registry {
    /// The definitions embedded at build time (no filesystem needed).
    pub fn builtin() -> &'static Registry {
        static BUILTIN: OnceLock<Registry> = OnceLock::new();
        BUILTIN.get_or_init(|| load::builtin().expect("builtin definitions are valid"))
    }

    /// Builtin definitions, replaced by a DAGON source tree's when one is
    /// given, plus project definitions (which may not redefine an id).
    pub fn load(sources: &[Source<'_>]) -> Result<Registry, DefError> {
        load::load(sources)
    }

    /// The definitions as JSON for a loader without a filesystem (the
    /// browser): their TOML and the text files they reference, with no
    /// local path, comment or doctor command in it.
    pub fn to_json(&self) -> String {
        load::public_json(self)
    }

    /// A registry from [`Registry::to_json`]'s form, validated as on load
    /// (referenced binary files are shape-checked only).
    pub fn from_json(json: &str) -> Result<Registry, DefError> {
        load::from_json(json)
    }

    pub fn mcus(&self) -> &[McuDef] {
        &self.mcus
    }

    pub fn boards(&self) -> &[BoardDef] {
        &self.boards
    }

    pub fn mcu(&self, id: &str) -> Option<&McuDef> {
        self.mcus.iter().find(|m| m.id == id)
    }

    /// Where MCU `id`'s definition was read from.
    pub fn mcu_origin(&self, id: &str) -> Option<Origin> {
        let i = self.mcus.iter().position(|m| m.id == id)?;
        self.mcu_origins.get(i).copied()
    }

    /// Where board `id`'s definition was read from.
    pub fn board_origin(&self, id: &str) -> Option<Origin> {
        let i = self.boards.iter().position(|b| b.id == id)?;
        self.board_origins.get(i).copied()
    }

    pub fn board(&self, id: &str) -> Option<&BoardDef> {
        self.boards.iter().find(|b| b.id == id)
    }

    /// The board a node naming only `mcu` is checked against: its board
    /// marked `default = true`, else its only board; none when it has
    /// several and none is marked (load refuses two marked). Definition
    /// order never decides it.
    pub fn default_board(&self, mcu: &str) -> Option<&BoardDef> {
        let mut boards = self.boards.iter().filter(|b| b.mcu == mcu);
        let first = boards.next()?;
        match boards.next() {
            None => Some(first),
            Some(_) => self.boards.iter().find(|b| b.mcu == mcu && b.default),
        }
    }

    /// Flash map by name, with its MCU.
    pub fn flash_map(&self, name: &str) -> Option<(&McuDef, &FlashMapDef)> {
        self.mcus
            .iter()
            .find_map(|m| m.flash_maps.iter().find(|f| f.name == name).map(|f| (m, f)))
    }

    /// MCU by image header `mcu_id`.
    pub fn mcu_by_wire(&self, wire_id: u16) -> Option<&McuDef> {
        self.mcus.iter().find(|m| m.wire_id == wire_id)
    }

    /// Slot for an image of header kind `kind` in slot `slot`
    /// (`slot_binding::A` / `B`) on `mcu`. Slots bound to `any` answer for
    /// slot A only.
    pub fn slot(&self, mcu: &str, kind: u8, slot: u8) -> Option<&Slot> {
        let want = match slot {
            slot_binding::A => [Binding::A, Binding::Any],
            slot_binding::B => [Binding::B, Binding::B],
            _ => return None,
        };
        self.mcu(mcu)?
            .slots
            .iter()
            .find(|s| s.image.wire() == kind && want.contains(&s.binding))
    }
}
