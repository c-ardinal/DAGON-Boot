//! MCU and board definitions (`dagon-mcu.toml` / `dagon-board.toml`) and the
//! [`Registry`] built from them. The format is described in
//! `targets/README.md`. Kinds (port, image, placement, writer, link, ROM
//! entry) are closed sets in code; everything else is data.

mod load;
pub mod requirements;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_requirements;

use std::sync::OnceLock;

use dagon_format::header::{image_kind, placement, port_kind, slot_binding};
use serde::Deserialize;

use crate::schema::{ControlKind, LinkKind, RomEntry, UartIface};

pub use load::{DefError, DefSource, Source};

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
}

/// `dagon-mcu.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McuDef {
    pub id: String,
    /// Display name (`RP2040`).
    pub name: String,
    /// Image header `mcu_id`.
    pub wire_id: u16,
    pub port: PortKind,
    /// GPIO0..GPIOn-1 a node without a named board is checked against;
    /// 0 when pins are named by port (`PB4`).
    #[serde(default)]
    pub gpio_count: u8,
    /// The first entry is the default.
    #[serde(rename = "flash_map")]
    pub flash_maps: Vec<FlashMapDef>,
    /// Partition table CSV inside the package.
    pub partitions: Option<String>,
    #[serde(default)]
    pub layout: Layout,
    #[serde(rename = "slot")]
    pub slot_defs: Vec<SlotDef>,
    pub writer: Writer,
    #[serde(default, rename = "part")]
    pub parts: Vec<Part>,
    pub coordinator: Option<Coordinator>,
    pub rom_i2c: Option<RomI2c>,
    /// Link kinds the MCU package's firmware implements (its `LINK_KINDS`).
    #[serde(default)]
    pub links: Links,
    pub connection: Connection,
    #[serde(default)]
    pub doctor: Vec<DoctorCheck>,
    /// Filled on load from `partitions`.
    #[serde(skip)]
    pub partition_table: Vec<Partition>,
    /// Filled on load from `slot_defs`.
    #[serde(skip)]
    pub slots: Vec<Slot>,
}

impl McuDef {
    pub fn default_flash_map(&self) -> &FlashMapDef {
        &self.flash_maps[0]
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
    pub id: String,
    pub product: String,
    pub mcu: String,
    pub flash_bytes: u32,
    #[serde(default)]
    pub i2c_target: bool,
    /// Which line drawing the UI uses.
    pub drawing: String,
    #[serde(default)]
    pub header: Vec<HeaderPin>,
    /// I2C pin pairs usable from the header.
    #[serde(default, rename = "i2c")]
    pub i2c_pairs: Vec<I2cPair>,
    /// `None` for the coordinator.
    pub default_link: Option<DefaultLink>,
    /// Parts built from the board package itself.
    #[serde(default, rename = "part")]
    pub parts: Vec<Part>,
}

/// Every MCU and board known: builtin, a DAGON source tree, project
/// definitions. Order is definition order; the first board of an MCU is
/// its default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    mcus: Vec<McuDef>,
    boards: Vec<BoardDef>,
    /// What each definition was read from, in `mcus` / `boards` order: the
    /// JSON form the browser loads ([`Registry::to_json`]).
    mcu_sources: Vec<DefSource>,
    board_sources: Vec<DefSource>,
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

    /// The definitions as JSON (their TOML text and the text files they
    /// reference), for a loader without a filesystem.
    pub fn to_json(&self) -> String {
        let all = load::JsonForm {
            mcus: self.mcu_sources.clone(),
            boards: self.board_sources.clone(),
        };
        serde_json::to_string(&all).expect("definitions serialise")
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

    pub fn board(&self, id: &str) -> Option<&BoardDef> {
        self.boards.iter().find(|b| b.id == id)
    }

    /// First board defined for an MCU.
    pub fn default_board(&self, mcu: &str) -> Option<&BoardDef> {
        self.boards.iter().find(|b| b.mcu == mcu)
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
