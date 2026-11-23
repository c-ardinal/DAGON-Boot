//! The start of a boot core whose stage0 starts the app on its request
//! (`dagon_mcu_common::handed::begin`), over an in-memory flash with a
//! boot core log and an app log: recovery mode on stage0's result, update
//! mode on the app's hand-over or a STAY in the window, else the request
//! for the app; the core's own trial settled first.

use std::cell::RefCell;
use std::collections::VecDeque;

use dagon_agent::update_mode::{Entry, Port};
use dagon_core::bootcore::{app_result, trial_cell, Refusal, CLEARED, START_APP};
use dagon_core::log::{trial_room, Log, LogLayout, SlotRef};
use dagon_events::Clock;
use dagon_format::statelog::{kind, reason, target, FaceHeader, Record};
use dagon_format::Version;
use dagon_mcu_common::handed::{begin, CellIo, Logs, Next, Window};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::{Cmd, Frame};

const SECTOR: u32 = 0x1000;
const CORE_LOG: LogLayout = LogLayout {
    faces: [0x0000, 0x2000],
    face_len: 0x2000,
};
const APP_LOG: LogLayout = LogLayout {
    faces: [0x4000, 0x6000],
    face_len: 0x2000,
};
const REGIONS: [FlashRegion; 1] = [FlashRegion {
    start: 0,
    len: 0x8000,
    erase_size: SECTOR,
    write_size: 4,
    erased_value: 0xFF,
}];
const WINDOW_MS: u16 = 50;
const MAX: u8 = 3;

struct Mem(Vec<u8>);

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let a = addr as usize;
        buf.copy_from_slice(self.0.get(a..a + buf.len()).ok_or(FlashError::Range)?);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let a = addr as usize;
        self.0
            .get_mut(a..a + len as usize)
            .ok_or(FlashError::Range)?
            .fill(0xFF);
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let a = addr as usize;
        for (m, d) in self
            .0
            .get_mut(a..a + data.len())
            .ok_or(FlashError::Range)?
            .iter_mut()
            .zip(data)
        {
            *m &= d;
        }
        Ok(())
    }
}

fn core_ref() -> SlotRef {
    SlotRef {
        slot: 0,
        version: Version::new(1, 0, 0),
        digest16: [0xC0; 16],
    }
}

/// Both logs with a face; the boot core log names the core in slot 0
/// (pending: on trial).
fn flash(pending: bool) -> Mem {
    let mut m = Mem(vec![0xFF; 0x8000]);
    let face = FaceHeader { generation: 1 }.to_bytes();
    for at in [CORE_LOG.faces[0], APP_LOG.faces[0]] {
        m.0[at as usize..][..face.len()].copy_from_slice(&face);
    }
    let r = core_ref();
    let rec = Record {
        kind: if pending {
            kind::SLOT_PENDING
        } else {
            kind::PROVISION
        },
        target: target::BOOTCORE,
        slot: r.slot,
        reason: reason::NONE,
        seq: 1,
        version: r.version,
        digest16: r.digest16,
    }
    .to_bytes();
    let at = CORE_LOG.faces[0] as usize + face.len();
    m.0[at..][..rec.len()].copy_from_slice(&rec);
    m
}

/// A link that brings `requests` one per poll, from the start; the clock
/// moves only when the core waits.
struct Link {
    requests: VecDeque<Vec<u8>>,
    held: Vec<u8>,
    answered: Vec<Option<Vec<u8>>>,
    now: std::rc::Rc<RefCell<u64>>,
}

impl Port for Link {
    fn poll(&mut self) -> Option<(usize, &[u8])> {
        self.held = self.requests.pop_front()?;
        Some((0, &self.held))
    }
    fn reply(&mut self, frame: Option<&[u8]>, _: bool) {
        self.answered.push(frame.map(<[u8]>::to_vec));
    }
    fn delivered(&mut self) -> bool {
        true
    }
    fn wait(&mut self, max_us: u32) {
        *self.now.borrow_mut() += u64::from(max_us);
    }
}

struct Time(std::rc::Rc<RefCell<u64>>);

impl Clock for Time {
    fn now_us(&mut self) -> u64 {
        *self.0.borrow()
    }
}

fn request(cmd: Cmd) -> Vec<u8> {
    let mut b = [0u8; 64];
    let n = Frame {
        flags: 0,
        seq: 1,
        cmd: cmd as u8,
        payload: &[],
    }
    .encode(&mut b)
    .unwrap();
    b[..n].to_vec()
}

/// One start over `f` with the cell at `cell`, the link bringing
/// `requests`: what the core does next, the cell after it, the replies
/// sent in the window and the time it waited (µs).
fn start(f: &mut Mem, cell: [u32; 2], requests: &[Cmd]) -> (Next, [u32; 2], usize, u64) {
    let c = RefCell::new(cell);
    let now = std::rc::Rc::new(RefCell::new(0));
    let mut link = Link {
        requests: requests.iter().map(|&r| request(r)).collect(),
        held: Vec::new(),
        answered: Vec::new(),
        now: now.clone(),
    };
    let mut clock = Time(now.clone());
    let mut fed = 0;
    let mut feed = || fed += 1;
    let next = begin(
        f,
        Logs {
            core: &CORE_LOG,
            app: &APP_LOG,
            running: 0,
            max: MAX,
        },
        CellIo {
            read: || *c.borrow(),
            write: |i: usize, w: u32| c.borrow_mut()[i] = w,
        },
        Window {
            port: &mut link,
            clock: &mut clock,
            window_ms: WINDOW_MS,
            feed_every_ms: 10,
            feed: &mut feed,
        },
    );
    let waited = *now.borrow();
    (next, c.into_inner(), link.answered.len(), waited)
}

fn entry(n: &Next) -> Option<Entry> {
    match n {
        Next::Serve(e, _) => Some(*e),
        _ => None,
    }
}

/// Nothing for the core: the window passes and the cell asks for the app.
#[test]
fn with_nothing_to_serve_the_core_asks_for_the_app_after_the_window() {
    let mut f = flash(false);
    let (next, cell, answered, waited) = start(&mut f, CLEARED, &[]);
    assert!(matches!(next, Next::AskForApp));
    assert_eq!(cell, START_APP);
    assert_eq!(answered, 0);
    assert!(waited >= u64::from(WINDOW_MS) * 1000, "{waited} µs");
}

/// Stage0 started no app: the core clears its result and serves recovery
/// mode at once (no window), whatever else waits.
#[test]
fn stage0s_result_puts_the_core_in_recovery_mode() {
    for r in [Refusal::NoImage, Refusal::Rejected] {
        let mut f = flash(false);
        let mut log = Log::load(&mut f, &APP_LOG).unwrap();
        log.request_stay(&mut f, trial_room(MAX)).unwrap();
        let (next, cell, _, waited) = start(&mut f, app_result(r), &[Cmd::Stay]);
        assert_eq!(entry(&next), Some(Entry::Recovery), "{r:?}");
        assert_eq!(cell, CLEARED);
        assert_eq!(waited, 0);
    }
}

/// The app handed over: update mode without a window, the UPDATE_STAY
/// consumed, so the next start asks for the app again.
#[test]
fn the_apps_hand_over_puts_the_core_in_update_mode_once() {
    let mut f = flash(false);
    let mut log = Log::load(&mut f, &APP_LOG).unwrap();
    log.request_stay(&mut f, trial_room(MAX)).unwrap();
    let (next, cell, _, waited) = start(&mut f, CLEARED, &[]);
    assert_eq!(entry(&next), Some(Entry::HandOver));
    assert_eq!(cell, CLEARED, "no request while serving");
    assert_eq!(waited, 0);
    assert!(!Log::load(&mut f, &APP_LOG).unwrap().update_stay);
    let (next, cell, _, _) = start(&mut f, CLEARED, &[]);
    assert!(matches!(next, Next::AskForApp));
    assert_eq!(cell, START_APP);
}

/// A STAY in the window: update mode, the STAY kept for its answer; any
/// other request in the window gets none.
#[test]
fn a_stay_in_the_window_keeps_the_core_and_other_requests_get_no_answer() {
    let mut f = flash(false);
    let (next, cell, answered, _) = start(&mut f, CLEARED, &[Cmd::Hello, Cmd::Stay]);
    match next {
        Next::Serve(Entry::Stay, Some(s)) => {
            let (fr, _) = Frame::decode(s.frame(), 64).unwrap();
            assert_eq!(fr.cmd, Cmd::Stay as u8);
        }
        _ => panic!("not update mode on the STAY"),
    }
    assert_eq!(cell, CLEARED);
    assert_eq!(answered, 1, "the HELLO before it went unanswered");
    let (next, _, answered, _) = start(&mut flash(false), CLEARED, &[Cmd::Hello]);
    assert!(matches!(next, Next::AskForApp));
    assert_eq!(answered, 1);
}

/// A pending core settles its trial first: it confirms itself and clears
/// the count, then asks for the app.
#[test]
fn a_pending_core_confirms_itself_before_it_asks() {
    let mut f = flash(true);
    let (next, cell, _, _) = start(&mut f, trial_cell(&core_ref(), 1), &[]);
    assert!(matches!(next, Next::AskForApp));
    assert_eq!(cell, START_APP);
    let core = Log::load(&mut f, &CORE_LOG).unwrap().core;
    assert_eq!((core.confirmed, core.pending), (Some(core_ref()), None));
}

/// A pending core whose cell does not hold (stage0 cannot count its
/// starts) rolls back and resets without asking.
#[test]
fn a_pending_core_whose_cell_does_not_hold_resets() {
    let mut f = flash(true);
    let now = std::rc::Rc::new(RefCell::new(0));
    let mut link = Link {
        requests: VecDeque::new(),
        held: Vec::new(),
        answered: Vec::new(),
        now: now.clone(),
    };
    let next = begin(
        &mut f,
        Logs {
            core: &CORE_LOG,
            app: &APP_LOG,
            running: 0,
            max: MAX,
        },
        CellIo {
            read: || CLEARED,
            write: |_: usize, _: u32| {},
        },
        Window {
            port: &mut link,
            clock: &mut Time(now),
            window_ms: WINDOW_MS,
            feed_every_ms: 10,
            feed: &mut || {},
        },
    );
    assert!(matches!(next, Next::Reset));
}
