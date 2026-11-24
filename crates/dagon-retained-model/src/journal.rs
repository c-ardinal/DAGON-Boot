//! The state log with keyed records, over byte-granular flash (faces and
//! records: retained-formats.md 6; the keyed records are no longer in the
//! design, the limitations page lists them).
//!
//! The faces, the face header, the 32-byte record with its CRC and the fold
//! of the records the log holds for image slots are the production ones
//! (`dagon_format::statelog`, `dagon_core::log::LogState`); loading,
//! appending and compaction follow `dagon_core::log::Log` step for step.
//!
//! A keyed record is a record whose kind is `0x80..=0xFE`. Its fold needs
//! no knowledge of what it means: the latest put of a key is the value of
//! that key, and an erase removes the keys it names ([`Keyed::supersedes`]).
//! Compaction copies, byte for byte, every put no later record supersedes,
//! so code that knows no keyed class carries all of them.

use crate::mem::{Cut, Mem};
use dagon_core::log::{LogState, SlotRef};
use dagon_format::statelog::{kind, reason, slot, target, FaceHeader, Record, RECORD_SIZE};
use dagon_format::{crc32, Version};

/// Kind bit 7: a keyed record.
pub const KEYED: u8 = 0x80;
/// Kind bit 6 of a keyed record: an erase.
pub const ERASE: u8 = 0x40;
/// The largest class (`0xFF` is the erased kind).
pub const CLASS_MAX: u8 = 0x3E;
/// Bytes 8..28: the key, then the value.
pub const BODY: usize = 20;

type Bytes = [u8; RECORD_SIZE];

/// A keyed record as the format reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keyed {
    pub erase: bool,
    pub class: u8,
    /// A put: the length of its key. An erase: the length of the key
    /// prefix it names.
    pub n: u8,
    /// An erase: the last class it covers (`class..=last`). A put: 0.
    pub last: u8,
    pub seq: u32,
    pub body: [u8; BODY],
}

impl Keyed {
    pub fn put(class: u8, key: &[u8], value: &[u8]) -> Keyed {
        let mut body = [0; BODY];
        body[..key.len()].copy_from_slice(key);
        body[key.len()..key.len() + value.len()].copy_from_slice(value);
        Keyed {
            erase: false,
            class,
            n: key.len() as u8,
            last: 0,
            seq: 0,
            body,
        }
    }

    pub fn erase(class: u8, last: u8, prefix: &[u8]) -> Keyed {
        let mut body = [0; BODY];
        body[..prefix.len()].copy_from_slice(prefix);
        Keyed {
            erase: true,
            class,
            n: prefix.len() as u8,
            last,
            seq: 0,
            body,
        }
    }

    /// `None` for anything that is not a keyed record of this format: a
    /// failed CRC, another kind, a class above 62, a length past the body, a
    /// class range that ends before it starts, a reserved byte that is not
    /// zero.
    pub fn parse(b: &Bytes) -> Option<Keyed> {
        Record::parse(b).ok()?;
        let (k, n, last) = (b[0], b[1], b[2]);
        if k & KEYED == 0 || k == kind::EMPTY || usize::from(n) > BODY || b[3] != 0 {
            return None;
        }
        let (erase, class) = (k & ERASE != 0, k & 0x3F);
        if class > CLASS_MAX
            || if erase {
                last < class || last > CLASS_MAX
            } else {
                last != 0
            }
        {
            return None;
        }
        Some(Keyed {
            erase,
            class,
            n,
            last,
            seq: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            body: b[8..28].try_into().expect("20 bytes"),
        })
    }

    pub fn to_bytes(&self) -> Bytes {
        let mut b = [0u8; RECORD_SIZE];
        b[0] = KEYED | if self.erase { ERASE } else { 0 } | self.class;
        b[1] = self.n;
        b[2] = self.last;
        b[4..8].copy_from_slice(&self.seq.to_le_bytes());
        b[8..28].copy_from_slice(&self.body);
        let crc = crc32(&b[..28]);
        b[28..].copy_from_slice(&crc.to_le_bytes());
        b
    }

    pub fn key(&self) -> &[u8] {
        &self.body[..usize::from(self.n)]
    }

    pub fn value(&self) -> &[u8] {
        &self.body[usize::from(self.n)..]
    }

    /// The fold rule: this record, later in the log than the put `older`,
    /// removes it. A put removes the put of its own key; an erase removes
    /// every put of its classes whose key starts with its prefix.
    pub fn supersedes(&self, older: &Keyed) -> bool {
        if older.erase {
            return false;
        }
        if self.erase {
            (self.class..=self.last).contains(&older.class) && older.key().starts_with(self.key())
        } else {
            self.class == older.class && self.key() == older.key()
        }
    }
}

/// Where the two faces are in the flash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub faces: [usize; 2],
    pub face_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Power failed inside the operation.
    Cut,
    NoValidFace,
    /// The face cannot take the record, compacted or not.
    Full,
}

impl From<Cut> for Error {
    fn from(_: Cut) -> Self {
        Error::Cut
    }
}

/// The active face and what its records fold to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    pub state: LogState,
    pub core: LogState,
    pub update_stay: bool,
    pub generation: u32,
    /// The puts no later record supersedes, in log order.
    pub keyed: Vec<Keyed>,
    /// Records appended (compaction's copies are not appends).
    pub writes: u32,
    layout: Layout,
    face: usize,
    next: Option<usize>,
    next_seq: u32,
}

fn at(mem: &Mem, addr: usize) -> Bytes {
    mem.bytes()[addr..addr + RECORD_SIZE]
        .try_into()
        .expect("32 bytes")
}

const NO_SLOT: SlotRef = SlotRef {
    slot: slot::NONE,
    version: Version::new(0, 0, 0),
    digest16: [0; 16],
};

impl Journal {
    /// As `Log::load`: the valid face of the larger generation, folded up
    /// to its first erased record or its first record failing the CRC.
    pub fn load(mem: &Mem, layout: &Layout) -> Result<Journal, Error> {
        let mut best: Option<(usize, u32)> = None;
        for (i, &face) in layout.faces.iter().enumerate() {
            if let Ok(h) = FaceHeader::parse(&at(mem, face)) {
                if best.is_none_or(|(_, g)| h.generation > g) {
                    best = Some((i, h.generation));
                }
            }
        }
        let (face, generation) = best.ok_or(Error::NoValidFace)?;
        let mut j = Journal {
            state: LogState::default(),
            core: LogState::default(),
            update_stay: false,
            generation,
            keyed: Vec::new(),
            writes: 0,
            layout: *layout,
            face,
            next: None,
            next_seq: 1,
        };
        let mut addr = layout.faces[face] + RECORD_SIZE;
        for b in j.records(mem) {
            j.fold(&b);
            addr += RECORD_SIZE;
        }
        if addr + RECORD_SIZE <= j.face_end() && Record::is_erased(&at(mem, addr)) {
            j.next = Some(addr);
        }
        Ok(j)
    }

    fn face_end(&self) -> usize {
        self.layout.faces[self.face] + self.layout.face_len
    }

    /// The records of the active face the scan accepts, in order.
    fn records(&self, mem: &Mem) -> Vec<Bytes> {
        let mut addr = self.layout.faces[self.face] + RECORD_SIZE;
        let mut out = Vec::new();
        while addr + RECORD_SIZE <= self.face_end() {
            let b = at(mem, addr);
            if Record::is_erased(&b) || Record::parse(&b).is_err() {
                break;
            }
            out.push(b);
            addr += RECORD_SIZE;
        }
        out
    }

    fn fold(&mut self, b: &Bytes) {
        let Ok(r) = Record::parse(b) else { return };
        self.state = self.state.apply(&r);
        self.core = self.core.apply_core(&r);
        match r.kind {
            kind::UPDATE_STAY => self.update_stay = true,
            kind::UPDATE_DONE => self.update_stay = false,
            _ => {}
        }
        self.next_seq = self.next_seq.max(r.seq.wrapping_add(1));
        if let Some(k) = Keyed::parse(b) {
            self.keyed.retain(|o| !k.supersedes(o));
            if !k.erase {
                self.keyed.push(k);
            }
        }
    }

    /// Programs one record at the end of the face and folds it.
    fn write(&mut self, mem: &mut Mem, b: Bytes) -> Result<(), Error> {
        let addr = self.next.ok_or(Error::Full)?;
        for (i, v) in b.into_iter().enumerate() {
            mem.program(addr + i, v)?;
        }
        self.fold(&b);
        let after = addr + RECORD_SIZE;
        self.next = (after + RECORD_SIZE <= self.face_end()).then_some(after);
        Ok(())
    }

    fn slot_record(&self, t: u8, k: u8, at: SlotRef, why: u8) -> Bytes {
        Record {
            kind: k,
            target: t,
            slot: at.slot,
            reason: why,
            seq: self.next_seq,
            version: at.version,
            digest16: at.digest16,
        }
        .to_bytes()
    }

    /// An append goes only into an erased slot, as it does for a `Log`
    /// loaded for that append: a slot that is not erased (foreign bytes
    /// since the load) ends the face, and the append compacts first.
    fn settle(&mut self, mem: &Mem) {
        self.next = self.next.filter(|&a| Record::is_erased(&at(mem, a)));
    }

    fn push(&mut self, mem: &mut Mem, b: Bytes) -> Result<(), Error> {
        self.write(mem, b)?;
        self.writes += 1;
        Ok(())
    }

    /// One record for an image slot: as `Log::append` and `append_core`,
    /// or with `in_place` as `Log::append_in_place` (never compacting).
    pub fn append(
        &mut self,
        mem: &mut Mem,
        t: u8,
        k: u8,
        at: SlotRef,
        why: u8,
        in_place: bool,
    ) -> Result<(), Error> {
        self.settle(mem);
        // The record takes its number after a compaction, as in `Log`.
        if !in_place && self.next.is_none() {
            self.compact(mem)?;
        }
        let b = self.slot_record(t, k, at, why);
        self.push(mem, b)
    }

    /// One keyed record, compacting first when the face cannot take it.
    pub fn append_keyed(&mut self, mem: &mut Mem, k: Keyed) -> Result<(), Error> {
        self.settle(mem);
        if self.next.is_none() {
            self.compact(mem)?;
        }
        let b = Keyed {
            seq: self.next_seq,
            ..k
        }
        .to_bytes();
        self.push(mem, b)
    }

    /// Records the active face still takes.
    pub fn room(&self) -> usize {
        self.next.map_or(0, |a| (self.face_end() - a) / RECORD_SIZE)
    }

    /// As `Log::reserve`.
    pub fn reserve(&mut self, mem: &mut Mem, records: usize) -> Result<(), Error> {
        self.settle(mem);
        if self.room() >= records {
            return Ok(());
        }
        self.compact(mem)?;
        if self.room() >= records {
            Ok(())
        } else {
            Err(Error::Full)
        }
    }

    /// As `Log::request_stay`.
    pub fn request_stay(&mut self, mem: &mut Mem, keep: usize) -> Result<(), Error> {
        if self.update_stay {
            return Ok(());
        }
        self.reserve(mem, keep + 2)?;
        self.append(
            mem,
            target::APP,
            kind::UPDATE_STAY,
            NO_SLOT,
            reason::NONE,
            true,
        )
    }

    /// As `Log::take_stay`.
    pub fn take_stay(&mut self, mem: &mut Mem) -> Result<bool, Error> {
        if !self.update_stay {
            return Ok(false);
        }
        self.append(
            mem,
            target::APP,
            kind::UPDATE_DONE,
            NO_SLOT,
            reason::NONE,
            true,
        )?;
        Ok(true)
    }

    /// The puts of the active face no later record of it supersedes, as
    /// their bytes. It reads the face record by record and keeps no table,
    /// so it runs in the RAM of the smallest reader.
    pub fn live(&self, mem: &Mem) -> Vec<Bytes> {
        let recs = self.records(mem);
        let later = |i: usize, k: &Keyed| {
            recs[i + 1..]
                .iter()
                .filter_map(Keyed::parse)
                .any(|l| l.supersedes(k))
        };
        recs.iter()
            .enumerate()
            .filter(|(i, b)| Keyed::parse(b).is_some_and(|k| !k.erase && !later(*i, &k)))
            .map(|(_, b)| *b)
            .collect()
    }

    /// Copies the fold into the other face and makes it the active one; on
    /// any error the journal stays on the face it had.
    pub fn compact(&mut self, mem: &mut Mem) -> Result<(), Error> {
        let old = self.clone();
        let result = self.copy_to_other_face(mem);
        if result.is_err() {
            *self = old;
        }
        result
    }

    /// `Log::copy_to_other_face`, then every live put as it is, then the
    /// header.
    fn copy_to_other_face(&mut self, mem: &mut Mem) -> Result<(), Error> {
        let carried = [(target::BOOTCORE, self.core), (target::APP, self.state)];
        let stay = self.update_stay;
        let live = self.live(mem);
        let generation = self.generation.wrapping_add(1);
        self.face ^= 1;
        let face = self.layout.faces[self.face];
        mem.erase(face..face + self.layout.face_len)?;
        self.next = Some(face + RECORD_SIZE);
        self.state = LogState::default();
        self.core = LogState::default();
        self.update_stay = false;
        self.keyed.clear();
        for (t, s) in carried {
            for (k, at, why) in s.carry() {
                let b = self.slot_record(t, k, at, why);
                self.write(mem, b)?;
            }
        }
        if stay {
            let b = self.slot_record(target::APP, kind::UPDATE_STAY, NO_SLOT, reason::NONE);
            self.write(mem, b)?;
        }
        for b in live {
            self.write(mem, b)?;
        }
        let header = FaceHeader { generation }.to_bytes();
        for (i, v) in header.into_iter().enumerate() {
            mem.program(face + i, v)?;
        }
        self.generation = generation;
        Ok(())
    }
}
