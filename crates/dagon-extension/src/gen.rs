//! The files `dagon gen` writes for a node from its definitions and
//! extensions: `boot_wiring.rs` (the boot stage's board values, action
//! program, watchdog, boot record placement, isolation record and
//! extension chain), one linker fragment per image slot, and the readable
//! protection map.

use std::fmt::Write as _;

use dagon_catalog::Layer;
use dagon_topology::defs::board_boot::{
    Active, BoardBoot, HaltPattern, LedState, Op, PinLevel, BOOT_PROGRAM_FORMAT,
};
use dagon_topology::defs::{Binding, BoardDef, McuDef};

use crate::kind::{self, Map, Space};
use crate::node::{Ext, ImagePlan};
use crate::Known;

/// The library name of an extension crate's package name.
pub fn lib_name(package: &str) -> String {
    package.replace('-', "_")
}

fn binding(b: Binding) -> &'static str {
    match b {
        Binding::A => "a",
        Binding::B => "b",
        Binding::Any => "any",
    }
}

fn level(l: PinLevel) -> &'static str {
    match l {
        PinLevel::High => "Level::High",
        PinLevel::Low => "Level::Low",
        PinLevel::Input => "Level::Input",
    }
}

/// The `[boot]` program's events are boot kinds the catalog marks
/// action-capable.
fn check_program(boot: &BoardBoot, known: &Known) -> Result<(), String> {
    for a in &boot.actions {
        let ok = known
            .kinds
            .iter()
            .any(|k| k.layer == Layer::Boot && k.action && k.kind == a.on);
        if !ok {
            return Err(format!(
                "boot.action on '{}': no action-capable boot event of that name in the catalog",
                a.on
            ));
        }
    }
    Ok(())
}

const TYPES: &str = "\
/// A pin level a boot op sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    High,
    Low,
    Input,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedState {
    On,
    Off,
    Toggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HaltPattern {
    Blink,
    Code,
    Off,
}

#[derive(Clone, Copy, Debug)]
pub struct Led {
    pub pin: &'static str,
    pub active_high: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Halt {
    pub pattern: HaltPattern,
    pub period_ms: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct PowerHold {
    pub pin: &'static str,
    pub level: Level,
}

/// One op of the boot action program.
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Led(LedState),
    Gpio { pin: &'static str, level: Level },
    Delay { ms: u32 },
    BusWrite { bus: &'static str, addr: u8, reg: u8, bytes: &'static [u8] },
}

/// The ops run on one boot event.
#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub on: &'static str,
    pub ops: &'static [Op],
}

/// One writer's ring of the boot record, or one lockup fuse cell.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub owner: &'static str,
    pub at: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub kind: &'static str,
    pub at: u32,
    pub size: u32,
    pub survives: &'static str,
    pub rings: &'static [Area],
    pub fuse: &'static [Area],
}

/// A register read at boot that must hold the definition's value, else
/// the isolation level is none.
#[derive(Clone, Copy, Debug)]
pub struct ReadBack {
    pub register: &'static str,
    pub addr: u32,
    pub mask: u32,
    pub shift: u8,
    pub expect: u32,
}

/// One protection region of an extension's map.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub number: u8,
    pub base: u32,
    pub size: u32,
    /// Disabled subregions (bit n: the n-th eighth).
    pub disabled: u8,
    pub writable: bool,
    pub execute: bool,
    /// Device memory (a register block: never cached, accesses in order);
    /// else normal memory.
    pub device: bool,
}

/// One extension of the chain, in chain order.
#[derive(Clone, Copy, Debug)]
pub struct Extension {
    pub id: &'static str,
    pub trusted: bool,
    pub max_run_us: u32,
    /// The accepted source SHA-256 (trusted extensions), hex.
    pub sha256: Option<&'static str>,
}

/// The regions loaded for each sandboxed extension's calls, in chain
/// order, for one boot core slot.
#[derive(Clone, Copy, Debug)]
pub struct SlotMap {
    pub slot: &'static str,
    pub maps: &'static [(&'static str, &'static [Region])],
}
";

fn board_values(out: &mut String, boot: Option<&BoardBoot>, board: Option<&BoardDef>) {
    let led = boot.and_then(|b| b.led.as_ref().map(|pin| (pin, b.led_active)));
    let _ = writeln!(
        out,
        "pub const LED: Option<Led> = {};",
        led.map_or("None".into(), |(pin, a)| format!(
            "Some(Led {{ pin: {pin:?}, active_high: {} }})",
            a == Active::High
        ))
    );
    let halt = boot.and_then(|b| b.halt).map(|h| {
        let p = match h.pattern {
            HaltPattern::Blink => "Blink",
            HaltPattern::Code => "Code",
            HaltPattern::Off => "Off",
        };
        format!(
            "Some(Halt {{ pattern: HaltPattern::{p}, period_ms: {} }})",
            h.period_ms
        )
    });
    let _ = writeln!(
        out,
        "pub const HALT: Option<Halt> = {};",
        halt.unwrap_or("None".into())
    );
    let hold = boot.and_then(|b| b.power_hold.as_ref()).map(|h| {
        format!(
            "Some(PowerHold {{ pin: {:?}, level: {} }})",
            h.pin,
            level(h.level)
        )
    });
    let _ = writeln!(
        out,
        "pub const POWER_HOLD: Option<PowerHold> = {};\n",
        hold.unwrap_or("None".into())
    );
    let _ = writeln!(out, "/// `boot-program` format of [`PROGRAM`].\npub const BOOT_PROGRAM_FORMAT: u32 = {BOOT_PROGRAM_FORMAT};\n");
    out.push_str("pub const PROGRAM: &[Action] = &[\n");
    for a in boot.map_or(&[][..], |b| &b.actions) {
        let ops: Vec<String> = a
            .ops
            .iter()
            .map(|op| match op {
                Op::Led { state } => format!(
                    "Op::Led(LedState::{})",
                    match state {
                        LedState::On => "On",
                        LedState::Off => "Off",
                        LedState::Toggle => "Toggle",
                    }
                ),
                Op::Gpio { pin, level: l } => format!("Op::Gpio {{ pin: {pin:?}, level: {} }}", level(*l)),
                Op::Delay { ms } => format!("Op::Delay {{ ms: {ms} }}"),
                Op::BusWrite { device, reg, bytes } => {
                    let d = board.and_then(|b| b.device(device)).expect("checked on load");
                    format!(
                        "Op::BusWrite {{ bus: {:?}, addr: {:#04x}, reg: {reg:#04x}, bytes: &{bytes:?} }}",
                        d.bus, d.addr
                    )
                }
            })
            .collect();
        let _ = writeln!(
            out,
            "    Action {{ on: {:?}, ops: &[{}] }},",
            a.on,
            ops.join(", ")
        );
    }
    out.push_str("];\n\n");
}

fn record(out: &mut String, def: &McuDef) {
    let Some(r) = def.record() else {
        out.push_str("pub const RECORD: Option<Record> = None;\n\n");
        return;
    };
    let area = |owner: String, at: u32, size: u32| {
        format!("Area {{ owner: {owner:?}, at: {at:#010x}, size: {size:#x} }}")
    };
    let rings: Vec<_> = r
        .rings
        .iter()
        .map(|x| area(format!("{:?}", x.writer).to_lowercase(), x.at, x.size))
        .collect();
    let fuse: Vec<_> = r
        .fuse
        .cells
        .iter()
        .map(|x| area(format!("{:?}", x.image).to_lowercase(), x.at, x.size))
        .collect();
    let _ = writeln!(
        out,
        "pub const RECORD: Option<Record> = Some(Record {{\n    kind: {:?},\n    at: {:#010x},\n    size: {:#x},\n    survives: {:?},\n    rings: &[{}],\n    fuse: &[{}],\n}});\n",
        r.kind.as_str(),
        r.at,
        r.size,
        r.survives.as_str(),
        rings.join(", "),
        fuse.join(", ")
    );
}

fn isolation(out: &mut String, def: &McuDef) {
    let i = def.isolation();
    let _ = writeln!(
        out,
        "pub const ISOLATION_LEVEL: &str = {:?};\npub const ISOLATION_KIND: &str = {:?};\n\
         /// Bound of a service's non-preemptible part.\npub const NP_MAX_US: u32 = {};\n\
         /// Bound of a DMA channel's stop.\npub const DMA_STOP_MAX_US: u32 = {};",
        i.level.as_str(),
        i.kind.as_str(),
        def.np_max_us(),
        def.dma_stop_max_us()
    );
    let reads: Vec<String> = kind::for_kind(i.kind)
        .map(|k| k.read_back(def))
        .unwrap_or_default()
        .iter()
        .map(|r| {
            format!(
                "ReadBack {{ register: {:?}, addr: {:#010x}, mask: {:#x}, shift: {}, expect: {} }}",
                r.register, r.addr, r.mask, r.shift, r.expect
            )
        })
        .collect();
    let _ = writeln!(
        out,
        "pub const READ_BACK: &[ReadBack] = &[{}];\n",
        reads.join(", ")
    );
}

fn chain(out: &mut String, exts: &[Ext<'_>]) {
    out.push_str("pub const EXTENSIONS: &[Extension] = &[\n");
    for e in exts {
        let m = e.manifest;
        let _ = writeln!(
            out,
            "    Extension {{ id: {:?}, trusted: {}, max_run_us: {}, sha256: {} }},",
            m.id,
            m.trusted,
            m.limits.max_run_us,
            e.sha256
                .as_ref()
                .map_or("None".into(), |h| format!("Some({h:?})"))
        );
    }
    out.push_str("];\n\n");
    let budgets: Vec<String> = exts
        .iter()
        .map(|e| e.manifest.limits.max_run_us.to_string())
        .collect();
    let _ = writeln!(
        out,
        "/// Each extension's time limit per call, us, in chain order\n\
         /// (`dagon_events::compose_boot`).\n\
         pub const BOOT_BUDGETS_US: &[u32] = &[{}];\n",
        budgets.join(", ")
    );
    let mut ty = String::from("dagon_events::Nop");
    let mut value = String::from("dagon_events::Nop");
    for e in exts.iter().rev() {
        ty = format!("dagon_events::Chain<{}::DagonExtension, {ty}>", e.krate);
        value = format!(
            "dagon_events::Chain::new({}::DAGON_EXTENSION, {value})",
            e.krate
        );
    }
    let _ = writeln!(
        out,
        "/// The boot core's extensions in chain order (each crate exports\n\
         /// `DagonExtension` and `DAGON_EXTENSION`).\n\
         pub type BootChain = {ty};\npub const BOOT_CHAIN: BootChain = {value};\n"
    );
}

fn region_lit(r: &kind::Region) -> String {
    format!(
        "Region {{ number: {}, base: {:#010x}, size: {:#x}, disabled: {:#04x}, writable: {}, execute: {}, device: {} }}",
        r.number,
        r.base,
        r.size,
        r.disabled,
        r.access == kind::Access::ReadWrite,
        r.execute,
        r.memory == kind::Memory::Device
    )
}

fn maps(out: &mut String, maps: &[(Binding, Map)]) {
    out.push_str("pub const MAPS: &[SlotMap] = &[\n");
    for (b, map) in maps {
        let _ = writeln!(
            out,
            "    SlotMap {{\n        slot: {:?},\n        maps: &[",
            binding(*b)
        );
        for e in &map.exts {
            let rs: Vec<_> = e.regions.iter().map(region_lit).collect();
            let _ = writeln!(
                out,
                "            ({:?}, &[\n                {},\n            ]),",
                e.id,
                rs.join(",\n                ")
            );
        }
        out.push_str("        ],\n    },\n");
    }
    out.push_str("];\n");
}

/// `boot_wiring.rs` for a node whose MCU has a DAGON boot stage.
pub fn boot_wiring(
    node: &str,
    def: &McuDef,
    board: Option<&BoardDef>,
    plan: &ImagePlan<'_>,
    known: &Known,
) -> Result<String, String> {
    let boot = board.and_then(|b| b.boot.as_ref());
    if let Some(b) = boot {
        check_program(b, known)?;
    }
    let mut out = format!(
        "//! Generated by `dagon gen` for node {node:?} (MCU {}{}). Do not edit:\n\
         //! dagon gen rewrites this file; change the definitions, the extension\n\
         //! manifests or dagon.toml instead.\n\n\
         #![allow(dead_code)]\n\n{TYPES}\n",
        def.id,
        board.map_or(String::new(), |b| format!(", board {}", b.id))
    );
    board_values(&mut out, boot, board);
    if let Some(w) = def.watchdog {
        let _ = writeln!(
            out,
            "pub const WATCHDOG_KIND: &str = {:?};\npub const WATCHDOG_TRIAL_MS: u32 = {};\n\
             pub const WATCHDOG_CONFIRMED_MS: u32 = {};\n\
             /// Longest an app may delay a requested reboot.\npub const REBOOT_DELAY_MAX_MS: u32 = {};\n",
            w.kind.as_str(),
            w.trial_ms,
            w.confirmed_ms,
            w.reboot_delay_max_ms()
        );
    }
    record(&mut out, def);
    isolation(&mut out, def);
    chain(&mut out, &plan.exts);
    maps(&mut out, &plan.maps);
    Ok(out)
}

/// The linker fragment of one image slot: the image's own FLASH and RAM
/// after the extension regions, each extension's regions with their
/// sections, and the boot record and fuse cells as their own regions.
pub fn linker_fragment(
    node: &str,
    def: &McuDef,
    plan: &ImagePlan<'_>,
    slot: Binding,
    space: &Space,
) -> String {
    let mut out = format!(
        "/* Generated by `dagon gen` for node {node:?}, {} image slot {} on {}. Do not edit. */\n\nMEMORY\n{{\n",
        plan.image.name(),
        binding(slot),
        def.id
    );
    let _ = writeln!(
        out,
        "  FLASH : ORIGIN = {:#010x}, LENGTH = {:#x}",
        space.flash_at, space.flash_size
    );
    let _ = writeln!(
        out,
        "  RAM : ORIGIN = {:#010x}, LENGTH = {:#x}",
        space.ram_at, space.ram_size
    );
    let map = plan.maps.iter().find(|(b, _)| *b == slot).map(|(_, m)| m);
    let regions: Vec<(String, &kind::Region)> = map
        .into_iter()
        .flat_map(|m| m.exts.iter().enumerate())
        .flat_map(|(i, e)| {
            e.regions
                .iter()
                .filter(|r| !matches!(r.purpose, kind::Purpose::Window(_)))
                .map(move |r| {
                    (
                        format!("EXT{i}_{}", r.purpose.to_string().to_uppercase()),
                        r,
                    )
                })
        })
        .collect();
    for (name, r) in &regions {
        let _ = writeln!(
            out,
            "  {name} : ORIGIN = {:#010x}, LENGTH = {:#x}",
            r.base, r.size
        );
    }
    if let Some(r) = def.record() {
        let _ = writeln!(
            out,
            "  BOOT_RECORD : ORIGIN = {:#010x}, LENGTH = {:#x}",
            r.at, r.size
        );
        for c in &r.fuse.cells {
            let _ = writeln!(
                out,
                "  FUSE_{} : ORIGIN = {:#010x}, LENGTH = {:#x}",
                format!("{:?}", c.image).to_uppercase(),
                c.at,
                c.size
            );
        }
    }
    out.push_str("}\n");
    if let Some(m) = map {
        out.push_str("\nSECTIONS\n{\n");
        for (i, e) in m.exts.iter().enumerate() {
            let s = e.id.replace('-', "_");
            let _ = writeln!(
                out,
                "  .ext.{s}.text : {{ KEEP(*(.ext.{s}.text .ext.{s}.text.* .ext.{s}.rodata .ext.{s}.rodata.*)) }} > EXT{i}_TEXT\n  \
                 .ext.{s}.data : {{ *(.ext.{s}.data .ext.{s}.data.*) }} > EXT{i}_DATA AT > EXT{i}_TEXT\n  \
                 .ext.{s}.bss (NOLOAD) : {{ *(.ext.{s}.bss .ext.{s}.bss.*) }} > EXT{i}_DATA\n  \
                 __dagon_ext{i}_stack_bottom = ORIGIN(EXT{i}_STACK);\n  \
                 __dagon_ext{i}_stack_top = ORIGIN(EXT{i}_STACK) + LENGTH(EXT{i}_STACK);"
            );
        }
        out.push_str("}\nINSERT AFTER .bss;\n");
        for (i, e) in m.exts.iter().enumerate() {
            let s = e.id.replace('-', "_");
            let _ = writeln!(
                out,
                "ASSERT(SIZEOF(.ext.{s}.text) + SIZEOF(.ext.{s}.data) <= LENGTH(EXT{i}_TEXT), \"extension {} code is over its limits.text_bytes\");\n\
                 ASSERT(SIZEOF(.ext.{s}.data) + SIZEOF(.ext.{s}.bss) <= LENGTH(EXT{i}_DATA), \"extension {} data is over its limits.ram_bytes\");",
                e.id, e.id
            );
        }
    }
    if def.record().is_some() {
        out.push_str(
            "\n__dagon_boot_record_start = ORIGIN(BOOT_RECORD);\n__dagon_boot_record_size = LENGTH(BOOT_RECORD);\n",
        );
    }
    out
}

/// A readable copy of every planned map of a node.
pub fn protection_map(node: &str, def: &McuDef, plans: &[ImagePlan<'_>]) -> String {
    let i = def.isolation();
    let mut out = format!(
        "# Generated by `dagon gen` for node {node:?}. Do not edit.\n\
         # The protection map each sandboxed extension's calls load.\n\n\
         node = {node:?}\nmcu = {:?}\nlevel = {:?}\nkind = {:?}\n",
        def.id,
        i.level.as_str(),
        i.kind.as_str()
    );
    for p in plans {
        let _ = write!(
            out,
            "\n[[image]]\nimage = {:?}\nextensions = [{}]\n",
            p.image.name(),
            p.exts
                .iter()
                .map(|e| format!("{:?}", e.manifest.id))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let notes: Vec<String> = p.notes.iter().map(|n| format!("{n:?}")).collect();
        let _ = writeln!(out, "notes = [{}]", notes.join(", "));
        for (b, space) in &p.memory {
            let _ = write!(
                out,
                "\n[[image.slot]]\nslot = {:?}\nflash = {{ at = \"{:#010x}\", size = \"{:#x}\" }}\nram = {{ at = \"{:#010x}\", size = \"{:#x}\" }}\n",
                binding(*b),
                space.flash_at,
                space.flash_size,
                space.ram_at,
                space.ram_size
            );
            let Some((_, map)) = p.maps.iter().find(|(x, _)| x == b) else {
                continue;
            };
            for e in &map.exts {
                let _ = writeln!(
                    out,
                    "\n[[image.slot.extension]]\nid = {:?}\nregions = [",
                    e.id
                );
                for r in &e.regions {
                    let _ = writeln!(
                        out,
                        "  {{ number = {}, base = \"{:#010x}\", size = \"{:#x}\", disabled = \"{:#04x}\", access = {:?}, execute = {}, memory = {:?}, purpose = {:?} }},",
                        r.number, r.base, r.size, r.disabled, r.access.as_str(), r.execute, r.memory.as_str(), r.purpose.to_string()
                    );
                }
                out.push_str("]\n");
            }
        }
    }
    out
}
