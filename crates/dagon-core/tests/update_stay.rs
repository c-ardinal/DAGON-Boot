//! The update-mode records of the state log: UPDATE_STAY (the app asks its
//! boot core to stay in update mode at the next start) and UPDATE_DONE (the
//! boot core consumed it). Readers that predate them skip them; the boot
//! core consumes a stay without erasing; a compaction keeps an unconsumed
//! stay.

mod bench;

use bench::*;
use dagon_core::log::{trial_room, LogState};

/// A face of 8 records: compaction comes soon.
const SMALL: LogLayout = LogLayout {
    faces: [0x0800_8000, 0x0800_8120],
    face_len: 0x120,
};

/// Counts the erases a run makes.
struct Erases<'a> {
    m: &'a mut Mem,
    erases: usize,
}

impl Flash for Erases<'_> {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.m.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.erases += 1;
        self.m.erase(addr, len)
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.m.write(addr, data)
    }
}

fn rec(kind: u8, at: SlotRef, seq: u32) -> Record {
    Record {
        kind,
        target: target::APP,
        slot: at.slot,
        reason: reason::NONE,
        seq,
        version: at.version,
        digest16: at.digest16,
    }
}

fn provisioned(layout: &LogLayout) -> (Mem, SlotRef) {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    m.write(layout.faces[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let mut log = Log::load(&mut m, layout).unwrap();
    log.append(&mut m, kind::PROVISION, a, reason::NONE)
        .unwrap();
    (m, a)
}

#[test]
fn the_records_take_the_design_numbers() {
    assert_eq!((kind::UPDATE_STAY, kind::UPDATE_DONE), (0x06, 0x09));
}

/// The fold every reader of this release and the ones before it run
/// (`LogState::apply`) passes over both records: the same state with or
/// without them.
#[test]
fn readers_that_predate_the_records_skip_them() {
    let a = slot_ref(A, &image(A, 0).0);
    let b = slot_ref(B, &image(B, 1).0);
    let plain = [
        rec(kind::PROVISION, a, 1),
        rec(kind::SLOT_PENDING, b, 2),
        rec(kind::BOOT_ATTEMPT, b, 3),
    ];
    let marker = |k, seq| Record {
        kind: k,
        ..rec(k, a, seq)
    };
    let with = [
        plain[0],
        marker(kind::UPDATE_STAY, 2),
        plain[1],
        marker(kind::UPDATE_DONE, 4),
        plain[2],
    ];
    let fold = |rs: &[Record]| rs.iter().fold(LogState::default(), |s, r| s.apply(r));
    assert_eq!(fold(&with), fold(&plain));
    assert!(fold(&with).trying(B));
}

/// The app writes UPDATE_STAY; the boot core reads it, consumes it with
/// UPDATE_DONE written into room the face has (no erase), and the next
/// start sees none.
#[test]
fn the_boot_core_consumes_a_stay_once_without_erasing() {
    let (mut m, a) = provisioned(&LOG);
    let mut log = Log::load(&mut m, &LOG).unwrap();
    assert!(!log.update_stay);
    log.request_stay(&mut m, trial_room(3)).unwrap();
    assert!(log.update_stay);

    let mut f = Erases {
        m: &mut m,
        erases: 0,
    };
    let mut log = Log::load(&mut f, &LOG).unwrap();
    assert!(log.update_stay, "the next start reads the stay");
    assert_eq!(log.take_stay(&mut f), Ok(true));
    assert!(!log.update_stay);
    let mut again = Log::load(&mut f, &LOG).unwrap();
    assert!(!again.update_stay, "consumed for good");
    assert_eq!(again.take_stay(&mut f), Ok(false));
    assert_eq!(f.erases, 0, "the boot core never erases to consume it");
    assert_eq!(again.state.confirmed, Some(a), "the APP state is kept");
}

/// The app makes room for the stay, its consumption and the trial room
/// before it writes the stay; a full face is compacted then (the app
/// runs, no trial is waiting), never by the boot core.
#[test]
fn the_app_makes_room_for_the_stay_and_its_consumption() {
    let (mut m, a) = provisioned(&SMALL);
    let keep = 2;
    let mut log = Log::load(&mut m, &SMALL).unwrap();
    while log.room() > keep {
        log.append(&mut m, kind::CONFIRMED, a, reason::NONE)
            .unwrap();
    }
    let generation = log.generation;
    log.request_stay(&mut m, keep).unwrap();
    assert_eq!(log.generation, generation + 1, "compacted first");
    assert!(log.room() > keep, "room for UPDATE_DONE and the trials");

    let mut f = Erases {
        m: &mut m,
        erases: 0,
    };
    let mut log = Log::load(&mut f, &SMALL).unwrap();
    assert_eq!(log.take_stay(&mut f), Ok(true));
    assert_eq!(f.erases, 0);
    assert!(log.room() >= keep);
}

/// A compaction while a stay is unconsumed copies it: the hand-over is
/// not lost. A consumed one is not copied.
#[test]
fn a_compaction_keeps_an_unconsumed_stay() {
    let (mut m, a) = provisioned(&SMALL);
    let mut log = Log::load(&mut m, &SMALL).unwrap();
    log.request_stay(&mut m, 0).unwrap();
    let generation = log.generation;
    while log.generation == generation {
        log.append(&mut m, kind::CONFIRMED, a, reason::NONE)
            .unwrap();
    }
    let mut log = Log::load(&mut m, &SMALL).unwrap();
    assert!(log.update_stay, "carried into the new face");
    assert_eq!(log.state.confirmed, Some(a));

    assert_eq!(log.take_stay(&mut m), Ok(true));
    let generation = log.generation;
    while log.generation == generation {
        log.append(&mut m, kind::CONFIRMED, a, reason::NONE)
            .unwrap();
    }
    assert!(!Log::load(&mut m, &SMALL).unwrap().update_stay);
}

/// With no room left the boot core leaves the stay as it is and reports
/// it: it never compacts to consume it.
#[test]
fn a_stay_with_no_room_to_consume_is_refused_not_compacted() {
    let (mut m, a) = provisioned(&SMALL);
    let mut log = Log::load(&mut m, &SMALL).unwrap();
    log.request_stay(&mut m, 0).unwrap();
    while log.room() > 0 {
        log.append(&mut m, kind::CONFIRMED, a, reason::NONE)
            .unwrap();
    }
    let mut f = Erases {
        m: &mut m,
        erases: 0,
    };
    let mut log = Log::load(&mut f, &SMALL).unwrap();
    assert_eq!(log.take_stay(&mut f), Err(LogError::Full));
    assert_eq!(f.erases, 0);
}
