//! The boot core receiving on its I2C target link, over the F401 layout:
//! the listen window that takes a STAY, update mode answering the shared
//! session (`Agent<_, _, _, BootCore>`) until REBOOT or 60 s without a
//! request, and a write through update mode that the next start tries. The
//! link is the boot core's polled port (`dagon_agent::update_mode::Port`)
//! with the I2C mailbox the app's link uses; the hub's requests arrive at
//! set times. Between events the boot core sleeps in the port's wait (on
//! hardware WFE until the link or a timer wakes it): the clock advances
//! only there, so a loop that polls without waiting never sees time pass.

mod cut;

use cut::*;

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dagon_agent::update_mode::{listen, serve, Exit, Port, EXIT_IDLE_MS};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, Config};
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::mailbox::Mailbox;
use dagon_mcu_stm32f401re::{KindSStore, ERASE_REGIONS, TRIAL_ATTEMPTS};
use dagon_proto::payload::{
    mode, reboot_mode, role, target, HelloReq, HelloResp, SetPending, XferBegin, XferChunk,
};
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status, PROTO_VERSION};
use ed25519_compact::{KeyPair, Seed};

/// The longest the boot core goes without feeding its watchdog.
const FEED_EVERY_MS: u32 = 2_000;

/// The hub's side of the I2C mailbox, as the boot core's port sees it:
/// each request arrives once the clock reaches its time; a reply held is
/// read whole by the hub right away (an event that ends a wait).
struct Hub {
    now: Rc<Cell<u64>>,
    due: VecDeque<(u64, Vec<u8>)>,
    rx: Vec<u8>,
    mailbox: Mailbox,
    delivered: bool,
    polls: usize,
    waits: usize,
    replies: Vec<Option<Vec<u8>>>,
}

impl Hub {
    fn new(now: Rc<Cell<u64>>, due: Vec<(u64, Vec<u8>)>) -> Self {
        Hub {
            now,
            due: due.into(),
            rx: Vec::new(),
            mailbox: Mailbox::default(),
            delivered: false,
            polls: 0,
            waits: 0,
            replies: Vec::new(),
        }
    }

    /// The hub reads the held reply: the state, then the frame.
    fn read_reply(&mut self) {
        let state = self.mailbox.read_data().len() - 1;
        if !self.mailbox.read_done(state) {
            let frame = self.mailbox.read_data().len() - 1;
            self.delivered |= self.mailbox.read_done(frame);
        }
    }
}

impl Port for Hub {
    fn poll(&mut self) -> Option<&[u8]> {
        self.polls += 1;
        self.read_reply();
        let at = self.due.front()?.0;
        if self.now.get() < at {
            return None;
        }
        self.rx = self.due.pop_front().unwrap().1;
        self.mailbox.request();
        Some(&self.rx)
    }

    fn reply(&mut self, frame: Option<&[u8]>, reboot: bool) {
        self.replies.push(frame.map(<[u8]>::to_vec));
        self.mailbox.respond(frame, reboot);
        self.delivered = false;
    }

    fn delivered(&mut self) -> bool {
        self.delivered
    }

    /// Sleeps until the next request arrives or `max_us` pass, whichever
    /// comes first; a reply waiting to be read wakes it at once.
    fn wait(&mut self, max_us: u32) {
        self.waits += 1;
        if matches!(self.mailbox.read_data().first(), Some(1)) {
            self.read_reply();
            return;
        }
        let until = self.now.get() + u64::from(max_us);
        let next = self.due.front().map_or(until, |(at, _)| (*at).min(until));
        self.now.set(next.max(self.now.get()));
    }
}

/// The boot core's clock: the time the waits have let pass.
fn clock(now: &Rc<Cell<u64>>) -> impl FnMut() -> u64 {
    let now = now.clone();
    move || now.get()
}

fn frame(seq: u8, cmd: Cmd, payload: &[u8]) -> Vec<u8> {
    let mut buf = [0u8; 300];
    let n = Frame {
        flags: 0,
        seq,
        cmd: cmd as u8,
        payload,
    }
    .encode(&mut buf)
    .unwrap();
    buf[..n].to_vec()
}

fn stay(seq: u8) -> Vec<u8> {
    frame(seq, Cmd::Stay, &[])
}

fn hello(seq: u8) -> Vec<u8> {
    let mut p = [0u8; 16];
    let n = HelloReq {
        proto_ver: PROTO_VERSION as u32,
        max_payload: max_payload::I2C as u16,
    }
    .encode(&mut p)
    .unwrap();
    frame(seq, Cmd::Hello, &p[..n])
}

fn ms(ms: u64) -> u64 {
    ms * 1_000
}

/// The status and body of a reply frame.
fn answer(reply: &[u8]) -> (Status, Vec<u8>) {
    let (f, _) = Frame::decode(reply, 300).unwrap();
    let (st, body) = split_status(f.payload).unwrap();
    (st, body.to_vec())
}

static TICKS: AtomicUsize = AtomicUsize::new(0);

fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

type Verifier = Ed25519Small<fn()>;

/// The boot core's verifier: the small Ed25519 that ticks the watchdog at
/// every step of a check.
fn verifier() -> Verifier {
    Ed25519Small::with_tick(*KeyPair::from_seed(Seed::new(SEED)).pk, tick as fn())
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

fn chain() -> Chain {
    let events: &'static mut Published<(), 4> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    let periods = Watchdog::new(TRIAL_WDT_MS, CONFIRMED_WDT_MS);
    compose(Layer::App, events, Nop, Nop, Bounds::new(now, periods))
}

/// The session the boot core builds once it is in update mode.
fn session(f: &Cut) -> Agent<KindSStore<Cut>, Chain, Verifier, BootCore> {
    let store = KindSStore::load(f.clone(), &LOG, SLOTS, &ERASE_REGIONS)
        .unwrap()
        .with_cores(&CORE_SLOTS, true);
    let cfg = Config {
        mcu_id: MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag("seg-f401-7seg"),
        role: role::NODE,
        mode: mode::UPDATE,
        max_payload: max_payload::I2C as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    };
    Agent::for_writer(cfg, verifier(), store, chain())
}

fn app(slot: u8) -> Vec<u8> {
    signed(
        image_kind::APP,
        slot,
        SLOTS.slots[usize::from(slot)].0,
        4,
        2000,
    )
}

/// The host's requests that write `img` into APP `slot` and set it
/// pending, from seq `seq` on.
fn write_requests(slot: u8, img: &[u8], mut seq: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = [0u8; 256];
    let mut push = |cmd, p: &[u8]| {
        seq = seq.wrapping_add(1);
        out.push(frame(seq, cmd, p));
    };
    let n = XferBegin {
        target: target::APP,
        slot,
        total_len: img.len() as u32,
        hdr_len: HEADER_SIZE as u16,
    }
    .encode(&mut p)
    .unwrap();
    push(Cmd::XferBegin, &p[..n]);
    let (hdr, body) = img.split_at(HEADER_SIZE);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(200).enumerate() {
            let n = XferChunk {
                offset: (i * 200) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            push(cmd, &p[..n]);
        }
    }
    push(Cmd::XferEnd, &[]);
    let n = SetPending {
        target: target::APP,
        slot,
    }
    .encode(&mut p)
    .unwrap();
    push(Cmd::SetPending, &p[..n]);
    out
}

/// A STAY that has arrived inside the window keeps the boot core: the
/// window hands it on for update mode to answer.
#[test]
fn a_stay_within_the_listen_window_keeps_the_boot_core() {
    let now = Rc::new(Cell::new(0));
    let mut hub = Hub::new(now.clone(), vec![(ms(2), stay(7))]);
    let mut feeds = 0;
    let kept = listen(&mut hub, &mut clock(&now), 50, FEED_EVERY_MS, &mut || feeds += 1);
    assert_eq!(kept.as_ref().map(|s| s.frame().to_vec()), Some(stay(7)));
    assert_eq!(now.get(), ms(2), "the window ended at the STAY");
    assert!(feeds > 0, "the watchdog is fed while the window is open");
}

/// No STAY, or one after the window, and the window ends exactly at its
/// length, having slept rather than polled; the late STAY is left
/// unanswered.
#[test]
fn the_listen_window_ends_on_time_without_a_stay() {
    for due in [vec![], vec![(ms(60), stay(1))]] {
        let now = Rc::new(Cell::new(0));
        let mut hub = Hub::new(now.clone(), due);
        assert!(listen(&mut hub, &mut clock(&now), 50, FEED_EVERY_MS, &mut || {}).is_none());
        assert_eq!(now.get(), ms(50));
        assert!(hub.polls <= 3, "{} polls", hub.polls);
        assert!(hub.replies.is_empty());
    }
}

/// A window of 0 opens none: the link is not even polled.
#[test]
fn a_listen_window_of_0_opens_none() {
    let now = Rc::new(Cell::new(0));
    let mut hub = Hub::new(now.clone(), vec![(0, stay(1))]);
    assert!(listen(&mut hub, &mut clock(&now), 0, FEED_EVERY_MS, &mut || {}).is_none());
    assert_eq!(hub.polls, 0);
}

/// Only STAY keeps the boot core: another request in the window gets no
/// answer (the mailbox stays BUSY) and the window runs on.
#[test]
fn a_request_other_than_stay_does_not_keep_the_boot_core() {
    let now = Rc::new(Cell::new(0));
    let mut hub = Hub::new(now.clone(), vec![(ms(2), hello(1))]);
    assert!(listen(&mut hub, &mut clock(&now), 50, FEED_EVERY_MS, &mut || {}).is_none());
    assert_eq!(hub.replies, [None]);
    assert_eq!(now.get(), ms(50));
}

/// In update mode the session answers the window's STAY, a later STAY and
/// HELLO, which tells the update mode.
#[test]
fn update_mode_answers_stay_and_hello_with_its_mode() {
    let f = installed();
    let now = Rc::new(Cell::new(0));
    let mut hub = Hub::new(now.clone(), vec![(ms(10), stay(2)), (ms(20), hello(3))]);
    let mut agent = session(&f);
    let exit = serve(
        &mut hub,
        &mut agent,
        &mut clock(&now),
        FEED_EVERY_MS,
        &mut || {},
        Some(&stay(1)),
    );
    assert_eq!(exit, Exit::Idle);
    let answers: Vec<_> = hub
        .replies
        .iter()
        .map(|r| answer(r.as_ref().unwrap()))
        .collect();
    assert_eq!(answers[0], (Status::Ok, vec![]));
    assert_eq!(answers[1], (Status::Ok, vec![]));
    assert_eq!(answers[2].0, Status::Ok);
    assert_eq!(HelloResp::parse(&answers[2].1).unwrap().mode, mode::UPDATE);
}

/// Update mode ends exactly 60 s after the last request; each request
/// starts the 60 s again. Meanwhile the boot core sleeps between events
/// (a few waits, not a busy poll) and feeds the watchdog at least every
/// `FEED_EVERY_MS`.
#[test]
fn update_mode_ends_60_s_after_the_last_request() {
    assert_eq!(EXIT_IDLE_MS, 60_000);
    let f = installed();
    let now = Rc::new(Cell::new(0));
    let mut hub = Hub::new(now.clone(), vec![(ms(30_000), hello(1))]);
    let mut agent = session(&f);
    let fed = Rc::new(Cell::new(0u64));
    let (mut last_feed, mut longest_gap) = (0u64, 0u64);
    let mut feed = || {
        longest_gap = longest_gap.max(now.get() - last_feed);
        last_feed = now.get();
        fed.set(fed.get() + 1);
    };
    let exit = serve(&mut hub, &mut agent, &mut clock(&now), FEED_EVERY_MS, &mut feed, None);
    assert_eq!(exit, Exit::Idle);
    let ended = now.get();
    assert_eq!(ended, ms(90_000));
    assert_eq!(hub.replies.len(), 1);
    assert!(
        longest_gap <= ms(u64::from(FEED_EVERY_MS)),
        "longest gap between feeds {longest_gap} us"
    );
    let most = 2 * (90_000 / FEED_EVERY_MS as usize) + 10;
    assert!(hub.polls <= most && hub.waits <= most, "{} polls, {} waits", hub.polls, hub.waits);
}

/// REBOOT ends update mode once the hub has read its reply whole.
#[test]
fn reboot_ends_update_mode_once_its_reply_is_read() {
    let f = installed();
    let now = Rc::new(Cell::new(0));
    let reboot = frame(2, Cmd::Reboot, &[reboot_mode::NORMAL]);
    let mut hub = Hub::new(now.clone(), vec![(ms(5), reboot)]);
    let mut agent = session(&f);
    let exit = serve(&mut hub, &mut agent, &mut clock(&now), FEED_EVERY_MS, &mut || {}, None);
    assert_eq!(exit, Exit::Reboot);
    assert_eq!(answer(hub.replies[0].as_ref().unwrap()).0, Status::Ok);
    assert!(now.get() < ms(1_000));
}

/// The host writes an app through update mode (into either APP slot, the
/// confirmed one too) and reboots; the next start tries it. The check of
/// the image ticks the watchdog (U1: a slow check never resets the node).
#[test]
fn a_write_through_update_mode_is_tried_at_the_next_start() {
    for slot in [1u8, 0] {
        let mut f = installed();
        let now = Rc::new(Cell::new(0));
        let mut due: Vec<(u64, Vec<u8>)> = write_requests(slot, &app(slot), 1)
            .into_iter()
            .enumerate()
            .map(|(i, r)| (ms(10 + i as u64), r))
            .collect();
        let at = ms(10 + due.len() as u64);
        due.push((at, frame(250, Cmd::Reboot, &[reboot_mode::NORMAL])));
        let mut hub = Hub::new(now.clone(), due);
        let mut agent = session(&f);
        let ticks = TICKS.load(Ordering::Relaxed);
        let exit = serve(
            &mut hub,
            &mut agent,
            &mut clock(&now),
            FEED_EVERY_MS,
            &mut || {},
            Some(&stay(1)),
        );
        assert_eq!(exit, Exit::Reboot, "slot {slot}");
        for r in &hub.replies {
            assert_eq!(answer(r.as_ref().unwrap()).0, Status::Ok, "slot {slot}");
        }
        assert!(
            TICKS.load(Ordering::Relaxed) > ticks,
            "the check ticks the watchdog"
        );
        let mut log = Log::load(&mut f, &LOG).unwrap();
        let cfg = BootConfig {
            mcu_id: MCU_ID,
            target_tag: target_tag("seg-f401-7seg"),
            max_attempts: TRIAL_ATTEMPTS,
        };
        assert_eq!(
            decide(&mut f, &mut log, &SLOTS, &cfg, &verifier()),
            Decision::Boot {
                slot,
                entry: SLOTS.slots[usize::from(slot)].0 + HEADER_LEN,
                trial: true
            },
            "slot {slot}"
        );
    }
}
