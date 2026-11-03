//! What the engines consuming a definition assume, declared as data. Every
//! MCU and board definition is checked against all of them on load; the
//! engines (dagon-sign's FlashImage, UF2 and BIN builders and log-face
//! initialiser, the slot/header placement and partition resolution here)
//! build on the same constants and checks. A test fails when a numeric
//! definition field has no requirement declared here.

use std::ops::RangeInclusive;

use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{FACE_HEADER_SIZE, RECORD_SIZE};

use super::{Binding, BoardDef, McuDef, Placement, Writer};

/// Slot/header placement: an image header.
pub const HEADER: u32 = HEADER_SIZE as u32;
/// FlashImage page and UF2 block payload.
pub const PAGE: u32 = 256;
/// Log-face initialiser: face header + PROVISION + CONFIRMED.
pub const LOG_FACE_MIN: u32 = (FACE_HEADER_SIZE + 2 * RECORD_SIZE) as u32;
/// espflash writer: the sector the partition table is written to.
pub const PARTITION_TABLE_LEN: u32 = 0x1000;
/// 7-bit I2C addresses outside the reserved ranges.
pub const I2C_ADDRS: RangeInclusive<u8> = 0x08..=0x77;

/// UF2 builder: whole sectors of whole pages, aligned by masking.
pub fn uf2_sector_ok(sector: u32) -> bool {
    sector.is_power_of_two() && sector >= PAGE
}

/// One engine's assumption about the fields it reads; `C` is its check.
pub struct Requirement<C: 'static> {
    pub engine: &'static str,
    /// Definition keys, `[]` for array elements (`flash_map[].base`).
    pub fields: &'static [&'static str],
    pub rule: &'static str,
    pub(super) check: C,
}

type McuCheck = fn(&McuDef) -> Result<(), String>;
/// A board's requirements see its MCU too.
type BoardCheck = fn(&BoardDef, &McuDef) -> Result<(), String>;

fn any(_: &McuDef) -> Result<(), String> {
    Ok(())
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

/// Exclusive end of `at + size` when it fits in 32 bits.
fn end(at: u32, size: u32) -> Option<u32> {
    at.checked_add(size)
}

pub const MCU: &[Requirement<McuCheck>] = &[
    Requirement {
        engine: "image header",
        fields: &["wire_id"],
        rule: "not 0 (unique across MCUs: registry)",
        check: |m| fail(m.wire_id == 0, || "wire_id 0 is not an MCU id".into()),
    },
    Requirement {
        engine: "topology check V-07",
        fields: &["gpio_count"],
        rule: "any; 0 = pins named by port",
        check: any,
    },
    Requirement {
        engine: "FlashImage / BIN builder",
        fields: &["flash_map[].base", "flash_map[].bytes"],
        rule: "at least one map; bytes >= 1; base a multiple of PAGE; base + bytes <= 2^32",
        check: flash_maps,
    },
    Requirement {
        engine: "partition resolution",
        fields: &["partitions.csv offset", "partitions.csv size"],
        rule: "size >= 1; offset + size <= 2^32",
        check: partitions,
    },
    Requirement {
        engine: "slot/header placement",
        fields: &[
            "slot[].at",
            "slot[].size",
            "slot[].header_at",
            "slot[].header_offset",
        ],
        rule: "inline/trailer slots > HEADER, detached >= 1, headers HEADER; one slot per image and binding",
        check: slots,
    },
    Requirement {
        engine: "flash placement (all engines' regions)",
        fields: &[
            "layout.stage0.at",
            "layout.stage0.size",
            "layout.bootcore_log.faces[]",
            "layout.app_log.faces[]",
            "writer.table_at",
        ],
        rule: "every region (slots, headers, stage0 >= 1, log faces, partition table) inside every flash map, none overlapping",
        check: placement,
    },
    Requirement {
        engine: "log-face initialiser",
        fields: &["layout.bootcore_log.face_size", "layout.app_log.face_size"],
        rule: "face_size >= LOG_FACE_MIN (96)",
        check: log_faces,
    },
    Requirement {
        engine: "UF2 builder",
        fields: &["writer.sector"],
        rule: "power of two, >= PAGE; the last sector of every flash map ends below 2^32",
        check: uf2_sector,
    },
    Requirement {
        engine: "UF2 builder",
        fields: &["writer.family"],
        rule: "any (UF2 family id)",
        check: any,
    },
    Requirement {
        engine: "espflash writer (labels)",
        fields: &["writer.usb_hint.vid", "writer.usb_hint.pid"],
        rule: "any; labels candidates, never filters them",
        check: any,
    },
    Requirement {
        engine: "topology check V-09/V-10",
        fields: &[
            "coordinator.pio_state_machines",
            "coordinator.pio_uart_state_machines",
            "coordinator.uart0_tx[]",
            "coordinator.uart0_rx[]",
        ],
        rule: "1 <= pio_uart_state_machines <= pio_state_machines; UART0 pins below gpio_count (when numbered)",
        check: coordinator,
    },
    Requirement {
        engine: "topology check V-18/V-20, host ROM path",
        fields: &["rom_i2c.addr", "rom_i2c.max_hz", "rom_i2c.erase_sectors[]"],
        rule: "addr in 0x08..=0x77; max_hz >= 1; erase_sectors unique",
        check: rom_i2c,
    },
];

pub const BOARD: &[Requirement<BoardCheck>] = &[
    Requirement {
        engine: "topology check V-16",
        fields: &["flash_bytes"],
        rule: "holds one of the MCU's flash maps",
        check: |b, m| {
            fail(
                !m.flash_maps.iter().any(|f| f.bytes <= b.flash_bytes),
                || {
                    format!(
                        "flash_bytes {:#x} holds none of {}'s flash maps",
                        b.flash_bytes, m.id
                    )
                },
            )
        },
    },
    Requirement {
        engine: "topology default link",
        fields: &["default_link.addr", "default_link.baud"],
        rule: "addr in 0x08..=0x77; baud >= 1",
        check: |b, _| {
            let Some(l) = b.default_link else {
                return Ok(());
            };
            fail(l.addr.is_some_and(|a| !I2C_ADDRS.contains(&a)), || {
                "default_link.addr is outside 0x08..=0x77".into()
            })?;
            fail(l.baud == Some(0), || "default_link.baud is 0".into())
        },
    },
];

/// Every MCU requirement, the first failure named by its engine.
pub fn check_mcu(m: &McuDef) -> Result<(), String> {
    MCU.iter()
        .try_for_each(|r| (r.check)(m).map_err(|e| format!("{e} [{}]", r.engine)))
}

/// Every board requirement against its MCU.
pub fn check_board(b: &BoardDef, m: &McuDef) -> Result<(), String> {
    BOARD
        .iter()
        .try_for_each(|r| (r.check)(b, m).map_err(|e| format!("{e} [{}]", r.engine)))
}

/// Every declared field: MCU keys, then board keys.
pub fn declared_fields() -> (Vec<&'static str>, Vec<&'static str>) {
    let mcu = MCU.iter().flat_map(|r| r.fields.iter().copied()).collect();
    let board = BOARD
        .iter()
        .flat_map(|r| r.fields.iter().copied())
        .collect();
    (mcu, board)
}

fn flash_maps(m: &McuDef) -> Result<(), String> {
    fail(m.flash_maps.is_empty(), || {
        "needs at least one flash_map".into()
    })?;
    m.flash_maps.iter().try_for_each(|f| {
        fail(f.bytes == 0 || end(f.base, f.bytes).is_none(), || {
            format!(
                "flash map '{}' ({:#x}+{:#x}) is empty or runs past the 32-bit address space",
                f.name, f.base, f.bytes
            )
        })?;
        fail(f.base % PAGE != 0, || {
            format!(
                "flash map '{}' base {:#x} is not a multiple of {PAGE:#x}",
                f.name, f.base
            )
        })
    })
}

fn partitions(m: &McuDef) -> Result<(), String> {
    m.partition_table.iter().try_for_each(|p| {
        fail(p.size == 0 || end(p.offset, p.size).is_none(), || {
            format!(
                "partition '{}' ({:#x}+{:#x}) is empty or runs past the 32-bit address space",
                p.name, p.offset, p.size
            )
        })
    })
}

fn slots(m: &McuDef) -> Result<(), String> {
    for (i, (s, def)) in m.slots.iter().zip(&m.slot_defs).enumerate() {
        let what = format!("slot {:?}/{:?}", s.image, s.binding);
        let min = if s.placement == Placement::Detached {
            1
        } else {
            HEADER + 1
        };
        fail(s.size < min, || {
            format!(
                "{what}: {:#x} bytes cannot hold a header and a payload",
                s.size
            )
        })?;
        fail(end(s.start, s.size).is_none(), || {
            format!(
                "{what}: {:#x}+{:#x} runs past the 32-bit address space",
                s.start, s.size
            )
        })?;
        let in_partition = def.header_partition.as_ref().map(|name| {
            let p = m.partition_table.iter().find(|p| &p.name == name);
            (name, p.map_or(0, |p| p.size))
        });
        if let Some((name, size)) = in_partition {
            fail(
                u64::from(def.header_offset) + u64::from(HEADER) > u64::from(size),
                || format!("{what}: header_offset is outside partition '{name}'"),
            )?;
        }
        fail(end(s.header_addr, HEADER).is_none(), || {
            format!(
                "{what}: header at {:#x} runs past the 32-bit address space",
                s.header_addr
            )
        })?;
        let clash = m.slots[..i].iter().any(|o| {
            o.image == s.image
                && (o.binding == s.binding
                    || o.binding == Binding::Any
                    || s.binding == Binding::Any)
        });
        fail(clash, || format!("{what} is defined twice"))?;
    }
    Ok(())
}

/// The regions the engines write, each at its engine's minimum size.
fn regions(m: &McuDef) -> Vec<(String, u32, u32)> {
    let mut out = Vec::new();
    for s in &m.slots {
        let name = format!("slot {:?}/{:?}", s.image, s.binding);
        if s.placement == Placement::Detached {
            out.push((format!("{name} header"), s.header_addr, HEADER));
        }
        out.push((name, s.start, s.size));
    }
    if let Some(r) = m.layout.stage0 {
        out.push(("stage0".into(), r.at, r.size));
    }
    for (name, log) in [
        ("bootcore_log", m.layout.bootcore_log),
        ("app_log", m.layout.app_log),
    ] {
        for (i, face) in log
            .iter()
            .flat_map(|l| l.faces.map(|f| (f, l.face_size)))
            .enumerate()
        {
            out.push((format!("{name} face {i}"), face.0, face.1));
        }
    }
    if let Writer::Espflash { table_at, .. } = m.writer {
        out.push(("partition table".into(), table_at, PARTITION_TABLE_LEN));
    }
    out
}

fn placement(m: &McuDef) -> Result<(), String> {
    let regions = regions(m);
    let end = |at: u32, size: u32| u64::from(at) + u64::from(size);
    for f in &m.flash_maps {
        for (name, at, size) in &regions {
            fail(
                *size == 0 || *at < f.base || end(*at, *size) > end(f.base, f.bytes),
                || {
                    format!(
                        "{name} ({at:#x}+{size:#x}) is outside flash map '{}'",
                        f.name
                    )
                },
            )?;
        }
    }
    for (i, (a, a_at, a_size)) in regions.iter().enumerate() {
        for (b, b_at, b_size) in &regions[i + 1..] {
            let overlap =
                u64::from(*a_at) < end(*b_at, *b_size) && u64::from(*b_at) < end(*a_at, *a_size);
            fail(overlap, || format!("{a} overlaps {b}"))?;
        }
    }
    Ok(())
}

fn log_faces(m: &McuDef) -> Result<(), String> {
    for (name, log) in [
        ("bootcore_log", m.layout.bootcore_log),
        ("app_log", m.layout.app_log),
    ] {
        let Some(l) = log else { continue };
        fail(l.face_size < LOG_FACE_MIN, || {
            format!(
                "{name}.face_size {:#x} is below the {LOG_FACE_MIN} bytes a provisioned face needs",
                l.face_size
            )
        })?;
    }
    Ok(())
}

fn uf2_sector(m: &McuDef) -> Result<(), String> {
    let Writer::Uf2Drive { sector, .. } = m.writer else {
        return Ok(());
    };
    fail(!uf2_sector_ok(sector), || {
        format!("writer.sector {sector:#x} must be a power of two of at least {PAGE:#x}")
    })?;
    m.flash_maps.iter().try_for_each(|f| {
        let last = u64::from(f.base) + u64::from(f.bytes) - 1;
        let sector_end = (last & !(u64::from(sector) - 1)) + u64::from(sector);
        // The builder computes sector ends in u32: the last may not be 2^32.
        fail(sector_end >= 1 << 32, || {
            format!(
                "writer.sector {sector:#x} rounds flash map '{}' past 2^32",
                f.name
            )
        })
    })
}

fn coordinator(m: &McuDef) -> Result<(), String> {
    let Some(c) = &m.coordinator else {
        return Ok(());
    };
    fail(
        c.pio_uart_state_machines == 0 || c.pio_uart_state_machines > c.pio_state_machines,
        || "pio_uart_state_machines must be 1..=pio_state_machines".into(),
    )?;
    let bad = c
        .uart0_tx
        .iter()
        .chain(&c.uart0_rx)
        .find(|&&g| m.gpio_count > 0 && g >= m.gpio_count);
    fail(bad.is_some(), || {
        format!(
            "UART0 pin GPIO{} is not below gpio_count {}",
            bad.unwrap(),
            m.gpio_count
        )
    })
}

fn rom_i2c(m: &McuDef) -> Result<(), String> {
    let Some(r) = &m.rom_i2c else { return Ok(()) };
    fail(!I2C_ADDRS.contains(&r.addr), || {
        format!("rom_i2c.addr {:#04x} is outside 0x08..=0x77", r.addr)
    })?;
    fail(r.max_hz == 0, || "rom_i2c.max_hz is 0".into())?;
    let mut seen = std::collections::BTreeSet::new();
    fail(!r.erase_sectors.iter().all(|s| seen.insert(s)), || {
        "rom_i2c.erase_sectors repeats a sector".into()
    })
}
