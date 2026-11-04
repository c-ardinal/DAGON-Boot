//! The Arm PMSAv6 / PMSAv7 MPU rules the `armv6m-mpu` and `armv7m-mpu`
//! kinds share: power-of-two regions aligned to their size, at least the
//! definition's `min_region`, eight subregions from `subregions_from`,
//! `PRIVDEFENA` giving DAGON the background map. The two kinds differ in
//! the numbers their capability records allow and in fault reporting.

use dagon_topology::defs::isolation::{Cache, Kind};
use dagon_topology::defs::{McuDef, Peripheral};

use super::{
    Access, ExtLayout, ExtMap, Map, Memory, Purpose, ReadBack, Refusal, Region, Space, Violation,
};

/// Regions every extension has: code, data, stack.
const FIXED: usize = 3;
const SUBREGIONS: u32 = 8;
/// Smallest region whose subregions the architecture honours.
const SUBREGION_MIN: u32 = 256;
/// `MPU_TYPE` (ARMv6-M and ARMv7-M system control space).
const MPU_TYPE: u32 = 0xE000_ED90;

struct Params {
    regions: usize,
    min: u32,
    subregions_from: u32,
}

fn params(def: &McuDef) -> Params {
    let c = &def.isolation().capabilities;
    Params {
        regions: c.mpu_regions.unwrap_or(0) as usize,
        min: c.min_region.unwrap_or(SUBREGION_MIN),
        subregions_from: c.subregions_from.unwrap_or(0),
    }
}

impl Params {
    fn subregions(&self, size: u32) -> bool {
        self.subregions_from != 0 && size >= self.subregions_from.max(SUBREGION_MIN)
    }
}

fn pow2(bytes: u32, min: u32) -> Result<u32, Refusal> {
    bytes
        .max(min)
        .checked_next_power_of_two()
        .ok_or_else(|| Refusal(format!("{bytes} bytes is past the largest region")))
}

/// The stack region and its guard: the lowest subregion disabled, so a
/// stack overflow faults before it leaves the region.
fn stack(bytes: u32, p: &Params) -> Result<(u32, u8), Refusal> {
    let mut size = pow2(bytes, p.min)?;
    if !p.subregions(size) {
        if p.subregions_from == 0 {
            return Ok((size, 0));
        }
        size = pow2(size, p.subregions_from.max(SUBREGION_MIN))?;
    }
    if size - size / SUBREGIONS < bytes {
        size = size
            .checked_mul(2)
            .ok_or_else(|| Refusal(format!("a {bytes}-byte stack is past the largest region")))?;
    }
    Ok((size, 1))
}

fn overlaps(a: (u64, u64), b: (u64, u64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn span(p: &Peripheral) -> (u64, u64) {
    (u64::from(p.base), p.end())
}

/// The smallest aligned region holding `p`, with the subregions that hold
/// none of it disabled; refused when it would still open another block
/// the extension did not declare.
fn window(def: &McuDef, p: &Peripheral, mine: &[String], prm: &Params) -> Result<Region, Refusal> {
    let mut size = pow2(p.size, prm.min)?;
    let mut base;
    loop {
        base = p.base & !(size - 1);
        if u64::from(base) + u64::from(size) >= p.end() {
            break;
        }
        size = size
            .checked_mul(2)
            .ok_or_else(|| Refusal(format!("no region holds peripheral {}", p.id)))?;
    }
    let mut disabled = 0u8;
    if prm.subregions(size) {
        let sub = u64::from(size / SUBREGIONS);
        for i in 0..SUBREGIONS {
            let at = u64::from(base) + u64::from(i) * sub;
            if !overlaps((at, at + sub), span(p)) {
                disabled |= 1 << i;
            }
        }
    }
    let r = Region {
        number: 0,
        base,
        size,
        disabled,
        access: Access::ReadWrite,
        execute: false,
        memory: Memory::Device,
        purpose: Purpose::Window(p.id.clone()),
    };
    let ranges = r.enabled();
    if let Some(q) = def.peripherals.iter().find(|q| {
        q.id != p.id && !mine.contains(&q.id) && ranges.iter().any(|&e| overlaps(e, span(q)))
    }) {
        return Err(Refusal(format!(
            "a window for {} would also open {} on {} (regions are aligned blocks of at least {} bytes)",
            p.id, q.id, def.id, prm.min
        )));
    }
    Ok(r)
}

/// Bases for `sizes`, placed downward from `top` (largest first, each
/// aligned to its size), and the lowest address used; refused below
/// `bottom`.
fn place(top: u32, bottom: u32, sizes: &[u32], what: &str) -> Result<(Vec<u32>, u32), Refusal> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let mut bases = vec![0; sizes.len()];
    let mut cursor = u64::from(top);
    for i in order {
        let size = u64::from(sizes[i]);
        let base = cursor
            .checked_sub(size)
            .map(|b| b & !(size - 1))
            // The image keeps at least one byte below the regions.
            .filter(|b| *b > u64::from(bottom))
            .ok_or_else(|| {
                let need: u64 = sizes.iter().map(|s| u64::from(*s)).sum();
                Refusal(format!(
                    "extensions need {need} bytes of {what} in aligned regions; {} bytes are free",
                    top - bottom
                ))
            })?;
        bases[i] = base as u32;
        cursor = base;
    }
    Ok((bases, cursor as u32))
}

/// What this generator does about caches: capability windows are device
/// memory (never cached) and code, data and stacks normal memory. It
/// places no DMA bounce buffer and nothing in tightly coupled memory, so a
/// definition that needs either is refused here, by field.
fn caches(def: &McuDef) -> Result<(), Refusal> {
    let i = def.isolation();
    if i.cache == Cache::IdCache {
        return Err(Refusal(format!(
            "isolation.cache = \"id-cache\" on {}: this generator places no non-cacheable DMA bounce buffer for a data cache",
            def.id
        )));
    }
    if !i.tcm.is_empty() {
        return Err(Refusal(format!(
            "isolation.tcm on {}: this generator places no extension region or bounce buffer in tightly coupled memory",
            def.id
        )));
    }
    Ok(())
}

/// `plan` for the PMSA kinds.
pub(super) fn plan(
    kind: Kind,
    def: &McuDef,
    space: &Space,
    exts: &[ExtLayout],
) -> Result<Map, Refusal> {
    let prm = params(def);
    let name = kind.as_str();
    caches(def)?;
    let mut maps = Vec::new();
    let (mut texts, mut rams) = (Vec::new(), Vec::new());
    for e in exts {
        let fit = prm.regions.saturating_sub(FIXED);
        if e.windows.len() > fit {
            return Err(Refusal(format!(
                "extension {} needs {} windows; {fit} fit after code, data and stack on an {}-region {name}",
                e.id,
                e.windows.len(),
                prm.regions
            )));
        }
        let mut regions = Vec::new();
        for (n, w) in e.windows.iter().enumerate() {
            let p = def.peripheral(w).ok_or_else(|| {
                Refusal(format!(
                    "extension {}: {} has no peripheral {w}",
                    e.id, def.id
                ))
            })?;
            if !p.window_eligible {
                return Err(Refusal(format!(
                    "extension {}: peripheral {w} of {} is not window-eligible (privileged: {})",
                    e.id,
                    def.id,
                    p.privileged_why
                        .iter()
                        .map(|w| w.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            let mut r = window(def, p, &e.windows, &prm)?;
            r.number = (FIXED + n) as u8;
            regions.push(r);
        }
        let (stack_size, guard) = stack(e.stack_bytes, &prm)?;
        texts.push(pow2(e.text_bytes, prm.min)?);
        rams.push(pow2(e.ram_bytes, prm.min)?);
        rams.push(stack_size);
        maps.push((e.id.clone(), guard, regions));
    }
    let ram_need: u64 = rams.iter().map(|s| u64::from(*s)).sum();
    let ram_max = def.isolation().ext_ram_max.unwrap_or(0);
    if ram_need > u64::from(ram_max) {
        return Err(Refusal(format!(
            "extensions need {ram_need} bytes of RAM in aligned regions; ext_ram_max of {} is {ram_max}",
            def.id
        )));
    }
    let (text_at, flash_low) = place(
        space.flash_at + space.flash_size,
        space.flash_at,
        &texts,
        "flash",
    )?;
    let (ram_at, ram_low) = place(space.ram_at + space.ram_size, space.ram_at, &rams, "RAM")?;
    let exts = maps
        .into_iter()
        .enumerate()
        .map(|(i, (id, guard, windows))| {
            let fixed = |number, base, size, disabled, access, execute, purpose| Region {
                number,
                base,
                size,
                disabled,
                access,
                execute,
                memory: Memory::Normal,
                purpose,
            };
            let mut regions = vec![
                fixed(
                    0,
                    text_at[i],
                    texts[i],
                    0,
                    Access::ReadOnly,
                    true,
                    Purpose::Text,
                ),
                fixed(
                    1,
                    ram_at[2 * i],
                    rams[2 * i],
                    0,
                    Access::ReadWrite,
                    false,
                    Purpose::Data,
                ),
                fixed(
                    2,
                    ram_at[2 * i + 1],
                    rams[2 * i + 1],
                    guard,
                    Access::ReadWrite,
                    false,
                    Purpose::Stack,
                ),
            ];
            regions.extend(windows);
            ExtMap { id, regions }
        })
        .collect();
    Ok(Map {
        kind,
        exts,
        image_flash: (space.flash_at, flash_low - space.flash_at),
        image_ram: (space.ram_at, ram_low - space.ram_at),
    })
}

fn inside(r: (u64, u64), outer: (u64, u64)) -> bool {
    r.0 >= outer.0 && r.1 <= outer.1
}

/// Where code and data of an image may be: the slots' bodies, the RAM.
/// An address range, end exclusive.
type Span = (u64, u64);

fn memories(def: &McuDef) -> (Vec<Span>, Option<Span>) {
    let bodies = def
        .slots
        .iter()
        .map(|s| {
            (
                u64::from(s.load_addr),
                u64::from(s.load_addr) + u64::from(s.max_payload),
            )
        })
        .collect();
    let ram = def
        .ram
        .map(|r| (u64::from(r.at), u64::from(r.at) + u64::from(r.size)));
    (bodies, ram)
}

/// What DAGON keeps in RAM beside the image: the boot record and the fuse.
fn kept(def: &McuDef) -> Vec<(String, (u64, u64))> {
    let Some(r) = def.record() else {
        return Vec::new();
    };
    let range = |at: u32, size: u32| (u64::from(at), u64::from(at) + u64::from(size));
    std::iter::once(("the boot record".to_string(), range(r.at, r.size)))
        .chain(
            r.fuse
                .cells
                .iter()
                .map(|c| (format!("the {:?} fuse cell", c.image), range(c.at, c.size))),
        )
        .collect()
}

fn check_region(def: &McuDef, prm: &Params, e: &ExtMap, r: &Region, out: &mut Vec<Violation>) {
    let mut v = |msg: String| out.push(Violation(format!("{} region {}: {msg}", e.id, r.number)));
    if !r.size.is_power_of_two() || r.size < prm.min {
        v(format!(
            "size {:#x} is not a power of two of at least {}",
            r.size, prm.min
        ));
    }
    if r.size.is_power_of_two() && r.base & (r.size - 1) != 0 {
        v(format!(
            "base {:#x} is not aligned to its size {:#x}",
            r.base, r.size
        ));
    }
    if r.disabled != 0 && !prm.subregions(r.size) {
        v(format!(
            "subregions disabled on a {:#x}-byte region",
            r.size
        ));
    }
    if r.disabled == 0xFF {
        v("every subregion disabled".into());
    }
    let (access, execute) = match r.purpose {
        Purpose::Text => (Access::ReadOnly, true),
        _ => (Access::ReadWrite, false),
    };
    if r.memory != r.purpose.memory() {
        v(format!(
            "{} must be {} memory",
            r.purpose,
            r.purpose.memory().as_str()
        ));
    }
    if (r.access, r.execute) != (access, execute) {
        v(format!(
            "{} must be {} with execute {execute}",
            r.purpose,
            access.as_str()
        ));
    }
    let ranges = r.enabled();
    // Privileged blocks stay outside every allowed range; a window opens
    // only declared window-eligible blocks.
    let windows: Vec<&str> = e
        .regions
        .iter()
        .filter_map(|x| match &x.purpose {
            Purpose::Window(p) => Some(p.as_str()),
            _ => None,
        })
        .collect();
    for p in &def.peripherals {
        if !ranges.iter().any(|&x| overlaps(x, span(p))) {
            continue;
        }
        if !p.window_eligible || !windows.contains(&p.id.as_str()) {
            v(format!("opens peripheral {}", p.id));
        }
    }
    let (bodies, ram) = memories(def);
    let whole = (u64::from(r.base), r.end());
    match &r.purpose {
        Purpose::Text => {
            if !bodies.iter().any(|b| inside(whole, *b)) {
                v("code outside every image body".into());
            }
        }
        Purpose::Data | Purpose::Stack => {
            if !ram.is_some_and(|m| inside(whole, m)) {
                v("outside the MCU's RAM".into());
            }
            for (what, k) in kept(def) {
                if overlaps(whole, k) {
                    v(format!("overlaps {what}"));
                }
            }
        }
        Purpose::Window(p) => {
            match def.peripheral(p) {
                Some(q) if ranges.iter().any(|&x| inside(span(q), x)) || inside(span(q), whole) => {
                }
                _ => v(format!("does not hold peripheral {p}")),
            }
            // A window never reaches memory: RAM, flash, the image slots,
            // the boot record or the fuse cells.
            let flash = def.flash_maps.iter().map(|f| {
                (
                    "flash",
                    (u64::from(f.base), u64::from(f.base) + u64::from(f.bytes)),
                )
            });
            let mems = ram
                .map(|m| ("RAM", m))
                .into_iter()
                .chain(flash)
                .chain(bodies.iter().map(|b| ("an image slot", *b)));
            for (what, m) in mems {
                if ranges.iter().any(|&x| overlaps(x, m)) {
                    v(format!("window reaches {what}"));
                }
            }
            for (what, k) in kept(def) {
                if ranges.iter().any(|&x| overlaps(x, k)) {
                    v(format!("window reaches {what}"));
                }
            }
        }
    }
}

/// `check` for the PMSA kinds.
pub(super) fn check(def: &McuDef, map: &Map) -> Vec<Violation> {
    let prm = params(def);
    let mut out = Vec::new();
    let image = [
        (
            u64::from(map.image_flash.0),
            u64::from(map.image_flash.0) + u64::from(map.image_flash.1),
        ),
        (
            u64::from(map.image_ram.0),
            u64::from(map.image_ram.0) + u64::from(map.image_ram.1),
        ),
    ];
    let mut owned: Vec<(String, (u64, u64))> = Vec::new();
    for e in &map.exts {
        if e.regions.len() > prm.regions {
            out.push(Violation(format!(
                "{}: {} regions on a unit of {}",
                e.id,
                e.regions.len(),
                prm.regions
            )));
        }
        for p in [Purpose::Text, Purpose::Data, Purpose::Stack] {
            if e.regions.iter().filter(|r| r.purpose == p).count() != 1 {
                out.push(Violation(format!("{}: needs exactly one {p} region", e.id)));
            }
        }
        for (i, r) in e.regions.iter().enumerate() {
            if usize::from(r.number) >= prm.regions
                || e.regions[..i].iter().any(|o| o.number == r.number)
            {
                out.push(Violation(format!(
                    "{}: region number {} is out of range or repeated",
                    e.id, r.number
                )));
            }
            check_region(def, &prm, e, r, &mut out);
            if matches!(r.purpose, Purpose::Window(_)) {
                continue;
            }
            let whole = (u64::from(r.base), r.end());
            if image.iter().any(|m| overlaps(whole, *m)) {
                out.push(Violation(format!(
                    "{} {}: overlaps the image's own memory",
                    e.id, r.purpose
                )));
            }
            if let Some((who, _)) = owned.iter().find(|(_, o)| overlaps(whole, *o)) {
                out.push(Violation(format!(
                    "{} {}: overlaps {who}'s region",
                    e.id, r.purpose
                )));
            }
            owned.push((e.id.clone(), whole));
        }
    }
    out
}

/// `MPU_TYPE.DREGION` (bits 15:8) holds the region count.
pub(super) fn read_back(def: &McuDef) -> Vec<ReadBack> {
    vec![ReadBack {
        register: "MPU_TYPE.DREGION",
        addr: MPU_TYPE,
        mask: 0xFF,
        shift: 8,
        expect: params(def).regions as u32,
    }]
}
