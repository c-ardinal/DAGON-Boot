//! SimStore: the agent's receiver over a SimFlash with a kind S layout
//! (inline headers, one state log face), plus the boot decision made at reset.
//! The log is folded and mapped to STATUS by dagon-core
//! (`LogState::apply`, `LogState::status`), as every kind S store does, and
//! the boot decision writes the records the real boot core writes.
//! A clone is another handle on the same flash: the agent holds one, the
//! simulated board (its boot core, ROM loader, power) another.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::flash::SimFlash;
use dagon_agent::{SlotStore, StoreError, Transition};
use dagon_core::bootcore::{choose, read_state, settle_log, MustReset, Settled, CLEARED};
use dagon_core::log::{Log, LogLayout, LogState, SlotRef};
use dagon_format::header::{
    self, build_flags, capabilities, image_kind, placement, port_kind, slot_binding, ImageHeader,
    HEADER_SIZE,
};
use dagon_format::statelog::{self, kind, reason, FaceHeader, Record, RECORD_SIZE};
use dagon_format::{sha256, Version};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::{target, SlotInfo, SlotState, StatusResp};

/// Where things live in a node's flash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The MCU definition this layout models (its boot attempt limit).
    pub mcu: &'static str,
    /// Boot core region; all-erased means the chip has nothing to run.
    pub bootcore: (u32, u32),
    /// State log face (one erase unit).
    pub log: (u32, u32),
    /// APP slots A and B: header inline at the slot start, body at +512.
    pub slots: [(u32, u32); 2],
    /// Hub display copy of the topology (body only), if this board keeps one.
    pub topology: Option<(u32, u32)>,
    /// Factory (spare) slot, reported as STATUS slot 2: the recovery
    /// agent's slot (transfer target AGENT, header inline in this model),
    /// first written by [`SimStore::provision_factory`]; never a boot
    /// target here.
    pub factory: Option<(u32, u32)>,
    /// Boot core slots A/B behind a stage0, as the MCU definition's
    /// `bootcore` slots declare them; `None`: the boot core is the
    /// `bootcore` region alone.
    pub cores: Option<Cores>,
}

/// Boot core slots (header inline at the slot start) and their log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cores {
    pub slots: [(u32, u32); 2],
    pub log: LogLayout,
}

impl Layout {
    /// STM32F401xE: stage0 sector 0, the state log (APP and boot core
    /// records in one face pair) sectors 1-2, boot core A sector 4, APP-A
    /// sector 5, APP-B sector 6, boot core B sector 7.
    pub const STM32F401XE: Layout = Layout {
        mcu: "stm32f401re",
        bootcore: (0x0800_0000, 0x4000),
        log: (0x0800_4000, 0x4000),
        slots: [(0x0802_0000, 0x2_0000), (0x0804_0000, 0x2_0000)],
        topology: None,
        factory: None,
        cores: Some(Cores {
            slots: [(0x0801_0000, 0x1_0000), (0x0806_0000, 0x2_0000)],
            log: LogLayout {
                faces: [0x0800_4000, 0x0800_8000],
                face_len: 0x4000,
            },
        }),
    };

    /// Hub on RP2040 with 2 MiB flash: stage0 (the `bootcore` region),
    /// boot core slots A/B with their log.
    pub const RP2040_HUB: Layout = Layout {
        mcu: "rp2040",
        bootcore: (0x1000_0000, 0x1_0000),
        log: (0x1001_0000, 0x1000),
        slots: [(0x1002_0000, 0x8_0000), (0x100A_0000, 0x8_0000)],
        topology: Some((0x1012_0000, 0x1_0000)),
        factory: None,
        cores: Some(Cores {
            slots: [(0x1013_0000, 0x1_0000), (0x1014_0000, 0x1_0000)],
            log: LogLayout {
                faces: [0x1015_0000, 0x1015_1000],
                face_len: 0x1000,
            },
        }),
    };

    /// ESP32-S3 watch (4 MiB modelled): loader, selection data, two OTA
    /// slots and a factory slot. Simplified to kind S semantics.
    pub const ESP32S3_WATCH: Layout = Layout {
        mcu: "esp32s3",
        bootcore: (0x0000_0000, 0x1_0000),
        log: (0x0001_0000, 0x1000),
        slots: [(0x0011_0000, 0x10_0000), (0x0021_0000, 0x10_0000)],
        topology: None,
        factory: Some((0x0031_0000, 0x0F_0000)),
        cores: None,
    };
}

impl Layout {
    /// Boots the modelled boot stage gives a pending image: the MCU
    /// definition's `[watchdog] trial_attempts`.
    pub fn trial_attempts(&self) -> u8 {
        trial_attempts(self.mcu)
    }
}

/// `[watchdog] trial_attempts` of MCU definition `mcu`.
pub fn trial_attempts(mcu: &str) -> u8 {
    dagon_topology::defs::Registry::builtin()
        .mcu(mcu)
        .and_then(|m| m.watchdog)
        .map(|w| w.trial_attempts)
        .unwrap_or_else(|| panic!("MCU definition {mcu} has no [watchdog] trial_attempts"))
}

/// The flash as a driver that programs any length at any address shows it
/// (the RP2040's does, by pages): writes are padded to whole write units,
/// the bytes around them rewritten with their current value.
struct Paged<'a>(MutexGuard<'a, SimFlash>);

impl Flash for Paged<'_> {
    fn geometry(&self) -> &[FlashRegion] {
        self.0.geometry()
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.0.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.0.erase(addr, len)
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        if data.is_empty() {
            return Ok(());
        }
        let ws = self.0.unit_at(addr).ok_or(FlashError::Range)?.write_size as usize;
        let start = addr as usize / ws * ws;
        let mut buf = vec![0xFF; (addr as usize + data.len()).div_ceil(ws) * ws - start];
        self.0.read(start as u32, &mut buf)?;
        buf[addr as usize - start..][..data.len()].copy_from_slice(data);
        self.0.write(start as u32, &buf)
    }
}

/// What the boot core decided at reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boot {
    /// Runs this slot (trying or confirmed).
    Run(u8),
    /// Nothing valid to run: the chip stays silent.
    Nothing,
}

#[derive(Clone)]
pub struct SimStore {
    flash: Arc<Mutex<SimFlash>>,
    pub layout: Layout,
    pubkey: [u8; 32],
    /// The boot core trial cell (retained memory: kept through resets,
    /// cleared by a power loss, [`SimStore::power_lost`]).
    cell: Arc<Mutex<[u32; 2]>>,
    /// A boot core of this version fails its own check (cannot start the
    /// app), as a broken new core does.
    pub broken_core: Option<Version>,
}

impl Default for SimStore {
    fn default() -> Self {
        SimStore::new(SimFlash::new(&[], 0), Layout::STM32F401XE, [0; 32])
    }
}

impl SimStore {
    pub fn new(flash: SimFlash, layout: Layout, pubkey: [u8; 32]) -> Self {
        SimStore {
            flash: Arc::new(Mutex::new(flash)),
            layout,
            pubkey,
            cell: Arc::new(Mutex::new(CLEARED)),
            broken_core: None,
        }
    }

    /// Power was lost: retained memory (the trial cell) is gone.
    pub fn power_lost(&mut self) {
        *self.cell.lock().unwrap_or_else(PoisonError::into_inner) = CLEARED;
    }

    fn cell(&self) -> MutexGuard<'_, [u32; 2]> {
        self.cell.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A store over a copy of this one's flash (a second, independent
    /// device in the same state).
    pub fn snapshot(&self) -> SimStore {
        SimStore::new(self.flash().clone(), self.layout, self.pubkey)
    }

    /// The flash, shared with every clone of this store.
    pub fn flash(&self) -> MutexGuard<'_, SimFlash> {
        self.flash.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn region(&self, t: u8, slot: u8) -> Result<(u32, u32), StoreError> {
        match t {
            target::APP if slot < 2 => {
                let (a, l) = self.layout.slots[slot as usize];
                Ok((a + HEADER_SIZE as u32, l - HEADER_SIZE as u32))
            }
            target::BOOTCORE if slot < 2 => {
                let c = self.layout.cores.ok_or(StoreError::Unsupported)?;
                let (a, l) = c.slots[slot as usize];
                Ok((a + HEADER_SIZE as u32, l - HEADER_SIZE as u32))
            }
            target::AGENT if slot == 2 => {
                let (a, l) = self.layout.factory.ok_or(StoreError::Unsupported)?;
                Ok((a + HEADER_SIZE as u32, l - HEADER_SIZE as u32))
            }
            target::TOPOLOGY if slot == 0 => self.layout.topology.ok_or(StoreError::Unsupported),
            _ => Err(StoreError::Unsupported),
        }
    }

    /// Slot 0/1 = A/B, 2 = factory.
    fn slot_base(&self, slot: u8) -> Option<u32> {
        match slot {
            0 | 1 => Some(self.layout.slots[slot as usize].0),
            2 => self.layout.factory.map(|f| f.0),
            _ => None,
        }
    }

    fn header_at(&mut self, slot: u8) -> Option<[u8; HEADER_SIZE]> {
        let mut h = [0u8; HEADER_SIZE];
        self.flash().read(self.slot_base(slot)?, &mut h).ok()?;
        ImageHeader::parse(&h).ok().map(|_| h)
    }

    /// Face generation (0 = no valid face), record places used (torn ones
    /// included) and the valid records.
    fn records(&mut self) -> (u32, usize, Vec<Record>) {
        let (base, len) = self.layout.log;
        let mut buf = vec![0u8; len as usize];
        if self.flash().read(base, &mut buf).is_err() {
            return (0, 0, Vec::new());
        }
        let face: [u8; 32] = buf[..32].try_into().expect("32");
        let Ok(fh) = FaceHeader::parse(&face) else {
            return (0, 0, Vec::new());
        };
        let used: Vec<&[u8; RECORD_SIZE]> = buf[statelog::FACE_HEADER_SIZE..]
            .chunks_exact(RECORD_SIZE)
            .map(|c| <&[u8; RECORD_SIZE]>::try_from(c).expect("32"))
            .take_while(|c| !Record::is_erased(c))
            .collect();
        let recs = used.iter().filter_map(|c| Record::parse(c).ok()).collect();
        (fh.generation, used.len(), recs)
    }

    /// Appends one record (writing the face header first on a blank face).
    pub fn append(&mut self, rec: Record) -> Result<(), StoreError> {
        let (base, len) = self.layout.log;
        let (gen, used, recs) = self.records();
        if gen == 0 {
            self.flash()
                .erase(base, len)
                .map_err(|_| StoreError::Flash)?;
            let fh = FaceHeader { generation: 1 }.to_bytes();
            self.program(base, &fh)?;
        }
        let at = base as usize + statelog::FACE_HEADER_SIZE + used * RECORD_SIZE;
        // One face, no compaction: a full face fails the append.
        if at + RECORD_SIZE > (base + len) as usize {
            return Err(StoreError::Flash);
        }
        let seq = recs.last().map_or(1, |r| r.seq + 1);
        self.program(at as u32, &Record { seq, ..rec }.to_bytes())
    }

    /// Writes bytes at any alignment into erased flash (pads the last unit).
    fn program(&mut self, addr: u32, data: &[u8]) -> Result<(), StoreError> {
        Paged(self.flash()).write(addr, data).map_err(|e| match e {
            FlashError::Range => StoreError::Range,
            _ => StoreError::Flash,
        })
    }

    fn log_record(k: u8, slot: u8, why: u8, hdr: &[u8; HEADER_SIZE]) -> Record {
        let version = ImageHeader::parse(hdr).map_or(Version::default(), |h| h.version);
        Record {
            kind: k,
            target: statelog::target::APP,
            slot,
            reason: why,
            seq: 0,
            version,
            digest16: header::header_digest16(hdr),
        }
    }

    /// Writes a factory image into `slot` and records it as provisioned and
    /// confirmed; also marks the boot core region as present.
    pub fn provision(&mut self, slot: u8, image: &[u8]) -> Result<(), StoreError> {
        let (b, _) = self.layout.bootcore;
        let mut marker = [0u8; 4];
        self.flash()
            .read(b, &mut marker)
            .map_err(|_| StoreError::Flash)?;
        if marker == [0xFF; 4] {
            self.program(b, b"DGBC")?;
        }
        self.provision_core()?;
        let hdr: [u8; HEADER_SIZE] = image
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(StoreError::Range)?;
        let body = &image[HEADER_SIZE..];
        self.erase_body(target::APP, slot, body.len() as u32)?;
        self.write_body(target::APP, slot, 0, body)?;
        self.place_header(slot, &hdr)?;
        self.append(Self::log_record(kind::PROVISION, slot, reason::NONE, &hdr))?;
        self.append(Self::log_record(kind::CONFIRMED, slot, reason::NONE, &hdr))
    }

    /// Erases every unit touching `start..end`.
    fn erase_range(&mut self, start: u32, end: u32) -> Result<(), StoreError> {
        let mut a = start;
        while a < end {
            let unit = self.flash().unit_at(a).ok_or(StoreError::Range)?;
            let unit_start = a - (a - unit.start) % unit.erase_size;
            self.flash()
                .erase(unit_start, unit.erase_size)
                .map_err(|_| StoreError::Flash)?;
            a = unit_start + unit.erase_size;
        }
        Ok(())
    }

    /// Writes an image (header inline, then body) into the factory slot.
    pub fn provision_factory(&mut self, image: &[u8]) -> Result<(), StoreError> {
        let (base, len) = self.layout.factory.ok_or(StoreError::Unsupported)?;
        if image.len() < HEADER_SIZE || image.len() > len as usize {
            return Err(StoreError::Range);
        }
        self.erase_range(base, base + image.len() as u32)?;
        self.program(base + HEADER_SIZE as u32, &image[HEADER_SIZE..])?;
        self.program(base, &image[..HEADER_SIZE])
    }

    /// The first install's boot core in slot A (version 1.0.0; stage0
    /// checks no signature, so the stand-in is unsigned) and the boot core
    /// log naming it, unless the log already holds one.
    fn provision_core(&mut self) -> Result<(), StoreError> {
        let Some(c) = self.layout.cores else {
            return Ok(());
        };
        if read_state(&mut Paged(self.flash()), &c.log).is_some() {
            return Ok(());
        }
        let body = b"DGBC".repeat(64);
        let h = ImageHeader {
            image_kind: image_kind::BOOTCORE,
            port_kind: port_kind::SELF_BOOT,
            mcu_id: 0,
            slot_binding: slot_binding::A,
            placement: placement::INLINE,
            load_addr: c.slots[0].0 + HEADER_SIZE as u32,
            payload_size: body.len() as u32,
            version: Version::new(1, 0, 0),
            build_flags: build_flags::VERIFY_ENFORCED,
            release_seq: 0,
            min_bootcore_version: Version::default(),
            payload_sha256: sha256(&body),
            key_id: [0; 8],
            target_tag: [0; 16],
            proto_version: 1,
            capabilities: capabilities::SIG_VERIFIER,
            signature: [0; 64],
        }
        .to_bytes();
        self.erase_range(c.slots[0].0, c.slots[0].0 + c.slots[0].1)?;
        self.program(c.slots[0].0 + HEADER_SIZE as u32, &body)?;
        self.program(c.slots[0].0, &h)?;
        for face in c.log.faces {
            self.erase_range(face, face + c.log.face_len)?;
        }
        let fh = FaceHeader { generation: 1 }.to_bytes();
        self.program(c.log.faces[0], &fh)?;
        let at = self.core_head(0).ok_or(StoreError::Flash)?;
        let mut f = Paged(self.flash());
        let mut log = Log::load(&mut f, &c.log).map_err(|_| StoreError::Flash)?;
        for k in [kind::PROVISION, kind::CONFIRMED] {
            log.append_core(&mut f, k, at, reason::NONE)
                .map_err(|_| StoreError::Flash)?;
        }
        Ok(())
    }

    /// The header in boot core slot `slot` as a log reference.
    fn core_head(&mut self, slot: u8) -> Option<SlotRef> {
        let (base, _) = self.layout.cores?.slots[usize::from(slot)];
        let mut h = [0u8; HEADER_SIZE];
        self.flash().read(base, &mut h).ok()?;
        Some(SlotRef {
            slot,
            version: ImageHeader::parse(&h).ok()?.version,
            digest16: header::header_digest16(&h),
        })
    }

    /// Stage0's check of the core a record names (slot A alone for
    /// `None`): its header is the recorded one, a boot core for that slot,
    /// and its body hashes to the header's digest. No signature, as on the
    /// hub.
    fn core_check(&mut self, r: Option<&SlotRef>) -> Result<(), ()> {
        let s = r.map_or(slot_binding::A, |r| r.slot);
        let head = self.core_head(s).ok_or(())?;
        if r.is_some_and(|r| *r != head) {
            return Err(());
        }
        let (base, len) = self.layout.cores.ok_or(())?.slots[usize::from(s)];
        let mut raw = [0u8; HEADER_SIZE];
        self.flash().read(base, &mut raw).map_err(|_| ())?;
        let h = ImageHeader::parse(&raw).map_err(|_| ())?;
        if h.image_kind != image_kind::BOOTCORE
            || h.slot_binding != s
            || h.payload_size > len - HEADER_SIZE as u32
        {
            return Err(());
        }
        let mut body = vec![0u8; h.payload_size as usize];
        self.flash()
            .read(base + HEADER_SIZE as u32, &mut body)
            .map_err(|_| ())?;
        (sha256(&body) == h.payload_sha256).then_some(()).ok_or(())
    }

    /// Stage0 and the boot core over the boot core slots
    /// (`dagon_core::bootcore`), through the resets a core that fails its
    /// own check causes: the slot of the core that starts the app, `None`
    /// when no core can start (or the power went).
    fn boot_core(&mut self, c: Cores) -> Option<u8> {
        let max = self.layout.trial_attempts();
        for _ in 0..=max + 1 {
            let st = read_state(&mut Paged(self.flash()), &c.log).unwrap_or_default();
            let cell = *self.cell();
            let start = choose(&st, cell, max, |r| self.core_check(r)).ok()?;
            *self.cell() = start.cell;
            let healthy = self.core_head(start.slot).map(|h| h.version) != self.broken_core;
            let settled = settle_log(
                &mut Paged(self.flash()),
                &c.log,
                start.slot,
                (start.cell, max),
                healthy,
            );
            if !self.flash().is_powered() {
                return None;
            }
            match settled {
                Ok(Settled::Nothing) => return Some(start.slot),
                Ok(_) => {
                    *self.cell() = CLEARED;
                    return Some(start.slot);
                }
                Err(MustReset) => continue,
            }
        }
        None
    }

    fn bootcore_present(&mut self) -> bool {
        let (b, l) = self.layout.bootcore;
        let mut buf = vec![0u8; l as usize];
        self.flash().read(b, &mut buf).is_ok() && buf.iter().any(|&x| x != 0xFF)
    }

    fn verified(&mut self, slot: u8) -> Option<[u8; HEADER_SIZE]> {
        let hdr = self.header_at(slot)?;
        let h = header::verify_header(&hdr, &self.pubkey).ok()?;
        let mut body = vec![0u8; h.payload_size as usize];
        self.read(target::APP, slot, 0, &mut body).ok()?;
        (sha256(&body) == h.payload_sha256).then_some(hdr)
    }

    /// The log folded as every kind S store folds it, and its generation.
    fn log_state(&mut self) -> (LogState, u32) {
        let (gen, _, recs) = self.records();
        let state = recs.iter().fold(LogState::default(), |s, r| s.apply(r));
        (state, gen)
    }

    /// The header in `slot` as a log reference, if it parses.
    fn head(&mut self, slot: u8) -> Option<SlotRef> {
        let h = self.header_at(slot)?;
        Some(SlotRef {
            slot,
            version: ImageHeader::parse(&h).ok()?.version,
            digest16: header::header_digest16(&h),
        })
    }

    fn append_ref(&mut self, k: u8, at: SlotRef, why: u8) -> Result<(), StoreError> {
        self.append(Record {
            kind: k,
            target: statelog::target::APP,
            slot: at.slot,
            reason: why,
            seq: 0,
            version: at.version,
            digest16: at.digest16,
        })
    }

    /// Boot core decision at reset, as dagon-core's: start the pending
    /// image on trial (its header the one the log names, signature and
    /// body verified, attempts left) after a BOOT_ATTEMPT; otherwise roll
    /// it back with a ROLLBACK naming the confirmed slot the boot returns
    /// to; then start the confirmed slot while it holds its image.
    pub fn boot(&mut self) -> Boot {
        if !self.bootcore_present() {
            return Boot::Nothing;
        }
        if let Some(c) = self.layout.cores {
            if self.boot_core(c).is_none() {
                return Boot::Nothing;
            }
        }
        let (ls, _) = self.log_state();
        if let Some(p) = ls.pending {
            let why = if ls.attempts >= self.layout.trial_attempts() {
                reason::TRIAL_EXHAUSTED
            } else if self.head(p.slot) == Some(p) && self.verified(p.slot).is_some() {
                return match self.append_ref(kind::BOOT_ATTEMPT, p, reason::NONE) {
                    Ok(()) => Boot::Run(p.slot),
                    Err(_) => Boot::Nothing,
                };
            } else {
                reason::HASH_MISMATCH
            };
            if let Some(c) = ls.confirmed {
                let back = SlotRef { slot: c.slot, ..p };
                let _ = self.append_ref(kind::ROLLBACK, back, why);
            }
        }
        match ls.confirmed {
            Some(c) if self.head(c.slot) == Some(c) => Boot::Run(c.slot),
            _ => Boot::Nothing,
        }
    }
}

impl SimStore {
    fn erase_body(&mut self, t: u8, slot: u8, len: u32) -> Result<(), StoreError> {
        let (body, cap) = self.region(t, slot)?;
        if len > cap {
            return Err(StoreError::Range);
        }
        let start = if matches!(t, target::APP | target::BOOTCORE | target::AGENT) {
            body - HEADER_SIZE as u32
        } else {
            body
        };
        self.erase_range(start, body + len.max(1))
    }

    fn write_body(&mut self, t: u8, slot: u8, offset: u32, data: &[u8]) -> Result<(), StoreError> {
        let (body, cap) = self.region(t, slot)?;
        if offset as u64 + data.len() as u64 > cap as u64 {
            return Err(StoreError::Range);
        }
        self.program(body + offset, data)
    }

    fn place_header(&mut self, slot: u8, hdr: &[u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let a = self
            .layout
            .slots
            .get(slot as usize)
            .ok_or(StoreError::Range)?
            .0;
        self.program(a, hdr)
    }
}

impl SlotStore for SimStore {
    /// The log's fold mapped by dagon-core (a record counts only while
    /// its slot holds the header it names); the factory slot, which no
    /// record names, is reported as written.
    fn status(&mut self) -> StatusResp {
        let (ls, gen) = self.log_state();
        let heads = [self.head(0), self.head(1)];
        let mut st = ls.status(heads, ls.active(), gen);
        if let Some(f) = self.head(2) {
            st.slots[2] = SlotInfo {
                state: SlotState::Written,
                version: f.version,
                digest16: f.digest16,
            };
        }
        st
    }

    fn capacity(&self, t: u8, slot: u8) -> u32 {
        self.region(t, slot).map_or(0, |(_, l)| l)
    }

    fn erase(&mut self, t: u8, slot: u8, len: u32, _: &Transition) -> Result<(), StoreError> {
        self.erase_body(t, slot, len)
    }

    fn write(
        &mut self,
        t: u8,
        slot: u8,
        offset: u32,
        data: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.write_body(t, slot, offset, data)
    }

    fn read(&mut self, t: u8, slot: u8, offset: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let (body, cap) = self.region(t, slot)?;
        if offset as u64 + buf.len() as u64 > cap as u64 {
            return Err(StoreError::Range);
        }
        self.flash()
            .read(body + offset, buf)
            .map_err(|_| StoreError::Flash)
    }

    fn write_header(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.place_header(slot, hdr)
    }

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        let a = self
            .layout
            .slots
            .get(slot as usize)
            .ok_or(StoreError::Range)?
            .0;
        self.flash().read(a, out).map_err(|_| StoreError::Flash)
    }

    fn set_pending(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(Self::log_record(
            kind::SLOT_PENDING,
            slot,
            reason::NONE,
            hdr,
        ))
    }

    fn confirm(
        &mut self,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.append(Self::log_record(kind::CONFIRMED, slot, reason::NONE, hdr))
    }

    /// The boot core slots, folded from their log as the hub's store does
    /// (the confirmed core is the one running: a core settles its trial
    /// before the app starts).
    fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        match t {
            target::APP => Some(self.status()),
            target::BOOTCORE => {
                let c = self.layout.cores?;
                let log = Log::load(&mut Paged(self.flash()), &c.log).ok();
                let (st, gen) = log.map_or((LogState::default(), 0), |l| (l.core, l.generation));
                let heads = [self.core_head(0), self.core_head(1)];
                let running = st.confirmed.map_or(slot_binding::A, |c| c.slot);
                Some(st.status(heads, running, gen))
            }
            _ => None,
        }
    }

    fn write_target_header(
        &mut self,
        t: u8,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        tr: &Transition,
    ) -> Result<(), StoreError> {
        match t {
            target::BOOTCORE | target::AGENT => {
                let (body, _) = self.region(t, slot)?;
                self.program(body - HEADER_SIZE as u32, hdr)
            }
            _ => self.write_header(slot, hdr, tr),
        }
    }

    fn read_target_header(
        &mut self,
        t: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        match t {
            target::BOOTCORE | target::AGENT => {
                let (body, _) = self.region(t, slot)?;
                self.flash()
                    .read(body - HEADER_SIZE as u32, out)
                    .map_err(|_| StoreError::Flash)
            }
            _ => self.read_header(slot, out),
        }
    }

    fn set_target_pending(
        &mut self,
        t: u8,
        slot: u8,
        hdr: &[u8; HEADER_SIZE],
        tr: &Transition,
    ) -> Result<(), StoreError> {
        match t {
            target::BOOTCORE => {
                let c = self.layout.cores.ok_or(StoreError::Unsupported)?;
                let r = Self::log_record(kind::SLOT_PENDING, slot, reason::NONE, hdr);
                let at = SlotRef {
                    slot,
                    version: r.version,
                    digest16: r.digest16,
                };
                let mut f = Paged(self.flash());
                let mut log = Log::load(&mut f, &c.log).map_err(|_| StoreError::Flash)?;
                log.append_core(&mut f, kind::SLOT_PENDING, at, reason::NONE)
                    .map_err(|_| StoreError::Flash)
            }
            _ => self.set_pending(slot, hdr, tr),
        }
    }
}
