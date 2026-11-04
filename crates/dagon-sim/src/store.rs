//! SimStore: the agent's receiver over a SimFlash with a kind S layout
//! (inline headers, one state log face), plus the boot decision made at reset.

use crate::flash::SimFlash;
use dagon_agent::{SlotStore, StoreError, Transition};
use dagon_format::header::{self, ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{self, kind, reason, FaceHeader, Record, RECORD_SIZE};
use dagon_format::{sha256, Version};
use dagon_port::Flash;
use dagon_proto::payload::{target, SlotState, StatusResp};

/// Trial boots of a pending image before it is rolled back.
pub const MAX_ATTEMPTS: u8 = 3;

/// Where things live in a node's flash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Boot core region; all-erased means the chip has nothing to run.
    pub bootcore: (u32, u32),
    /// State log face (one erase unit).
    pub log: (u32, u32),
    /// APP slots A and B: header inline at the slot start, body at +512.
    pub slots: [(u32, u32); 2],
    /// Hub display copy of the topology (body only), if this board keeps one.
    pub topology: Option<(u32, u32)>,
    /// Factory (spare) slot, reported as STATUS slot 2; written only by
    /// [`SimStore::provision_factory`], never a boot or transfer target.
    pub factory: Option<(u32, u32)>,
}

impl Layout {
    /// STM32F401xE: boot core sectors 0-1, log A sector 2, APP-A sector 5,
    /// APP-B sector 6.
    pub const STM32F401XE: Layout = Layout {
        bootcore: (0x0800_0000, 0x8000),
        log: (0x0800_8000, 0x4000),
        slots: [(0x0802_0000, 0x2_0000), (0x0804_0000, 0x2_0000)],
        topology: None,
        factory: None,
    };

    /// Hub on RP2040 with 2 MiB flash.
    pub const RP2040_HUB: Layout = Layout {
        bootcore: (0x1000_0000, 0x1_0000),
        log: (0x1001_0000, 0x1000),
        slots: [(0x1002_0000, 0x8_0000), (0x100A_0000, 0x8_0000)],
        topology: Some((0x1012_0000, 0x1_0000)),
        factory: None,
    };

    /// ESP32-S3 watch (4 MiB modelled): loader, selection data, two OTA
    /// slots and a factory slot. Simplified to kind S semantics.
    pub const ESP32S3_WATCH: Layout = Layout {
        bootcore: (0x0000_0000, 0x1_0000),
        log: (0x0001_0000, 0x1000),
        slots: [(0x0011_0000, 0x10_0000), (0x0021_0000, 0x10_0000)],
        topology: None,
        factory: Some((0x0031_0000, 0x0F_0000)),
    };
}

/// What the boot core decided at reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boot {
    /// Runs this slot (trying or confirmed).
    Run(u8),
    /// Nothing valid to run: the chip stays silent.
    Nothing,
}

pub struct SimStore {
    pub flash: SimFlash,
    pub layout: Layout,
    pubkey: [u8; 32],
}

impl Default for SimStore {
    fn default() -> Self {
        SimStore::new(SimFlash::new(&[], 0), Layout::STM32F401XE, [0; 32])
    }
}

impl SimStore {
    pub fn new(flash: SimFlash, layout: Layout, pubkey: [u8; 32]) -> Self {
        SimStore {
            flash,
            layout,
            pubkey,
        }
    }

    fn region(&self, t: u8, slot: u8) -> Result<(u32, u32), StoreError> {
        match t {
            target::APP if slot < 2 => {
                let (a, l) = self.layout.slots[slot as usize];
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
        self.flash.read(self.slot_base(slot)?, &mut h).ok()?;
        ImageHeader::parse(&h).ok().map(|_| h)
    }

    /// Face generation (0 = no valid face), record places used (torn ones
    /// included) and the valid records.
    fn records(&mut self) -> (u32, usize, Vec<Record>) {
        let (base, len) = self.layout.log;
        let mut buf = vec![0u8; len as usize];
        if self.flash.read(base, &mut buf).is_err() {
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
            self.flash.erase(base, len).map_err(|_| StoreError::Flash)?;
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
        if data.is_empty() {
            return Ok(());
        }
        let ws = self
            .flash
            .unit_at(addr)
            .ok_or(StoreError::Range)?
            .write_size as usize;
        let start = addr as usize / ws * ws;
        let mut buf = vec![0xFF; (addr as usize + data.len()).div_ceil(ws) * ws - start];
        self.flash
            .read(start as u32, &mut buf)
            .map_err(|_| StoreError::Flash)?;
        // Bytes around `data` keep their current value, so already programmed
        // neighbours in the same write unit are rewritten with themselves.
        buf[addr as usize - start..][..data.len()].copy_from_slice(data);
        self.flash
            .write(start as u32, &buf)
            .map_err(|_| StoreError::Flash)
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
        self.flash
            .read(b, &mut marker)
            .map_err(|_| StoreError::Flash)?;
        if marker == [0xFF; 4] {
            self.program(b, b"DGBC")?;
        }
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
            let unit = self.flash.unit_at(a).ok_or(StoreError::Range)?;
            let unit_start = a - (a - unit.start) % unit.erase_size;
            self.flash
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

    fn bootcore_present(&mut self) -> bool {
        let (b, l) = self.layout.bootcore;
        let mut buf = vec![0u8; l as usize];
        self.flash.read(b, &mut buf).is_ok() && buf.iter().any(|&x| x != 0xFF)
    }

    fn verified(&mut self, slot: u8) -> Option<[u8; HEADER_SIZE]> {
        let hdr = self.header_at(slot)?;
        let h = header::verify_header(&hdr, &self.pubkey).ok()?;
        let mut body = vec![0u8; h.payload_size as usize];
        self.read(target::APP, slot, 0, &mut body).ok()?;
        (sha256(&body) == h.payload_sha256).then_some(hdr)
    }

    /// Boot core decision at reset: start a verified pending image on trial,
    /// count another attempt, or roll an exhausted trial back.
    pub fn boot(&mut self) -> Boot {
        if !self.bootcore_present() {
            return Boot::Nothing;
        }
        let st = self.status();
        let pending = (0..2u8)
            .rev()
            .find(|&s| st.slots[s as usize].state == SlotState::Pending);
        let trying = st
            .slots
            .get(st.active as usize)
            .is_some_and(|s| s.state == SlotState::Trying);
        let step = if let Some(s) = pending.filter(|&s| self.verified(s).is_some()) {
            Some((kind::BOOT_ATTEMPT, s, reason::NONE))
        } else if trying && st.attempts >= MAX_ATTEMPTS {
            Some((kind::ROLLBACK, st.active, reason::TRIAL_EXHAUSTED))
        } else if trying {
            Some((kind::BOOT_ATTEMPT, st.active, reason::NONE))
        } else {
            None
        };
        if let Some((k, s, why)) = step {
            let Some(hdr) = self.header_at(s) else {
                return Boot::Nothing;
            };
            if self.append(Self::log_record(k, s, why, &hdr)).is_err() {
                return Boot::Nothing;
            }
        }
        let st = self.status();
        match st.slots.get(st.active as usize) {
            Some(s) if matches!(s.state, SlotState::Trying | SlotState::Confirmed) => {
                Boot::Run(st.active)
            }
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
        let start = if t == target::APP {
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
    /// Folds the log; a record only counts while its slot still holds the
    /// header it names.
    fn status(&mut self) -> StatusResp {
        let mut st = StatusResp {
            active: statelog::slot::NONE,
            confirmed: statelog::slot::NONE,
            ..Default::default()
        };
        let mut digests = [None; 3];
        for s in 0..3u8 {
            if let Some(h) = self.header_at(s) {
                let info = &mut st.slots[s as usize];
                info.state = SlotState::Written;
                info.version = ImageHeader::parse(&h)
                    .map(|h| h.version)
                    .unwrap_or_default();
                info.digest16 = header::header_digest16(&h);
                digests[s as usize] = Some(info.digest16);
            }
        }
        let (gen, _, recs) = self.records();
        st.log_generation = gen;
        for r in recs.iter().filter(|r| r.target == statelog::target::APP) {
            let s = r.slot as usize;
            if digests.get(s).copied().flatten() != Some(r.digest16) {
                continue;
            }
            match r.kind {
                kind::PROVISION | kind::CONFIRMED => {
                    if st.confirmed < 2 && st.confirmed as usize != s {
                        st.slots[st.confirmed as usize].state = SlotState::Written;
                    }
                    st.slots[s].state = SlotState::Confirmed;
                    (st.active, st.confirmed, st.attempts) = (r.slot, r.slot, 0);
                }
                kind::SLOT_PENDING => st.slots[s].state = SlotState::Pending,
                kind::BOOT_ATTEMPT => {
                    st.slots[s].state = SlotState::Trying;
                    st.active = r.slot;
                    st.attempts += 1;
                }
                kind::ROLLBACK => {
                    st.slots[s].state = SlotState::Reverted;
                    (st.active, st.attempts, st.last_reason) = (st.confirmed, 0, r.reason);
                }
                _ => {}
            }
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
        self.flash
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
        self.flash.read(a, out).map_err(|_| StoreError::Flash)
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
}
