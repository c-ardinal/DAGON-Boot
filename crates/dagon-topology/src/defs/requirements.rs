//! What the engines consuming a definition assume, declared as data. Every
//! MCU and board definition is checked against all of them on load; the
//! engines (dagon-sign's FlashImage, UF2 and BIN builders and log-face
//! initialiser, the slot/header placement and partition resolution here)
//! build on the same constants and checks. A test fails when a numeric
//! definition field has no requirement declared here.

use std::ops::RangeInclusive;

use dagon_format::header::HEADER_SIZE;
use dagon_format::statelog::{FACE_HEADER_SIZE, RECORD_SIZE};

use super::{
    board_boot, clock, dma, execution, isolation, slot_layout, Binding, BoardDef, ImageKind,
    McuDef, Placement, Writer,
};

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
        engine: "definition reader",
        fields: &["format"],
        rule: "FORMAT (left out is 1); checked before the other fields are read",
        check: any,
    },
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
        rule: "inline/trailer slots > HEADER, detached >= 1, headers HEADER; one slot per image and binding; a b slot only with an a slot of its image",
        check: slots,
    },
    Requirement {
        engine: "image writers (agent and boot stages)",
        fields: &[],
        rule: "one entry per image kind with a slot, naming only kinds with a slot; a kind with one slot is written by a kind other than itself",
        check: writers,
    },
    Requirement {
        engine: "boot stage listen window (boot core, recovery agent)",
        fields: &["boot_receive.listen_window_ms"],
        rule: "window 0..=200 ms; an enumerated kind (usb-cdc) does not take part",
        check: boot_receive,
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
        engine: "stage0 (its build and its key)",
        fields: &["stage0.region_bytes", "stage0.head_bytes"],
        rule: "[stage0] exactly when layout.stage0; region_bytes = layout.stage0.size; head_bytes < region_bytes; key one of KEY_PLACES",
        check: stage0,
    },
    Requirement {
        engine: "stage0 clock setup (its build)",
        fields: &[
            "stage0.clock[].in_hz",
            "stage0.clock[].out_hz",
            "stage0.clock[].pll.refdiv",
            "stage0.clock[].pll.fbdiv",
            "stage0.clock[].pll.postdiv1",
            "stage0.clock[].pll.postdiv2",
            "stage0.clock[].pll.m",
            "stage0.clock[].pll.n",
            "stage0.clock[].pll.p",
            "stage0.clock[].pll.q",
            "stage0.clock[].pll.flash_wait_states",
        ],
        rule: "exactly one internal setup (out_hz only); other setups with in_hz and pll fields that name a PLL_RULES row, whose rules they meet; frequencies above 0; no two setups of one source and in_hz with the same out_hz",
        check: clock::check_mcu,
    },
    Requirement {
        engine: "boot loader and app clock setup (their HAL, from their build and wiring.rs)",
        fields: &[
            "bootcore.clock[].in_hz",
            "bootcore.clock[].out_hz",
            "bootcore.clock[].pll.refdiv",
            "bootcore.clock[].pll.fbdiv",
            "bootcore.clock[].pll.postdiv1",
            "bootcore.clock[].pll.postdiv2",
            "bootcore.clock[].pll.usb_refdiv",
            "bootcore.clock[].pll.usb_fbdiv",
            "bootcore.clock[].pll.usb_postdiv1",
            "bootcore.clock[].pll.usb_postdiv2",
            "bootcore.clock[].pll.m",
            "bootcore.clock[].pll.n",
            "bootcore.clock[].pll.p",
            "bootcore.clock[].pll.q",
            "bootcore.clock[].pll.flash_wait_states",
            "app.clock[].in_hz",
            "app.clock[].out_hz",
            "app.clock[].pll.refdiv",
            "app.clock[].pll.fbdiv",
            "app.clock[].pll.postdiv1",
            "app.clock[].pll.postdiv2",
            "app.clock[].pll.usb_refdiv",
            "app.clock[].pll.usb_fbdiv",
            "app.clock[].pll.usb_postdiv1",
            "app.clock[].pll.usb_postdiv2",
            "app.clock[].pll.m",
            "app.clock[].pll.n",
            "app.clock[].pll.p",
            "app.clock[].pll.q",
            "app.clock[].pll.flash_wait_states",
            "app.clock[].tolerance_ppm",
        ],
        rule: "exactly when a bootcore slot; per table exactly one internal setup (out_hz, or in_hz and a pll); other setups with in_hz; pll fields that name a PLL_RULES row serving the stage, whose rules they meet; frequencies above 0; no two setups of one source and in_hz with the same out_hz",
        check: clock::check_boot_loader_and_app,
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
        engine: "topology check V-26",
        fields: &[
            "link_uart.clock_hz",
            "link_uart.clocks_per_bit",
            "link_uart.frac_bits",
            "link_uart.bauds[]",
            "link_uart.fast_ppm",
            "link_uart.slow_ppm",
        ],
        rule: "exactly one of clock_hz >= 1 and clock = \"app\" (with [[app.clock]]); clocks_per_bit >= 1; frac_bits <= 24; bauds not empty, each one the UART makes within fast_ppm and slow_ppm on clock_hz (on clock = \"app\" V-26 checks each node's); slow_ppm <= 1000000",
        check: link_uart,
    },
    Requirement {
        engine: "topology check V-20",
        fields: &[
            "link_i2c.hzs[]",
            "link_i2c.controller.min_period",
            "link_i2c.controller.fast_ppm",
            "link_i2c.controller.slow_ppm",
        ],
        rule: "hzs not empty; controller: exactly one of clock_hz >= 1 and clock = \"app\" (with [[app.clock]]), min_period >= 1, slow_ppm <= 1000000, each of hzs one it makes within fast_ppm and slow_ppm on clock_hz (on clock = \"app\" V-20 checks each node's)",
        check: link_i2c,
    },
    Requirement {
        engine: "relay (dagon-relay, the host, the MCU package's generated RELAY_LINKS)",
        fields: &["relay_link[].id", "relay_link[].addressing.fixed", "relay_link[].max_payload"],
        rule: "id 1-254, unique; one entry per port, a [[link_port]] name; seven-bit on a kind with addresses, fixed (0-255) on one without; max_payload 1 up to the kind's protocol maximum",
        check: relay_links,
    },
    Requirement {
        engine: "relay firmware RAM check (scripts/relay_ram.py, CI)",
        fields: &["relay.ram_bytes"],
        rule: "[relay] only with [[relay_link]]; ram_bytes 1 up to the MCU's RAM (ram.size, when given)",
        check: relay_firmware,
    },
    Requirement {
        engine: "codec budget check (scripts/codec_budget.py, CI)",
        fields: &[
            "codec.bootcore.flash_bytes",
            "codec.bootcore.stack_bytes",
            "codec.bootcore.ram_bytes",
            "codec.relay.flash_bytes",
            "codec.relay.stack_bytes",
            "codec.relay.ram_bytes",
        ],
        rule: "[codec.bootcore] only with a [[slot_layout]] of target bootcore, [codec.relay] only with [[relay_link]]; stack_bytes and ram_bytes up to the MCU's RAM (ram.size, when given)",
        check: codec_budgets,
    },
    Requirement {
        engine: "topology check V-27, compiled topology (a link end's port index)",
        fields: &["link_port[].index"],
        rule: "index and name each used once; name a-z, 0-9 and '-'",
        check: link_ports,
    },
    Requirement {
        engine: "topology check V-18/V-20, host ROM path",
        fields: &["rom_i2c.addr", "rom_i2c.max_hz", "rom_i2c.erase_sectors[]"],
        rule: "addr in 0x08..=0x77; max_hz >= 1; erase_sectors unique; each pin pair (sda, scl) once, so a pair names one controller",
        check: rom_i2c,
    },
    Requirement {
        engine: "protection map generator (kind record)",
        fields: &[
            "isolation.ext_ram_max",
            "isolation.capabilities.mpu_regions",
            "isolation.capabilities.min_region",
            "isolation.capabilities.subregions_from",
            "isolation.capabilities.np_max_us",
            "isolation.capabilities.dma_stop_max_us",
            "isolation.capabilities.worlds",
            "isolation.capabilities.gdma_channels",
        ],
        rule: "the kind's own fields only, in the sets its architecture allows; level one the kind gives; np_max_us / dma_stop_max_us only lower the catalog's; ext_ram_max >= 1",
        check: isolation::capabilities,
    },
    Requirement {
        engine: "isolation checker",
        fields: &["peripheral[].base", "peripheral[].size"],
        rule: "blocks unique, non-empty, apart; bus masters never window-eligible; every other block names privileged_why; a kind other than none lists every core and a block of each required class",
        check: isolation::peripherals,
    },
    Requirement {
        engine: "DMA stream assignment (board packages, MCU templates)",
        fields: &[
            "peripheral[].dma.streams",
            "peripheral[].dma.channels",
            "peripheral[].dma_requests[].stream",
            "peripheral[].dma_requests[].channel",
        ],
        rule: "controllers are bus masters with streams and channels >= 1; each request names a controller, a stream and a channel it has; one cell serves one block; no request twice",
        check: dma::check,
    },
    Requirement {
        engine: "watchdog driver, reboot delay cap",
        fields: &[
            "watchdog.trial_ms",
            "watchdog.confirmed_ms",
            "watchdog.trial_attempts",
            "watchdog.range_ms.min",
            "watchdog.range_ms.max",
        ],
        rule: ">= 1; 1 <= range_ms.min <= range_ms.max (the periods the MCU's watchdog can run); trial_ms and confirmed_ms inside range_ms; flag a fact of none | register | package (lockup fuse only with a source)",
        check: isolation::watchdog,
    },
    Requirement {
        engine: "execution model (flash operations, masking budget)",
        fields: &[
            "execution.masked_max_us",
            "execution.write_unit",
            "execution.erased_value",
            "execution.erase_regions[].start",
            "execution.erase_regions[].len",
            "execution.erase_regions[].unit",
            "execution.erase_regions[].unit_max_ms",
            "execution.erase_regions[].unit_max_ms.unverified",
        ],
        rule: "each fact a number (checked against its source), { unverified = N } or \"unmeasured\" (unit_max_ms needs a number either way); masked_max_us 1..=MASKED_MAX_US_LIMIT; at least one erase region; each region's len a whole number of units >= 1, start + len <= 2^32, no two overlapping; unit_max_ms >= 1, and 2 x unit_max_ms <= watchdog.trial_ms and watchdog.confirmed_ms; write_unit a power of two dividing every region's unit",
        check: execution::execution,
    },
    Requirement {
        engine: "erase planner (dagon_port::timeout, node and host)",
        fields: &[
            "slot_layout[].start",
            "slot_layout[].capacity",
            "slot_layout[].header.detached",
        ],
        rule: "only with [execution]; one per target and slot (topology: slot a); each slot[] image's target and slots have one, the slot it names agreeing with its start, largest body and header; a named partition is its start and size; the erase of the largest body, header included, lies in execution.erase_regions",
        check: slot_layout::slot_layouts,
    },
    Requirement {
        engine: "conformance build (CONF_RUN scratch)",
        fields: &["conformance.scratch.at", "conformance.scratch.size"],
        rule: "size >= 1; inside the default flash map; apart from every slot, header, log face and stage0",
        check: execution::conformance,
    },
    Requirement {
        engine: "boot record and lockup fuse placement (memory.x fragments)",
        fields: &[
            "ram.at",
            "ram.size",
            "boot.record.at",
            "boot.record.size",
            "boot.record.entropy.min_bits",
            "boot.record.rings[].at",
            "boot.record.rings[].size",
            "boot.record.fuse.cells[].at",
            "boot.record.fuse.cells[].size",
        ],
        rule: "entropy min_bits a fact, at least ENTROPY_MIN_BITS (16) when it has a value; record 1..=RECORD_MAX bytes inside RAM (noinit-ram) or a declared block (retained-registers); 1..=16 rings inside it, apart, one per boot stage the MCU has; fuse cells apart, outside the record, one per image kind",
        check: isolation::record,
    },
];

pub const BOARD: &[Requirement<BoardCheck>] = &[
    Requirement {
        engine: "definition reader",
        fields: &["format"],
        rule: "FORMAT (left out is 1); checked before the other fields are read",
        check: |_, _| Ok(()),
    },
    Requirement {
        engine: "stage0 clock choice (its build)",
        fields: &["clock.source.crystal_hz", "clock.source.external_hz"],
        rule: "above 0; a source no stage0.clock setup of the MCU matches runs stage0 on the internal setup",
        check: clock::check_board,
    },
    Requirement {
        engine: "erase planner (dagon_port::timeout, node and host), with the board's MCU",
        fields: &[
            "flash.erase_regions[].start",
            "flash.erase_regions[].len",
            "flash.erase_regions[].unit",
            "flash.erase_regions[].unit_max_ms",
            "flash.erase_regions[].unit_max_ms.unverified",
        ],
        rule: "part named; the MCU's erase-region rules against the MCU's watchdog; the erase of each MCU slot layout's largest body lies in them",
        check: slot_layout::board_flash,
    },
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
        rule: "addr in 0x08..=0x77; baud one of the MCU's link_uart.bauds",
        check: |b, m| {
            let Some(l) = b.default_link else {
                return Ok(());
            };
            fail(l.addr.is_some_and(|a| !I2C_ADDRS.contains(&a)), || {
                "default_link.addr is outside 0x08..=0x77".into()
            })?;
            fail(l.baud == Some(0), || "default_link.baud is 0".into())?;
            default_link_baud(b, m)
        },
    },
    Requirement {
        engine: "page link defaults, topology check V-07/V-17",
        fields: &[],
        rule: "coordinator pins (the dagon = true I2C and PIO UART buses' GPIOs, on an MCU with [coordinator]): below its gpio_count",
        check: hub_pins,
    },
    Requirement {
        engine: "extension capability validator",
        fields: &[
            "device[].addr",
            "device[].max_transfer",
            "device[].writable[]",
            "device[].regs[].reg",
        ],
        rule: "names unique; buses and peripherals name MCU blocks, a peripheral never one that drives DAGON's link; device ids and register names usable as code names, writable names among the regs, max_transfer at least 1; devices on a declared bus, I2C addresses in 0x08..=0x77",
        check: board_boot::tables,
    },
    Requirement {
        engine: "board package code generation (defs::codegen)",
        fields: &["bus[].hz", "bus[].baud"],
        rule: "pin names and bus ids usable as code names, each generated name once after - becomes _ (device ids too); each bus names its kind's pins; hz on i2c/spi, baud on uart/can, above 0; no pin used twice",
        check: board_boot::wiring,
    },
    Requirement {
        engine: "DMA stream assignment (defs::dma::board_streams)",
        fields: &[],
        rule: "dma only on i2c, spi and uart buses; every such bus gets a tx and an rx stream from the MCU's request mapping, no stream twice",
        check: |b, m| dma::board_streams(b, m).map(|_| ()),
    },
    Requirement {
        engine: "boot action interpreter (boot-program 1)",
        fields: &[
            "boot.halt.period_ms",
            "boot.action[].ops[].ms",
            "boot.action[].ops[].reg",
            "boot.action[].ops[].bytes[]",
        ],
        rule: "pins declared (gpio: boot = true, allowed level, the hold level on the power hold pin); a blinking halt period >= 10 ms on the boot LED of an MCU with a stage0; 1..=32 ops; delay 1..=20 ms; bus_write only with the MCU's i2c boot capability, to a writable register, 1..=8 bytes; <= 50 ms per event",
        check: board_boot::boot,
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
        let has_a = |o: &&super::Slot| o.image == s.image && o.binding == Binding::A;
        fail(
            s.binding == Binding::B && !m.slots.iter().any(|o| has_a(&o)),
            || {
                format!(
                    "{what} needs slot {:?}/A: one slot is a, two are a and b",
                    s.image
                )
            },
        )?;
    }
    Ok(())
}

/// `[writers]`: every image kind with a slot states what writes it, every
/// kind named has a slot, and a kind with one slot is written by another
/// kind (an image cannot rewrite the slot it runs from).
fn writers(m: &McuDef) -> Result<(), String> {
    let count = |k: ImageKind| m.slots.iter().filter(|s| s.image == k).count();
    for (kind, by) in &m.writers {
        for k in std::iter::once(kind).chain(by) {
            fail(count(*k) == 0, || {
                format!(
                    "writers.{}: {} has no slot on this MCU",
                    kind.as_str(),
                    k.as_str()
                )
            })?;
        }
    }
    for s in &m.slots {
        let k = s.image;
        let by = m
            .writers
            .get(&k)
            .ok_or_else(|| format!("writers has no entry for {}", k.as_str()))?;
        fail(count(k) == 1 && by.iter().all(|w| *w == k), || {
            format!(
                concat!(
                    "{0} has one slot, and no other image on this MCU can write it: ",
                    "an image cannot rewrite the slot it runs from (writers.{0} = {1:?})"
                ),
                k.as_str(),
                by.iter().map(|w| w.as_str()).collect::<Vec<_>>()
            )
        })?;
    }
    Ok(())
}

/// `[boot_receive]`: the boot stage that writes the app says how it
/// receives, within the window's range, no kind that must enumerate
/// first taking part in the window.
fn boot_receive(m: &McuDef) -> Result<(), String> {
    let Some(r) = &m.boot_receive else {
        return Ok(());
    };
    let max = super::BootReceive::MAX_WINDOW_MS;
    fail(r.listen_window_ms > max, || {
        format!(
            "boot_receive.listen_window_ms = {} is longer than {max} ms",
            r.listen_window_ms
        )
    })?;
    for (kind, &takes_part) in &r.kinds {
        fail(takes_part && kind.enumerated(), || {
            format!(
                "boot_receive.kinds: {} needs enumeration before it carries a frame, so it cannot take part in the listen window",
                kind.name()
            )
        })?;
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
    // A `bootcore_log` equal to `app_log` is one log holding both targets'
    // records: its faces are one region each.
    let core_log = m
        .layout
        .bootcore_log
        .filter(|l| Some(*l) != m.layout.app_log);
    for (name, log) in [("bootcore_log", core_log), ("app_log", m.layout.app_log)] {
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

/// `[stage0]` describes the stage0 `[layout]` places, and names a key
/// place a DAGON stage0 can read.
fn stage0(m: &McuDef) -> Result<(), String> {
    let s = match (&m.stage0, m.layout.stage0) {
        (None, None) => return Ok(()),
        (Some(s), Some(r)) => {
            fail(s.region_bytes != r.size, || {
                format!(
                    "stage0.region_bytes {:#x} is not the {:#x} bytes of layout.stage0",
                    s.region_bytes, r.size
                )
            })?;
            fail(s.head_bytes >= s.region_bytes, || {
                format!(
                    "stage0.head_bytes {:#x} leaves stage0 no room in its {:#x} bytes",
                    s.head_bytes, s.region_bytes
                )
            })?;
            s
        }
        (None, Some(_)) => return Err("layout.stage0 needs a [stage0] table".into()),
        (Some(_), None) => return Err("[stage0] needs layout.stage0, its region".into()),
    };
    fail(!super::KEY_PLACES.contains(&s.key.as_str()), || {
        format!(
            "stage0.key {:?}: this MCU's stage0 cannot read a key from that place (it reads one              from: {})",
            s.key,
            super::KEY_PLACES.join(", ")
        )
    })
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

/// A board's `[default_link] baud` is a speed its MCU's `[link_uart] bauds` names.
pub(crate) fn default_link_baud(b: &BoardDef, m: &McuDef) -> Result<(), String> {
    let (Some(l), Some(u)) = (b.default_link.as_ref(), m.link_uart.as_ref()) else {
        return Ok(());
    };
    let Some(baud) = l.baud else {
        return Ok(());
    };
    fail(!u.runs(baud), || {
        format!(
            "default_link.baud {baud} is not in the MCU's link_uart.bauds {:?}",
            u.bauds
        )
    })
}

/// A link section's clock: exactly one of `clock_hz` (at least 1) and
/// `clock = "app"` (with `[[app.clock]]`).
fn link_clock(
    m: &McuDef,
    section: &str,
    clock_hz: Option<u32>,
    clock: Option<super::LinkClock>,
) -> Result<(), String> {
    fail(clock_hz.is_some() == clock.is_some(), || {
        format!("{section} takes exactly one of clock_hz (a fixed clock) and clock = \"app\" (the app's core clock)")
    })?;
    fail(
        clock.is_some() && m.clock_setups(super::ClockStage::App).is_none(),
        || format!("{section}.clock = \"app\" needs [[app.clock]]"),
    )?;
    fail(clock_hz == Some(0), || {
        format!("{section}.clock_hz must be at least 1")
    })
}

/// A rate limit's slow side: no rate runs more than 100 % slow.
fn slow_side(section: &str, slow_ppm: u32) -> Result<(), String> {
    fail(slow_ppm > 1_000_000, || {
        format!("{section}.slow_ppm must be at most 1000000 (100 %)")
    })
}

fn link_uart(m: &McuDef) -> Result<(), String> {
    let Some(u) = &m.link_uart else {
        return Ok(());
    };
    link_clock(m, "link_uart", u.clock_hz, u.clock)?;
    fail(u.clocks_per_bit == 0, || {
        "link_uart.clocks_per_bit must be at least 1".into()
    })?;
    fail(u.frac_bits > 24, || {
        "link_uart.frac_bits must be at most 24".into()
    })?;
    fail(u.bauds.is_empty(), || {
        "link_uart.bauds must name at least one bit rate".into()
    })?;
    slow_side("link_uart", u.slow_ppm)?;
    // On the app's clock the rate depends on the node's board: V-26 checks
    // each node's.
    let Some(hz) = u.clock_hz else {
        return Ok(());
    };
    let off = u.bauds.iter().find(|&&b| {
        !u.baud_error(hz, b)
            .is_some_and(|e| u.limit().admits(e, 0.0))
    });
    fail(off.is_some(), || {
        format!(
            "link_uart.bauds: this UART cannot run at {} {}",
            off.unwrap(),
            u.limit().describe()
        )
    })
}

fn link_i2c(m: &McuDef) -> Result<(), String> {
    let Some(l) = &m.link_i2c else {
        return Ok(());
    };
    fail(l.hzs.is_empty(), || {
        "link_i2c.hzs must name at least one bus speed".into()
    })?;
    let Some(c) = &l.controller else {
        return Ok(());
    };
    link_clock(m, "link_i2c.controller", c.clock_hz, c.clock)?;
    fail(c.min_period == 0, || {
        "link_i2c.controller.min_period must be at least 1".into()
    })?;
    slow_side("link_i2c.controller", c.slow_ppm)?;
    // On the app's clock SCL depends on the node's board: V-20 checks each
    // node's.
    let Some(hz) = c.clock_hz else {
        return Ok(());
    };
    let off = l
        .hzs
        .iter()
        .find(|&&s| !c.hz_error(hz, s).is_some_and(|e| c.limit().admits(e, 0.0)));
    fail(off.is_some(), || {
        format!(
            "link_i2c.hzs: this I2C controller cannot run SCL at {} {}",
            off.unwrap(),
            c.limit().describe()
        )
    })
}

fn link_ports(m: &McuDef) -> Result<(), String> {
    for (i, p) in m.ports.iter().enumerate() {
        let well_formed = !p.name.is_empty()
            && p.name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        fail(!well_formed, || {
            format!("port name '{}' must be a-z, 0-9 and '-'", p.name)
        })?;
        let earlier = &m.ports[..i];
        fail(earlier.iter().any(|o| o.name == p.name), || {
            format!("port name '{}' is used twice", p.name)
        })?;
        fail(earlier.iter().any(|o| o.index == p.index), || {
            format!("port index {} is used twice", p.index)
        })?;
    }
    Ok(())
}

fn relay_links(m: &McuDef) -> Result<(), String> {
    use super::AddressingSpec;
    for (i, r) in m.relay_links.iter().enumerate() {
        let earlier = &m.relay_links[..i];
        fail(!(1..=254).contains(&r.id), || {
            format!("relay_link id {} is outside 1-254", r.id)
        })?;
        fail(earlier.iter().any(|o| o.id == r.id), || {
            format!("relay_link id {} is used twice", r.id)
        })?;
        fail(earlier.iter().any(|o| o.port == r.port), || {
            format!("port '{}' has two relay_link entries", r.port)
        })?;
        let Some(port) = m.port(&r.port) else {
            return Err(format!(
                "relay_link port '{}' is not a [[link_port]] of this MCU",
                r.port
            ));
        };
        let (kind, at) = (port.kind, &r.port);
        match r.addressing {
            AddressingSpec::SevenBit => fail(!kind.addressed(), || {
                format!(
                    "relay_link on '{at}': seven-bit addressing needs a kind with addresses, not {}",
                    kind.name()
                )
            })?,
            AddressingSpec::Fixed(_) => fail(kind.addressed(), || {
                format!(
                    "relay_link on '{at}': a fixed address is for a kind without addresses, not {}",
                    kind.name()
                )
            })?,
        }
        let limits = dagon_port::LinkKind::from_name(kind.name())
            .and_then(dagon_port::timeout::link_limits)
            .ok_or_else(|| {
                format!(
                    "relay_link on '{at}': DAGON relays no frames over {}",
                    kind.name()
                )
            })?;
        fail(
            r.max_payload == 0 || usize::from(r.max_payload) > limits.max_payload,
            || {
                format!(
                    "relay_link on '{at}': max_payload {} is outside 1-{} for {}",
                    r.max_payload,
                    limits.max_payload,
                    kind.name()
                )
            },
        )?;
    }
    Ok(())
}

fn relay_firmware(m: &McuDef) -> Result<(), String> {
    let Some(r) = m.relay else {
        return Ok(());
    };
    fail(m.relay_links.is_empty(), || {
        "[relay] is for an MCU that relays: it has no [[relay_link]]".into()
    })?;
    let ram = m.ram.map_or(u32::MAX, |r| r.size);
    fail(r.ram_bytes == 0 || r.ram_bytes > ram, || {
        format!("relay.ram_bytes {} is outside 1-{ram}", r.ram_bytes)
    })
}

fn codec_budgets(m: &McuDef) -> Result<(), String> {
    let Some(c) = m.codec else {
        return Ok(());
    };
    let boots = m
        .slot_layouts
        .iter()
        .any(|s| s.target == slot_layout::TargetName::Bootcore);
    fail(c.bootcore.is_some() && !boots, || {
        "[codec.bootcore] is for an MCU with a boot core: it has no [[slot_layout]] of target bootcore"
            .into()
    })?;
    fail(c.relay.is_some() && m.relay_links.is_empty(), || {
        "[codec.relay] is for an MCU that relays: it has no [[relay_link]]".into()
    })?;
    let ram = m.ram.map_or(u32::MAX, |r| r.size);
    for (image, b) in [("bootcore", c.bootcore), ("relay", c.relay)] {
        let Some(b) = b else { continue };
        fail(b.stack_bytes > ram || b.ram_bytes > ram, || {
            format!("codec.{image}: stack_bytes and ram_bytes are at most the RAM, {ram}")
        })?;
    }
    Ok(())
}

fn hub_pins(b: &BoardDef, m: &McuDef) -> Result<(), String> {
    let Some(h) = b.coordinator_pins(m) else {
        return Ok(());
    };
    let pins = [h.i2c.sda, h.i2c.scl, h.pio_uart.tx, h.pio_uart.rx];
    match pins.iter().find(|&&p| p >= m.gpio_count) {
        Some(p) => Err(format!(
            "coordinator pin GPIO{p} is not below gpio_count {}",
            m.gpio_count
        )),
        None => Ok(()),
    }
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
    })?;
    for (i, p) in r.pairs.iter().enumerate() {
        if let Some(q) = r.pairs[..i]
            .iter()
            .find(|q| q.sda == p.sda && q.scl == p.scl)
        {
            return Err(format!(
                "rom_i2c.pairs: {}/{} is listed for {} and {}; a pin pair names one controller",
                p.sda, p.scl, q.controller, p.controller
            ));
        }
    }
    Ok(())
}
