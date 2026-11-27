//! Exhaustive exploration of a node's boot state on the ESP32-S3 (kind V):
//! every sequence of what the host and the node do (an app written through
//! the agent, SET_PENDING, CONFIRM, a reset) up to a bound, with a power
//! loss at any flash operation of any of them, over the production code:
//! the agent (`dagon_agent::Agent`) over the store (`EspStore`) and the
//! otadata sequences the firmware runs (`otadata::select_trial`,
//! `confirm_running`, `withdraw_slot`). The vendor 2nd stage is a model
//! ([`second_stage`]): the highest bootable entry, one trial.
//!
//! States are compared by their flash contents, so each distinct state is
//! expanded once. After every step these hold:
//!
//! - V1: what the 2nd stage starts is whole (its header names its body).
//! - V2: STATUS says what the next reset starts: the pending slot, the
//!   fallback of an app on trial, else the running slot again.
//! - V3: from every state, with no further fault, the host updates the
//!   node to a new app that runs confirmed.
//!
//! The flash operations cut are classified ([`Site`]) and checked against
//! the inventory: each is cut both ways (nothing of it landed, half of it
//! landed). `targets/mcu/stm32f401re/tests/explore.rs` is the same search
//! over a kind S store.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::layout::{region_for, OTADATA, REGIONS, SLOT_FACTORY};
use crate::otadata::{self, raw, Counted, SectorError, Sectors, ENTRY_LEN, SECTOR_LEN};
use crate::store::{header_addr, EspStore, Otadata};
use crate::tests::Feeds;
use crate::tests_factory::{chain, image, keypair, wire, Chain, TAG};
use dagon_agent::Agent;
use dagon_ed25519::Ed25519Compact;
use dagon_format::header::*;
use dagon_format::sha256;
use dagon_port::timeout::HeaderPlace;
use dagon_port::{Flash, FlashError, FlashRegion, SlotStore, StoreError};
use dagon_proto::payload::*;
use dagon_proto::{split_status, Cmd, Frame, Status};

/// Acts per sequence the search follows.
const DEPTH: usize = 5;
/// Body bytes of the images the host writes: two chunks, two write calls
/// each side of a write unit.
const BODY: usize = 1500;
const MAX: usize = 1024;
const PAGE: u32 = 4096;

/// How a power loss met a flash operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Way {
    /// Nothing of it landed.
    None,
    /// Half of it landed.
    Half,
}

const WAYS: [Way; 2] = [Way::None, Way::Half];

/// The part of an image slot a flash operation goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Part {
    /// The detached header's sector, erased first.
    HeaderErase,
    BodyErase,
    Body,
    /// The detached header, written last.
    Header,
}

/// What a flash operation was, by where it went: the inventory the search
/// must cut all of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Site {
    /// An app slot.
    Slot { slot: u8, part: Part },
    /// An otadata sector erased (a withdrawn entry, or before a new one).
    OtaErase { sector: u8 },
    /// An otadata entry programmed (a trial entry, or a confirmed one).
    OtaEntry { sector: u8 },
    /// Anywhere else: the search expects none.
    Outside { addr: u32 },
}

impl Site {
    fn of(addr: u32, erase: bool) -> Site {
        if (OTADATA.offset..OTADATA.offset + 2 * SECTOR_LEN).contains(&addr) {
            let sector = ((addr - OTADATA.offset) / SECTOR_LEN) as u8;
            return if erase {
                Site::OtaErase { sector }
            } else {
                Site::OtaEntry { sector }
            };
        }
        for l in crate::SLOT_LAYOUTS
            .iter()
            .filter(|l| l.target == target::APP)
        {
            let HeaderPlace::Detached { at } = l.header else {
                continue;
            };
            if (at..at + SECTOR_LEN).contains(&addr) {
                let part = if erase {
                    Part::HeaderErase
                } else {
                    Part::Header
                };
                return Site::Slot { slot: l.slot, part };
            }
            if (l.start..l.start + l.capacity).contains(&addr) {
                let part = if erase { Part::BodyErase } else { Part::Body };
                return Site::Slot { slot: l.slot, part };
            }
        }
        Site::Outside { addr }
    }

    fn label(self) -> String {
        match self {
            Site::Slot { slot, part } => format!(
                "APPスロット{}: {}",
                (b'A' + slot) as char,
                match part {
                    Part::HeaderErase => "ヘッダ領域の消去（最初の操作）",
                    Part::BodyErase => "本体の消去",
                    Part::Body => "本体の書込",
                    Part::Header => "ヘッダの書込（最終操作）",
                }
            ),
            Site::OtaErase { sector } => format!("otadata セクター{sector}: 消去"),
            Site::OtaEntry { sector } => format!("otadata セクター{sector}: エントリーの書込"),
            Site::Outside { addr } => format!("範囲外 0x{addr:08X}"),
        }
    }
}

/// The whole flash (the slots, the dagon partition and otadata), only the
/// erase units written to kept, with a power loss: operations from number
/// `cut.0` on do nothing and fail (the one at `cut.0` lands half when the
/// way says so). Or with one failed operation and the power left on
/// (`stay`): operation number `cut.0` alone lands not at all, or half, and
/// answers an error; the ones after it land.
#[derive(Clone)]
struct Cut {
    pages: Rc<RefCell<BTreeMap<u32, Vec<u8>>>>,
    ops: Rc<RefCell<usize>>,
    cut: Option<(usize, Way)>,
    stay: bool,
    /// The site of every erase and write asked for, in order.
    trace: Rc<RefCell<Vec<Site>>>,
}

impl Cut {
    fn new() -> Self {
        Cut {
            pages: Rc::default(),
            ops: Rc::default(),
            cut: None,
            stay: false,
            trace: Rc::default(),
        }
    }

    /// A flash of its own holding what this one holds, powered, cut at
    /// `cut`.
    fn copy(&self, cut: Option<(usize, Way)>) -> Self {
        Cut {
            pages: Rc::new(RefCell::new(self.pages.borrow().clone())),
            ops: Rc::default(),
            cut,
            stay: false,
            trace: Rc::default(),
        }
    }

    /// As [`Cut::copy`], operation `fail.0` failing with the power left on.
    fn failing(&self, fail: (usize, Way)) -> Self {
        Cut {
            stay: true,
            ..self.copy(Some(fail))
        }
    }

    fn powered(&self) -> bool {
        self.stay || self.cut.is_none_or(|(n, _)| *self.ops.borrow() <= n)
    }

    /// Whether the operation the fault is at was asked for.
    fn reached(&self) -> bool {
        self.cut.is_none_or(|(n, _)| *self.ops.borrow() > n)
    }

    /// Counts one operation at `site`: `Ok(true)` it lands whole,
    /// `Ok(false)` half, `Err` not at all.
    fn op(&mut self, site: Site) -> Result<bool, FlashError> {
        self.trace.borrow_mut().push(site);
        let n = *self.ops.borrow();
        *self.ops.borrow_mut() += 1;
        match self.cut {
            Some((c, _)) if n > c && !self.stay => Err(FlashError::Hw),
            Some((c, Way::Half)) if n == c => Ok(false),
            Some((c, Way::None)) if n == c => Err(FlashError::Hw),
            _ => Ok(true),
        }
    }

    fn peek(&self, addr: u32, buf: &mut [u8]) {
        let pages = self.pages.borrow();
        for (a, b) in (addr..).zip(buf.iter_mut()) {
            *b = pages
                .get(&(a / PAGE))
                .map_or(0xFF, |p| p[(a % PAGE) as usize]);
        }
    }

    fn poke(&self, addr: u32, data: &[u8]) {
        let mut pages = self.pages.borrow_mut();
        for (a, d) in (addr..).zip(data) {
            pages
                .entry(a / PAGE)
                .or_insert_with(|| vec![0xFF; PAGE as usize])[(a % PAGE) as usize] = *d;
        }
    }

    fn wipe(&self, addr: u32, len: u32) {
        self.poke(addr, &vec![0xFF; len as usize]);
        self.pages
            .borrow_mut()
            .retain(|_, p| p.iter().any(|&b| b != 0xFF));
    }

    fn erase_at(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let whole = self.op(Site::of(addr, true))?;
        self.wipe(addr, if whole { len } else { len / 2 });
        if whole {
            Ok(())
        } else {
            Err(FlashError::Hw)
        }
    }

    fn write_at(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let mut now = vec![0u8; data.len()];
        self.peek(addr, &mut now);
        if now.iter().any(|&b| b != 0xFF) {
            return Err(FlashError::Hw);
        }
        let whole = self.op(Site::of(addr, false))?;
        let n = if whole {
            data.len()
        } else {
            data.len() / 2 / 4 * 4
        };
        self.poke(addr, &data[..n]);
        if whole {
            Ok(())
        } else {
            Err(FlashError::Hw)
        }
    }

    fn key(&self, running: u8) -> u64 {
        let mut h = DefaultHasher::new();
        (&*self.pages.borrow(), running).hash(&mut h);
        h.finish()
    }
}

impl Flash for Cut {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        region_for(addr, buf.len() as u32).ok_or(FlashError::Range)?;
        self.peek(addr, buf);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        region_for(addr, len).ok_or(FlashError::Range)?;
        if !addr.is_multiple_of(PAGE) || !len.is_multiple_of(PAGE) {
            return Err(FlashError::Align);
        }
        self.erase_at(addr, len)
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        region_for(addr, data.len() as u32).ok_or(FlashError::Range)?;
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        self.write_at(addr, data)
    }
}

/// The otadata partition of the flash, as the firmware's boot selection
/// reaches it.
impl Sectors for Cut {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> Result<(), SectorError> {
        self.peek(OTADATA.offset + at, buf);
        Ok(())
    }
    fn erase(&mut self, at: u32) -> Result<(), SectorError> {
        self.erase_at(OTADATA.offset + at, SECTOR_LEN)
            .map_err(|_| SectorError::Io)
    }
    fn program(&mut self, at: u32, data: &[u8]) -> Result<(), SectorError> {
        self.write_at(OTADATA.offset + at, data)
            .map_err(|_| SectorError::Io)
    }
}

/// The boot selection over the flash's otadata, as `EspBootSelect` runs
/// it: the same sequences, the slot the 2nd stage started.
struct Select {
    f: Cut,
    running: u8,
}

fn io<T>(r: Result<T, SectorError>) -> Result<T, StoreError> {
    r.map_err(|_| StoreError::Flash)
}

impl Otadata for Select {
    fn running(&self) -> u8 {
        self.running
    }
    fn entries(&self) -> Result<[[u8; ENTRY_LEN]; 2], StoreError> {
        io(otadata::read_entries(&mut self.f.clone()))
    }
    fn set_next(&mut self, slot: u8) -> Result<(), StoreError> {
        io(otadata::select_trial(&mut self.f, slot, self.running))
    }
    fn confirm(&mut self) -> Result<(), StoreError> {
        io(otadata::confirm_running(&mut self.f, self.running))
    }
    fn withdraw(&mut self, slot: u8) -> Result<(), StoreError> {
        if slot == self.running {
            return Err(StoreError::Flash);
        }
        io(otadata::withdraw_slot(&mut self.f, slot))
    }
}

type Store = EspStore<Cut, Select, Feeds>;
type NodeAgent = Agent<Store, Chain, Ed25519Compact>;

fn store(f: &Cut, running: u8) -> Store {
    EspStore::new(
        f.clone(),
        Select {
            f: f.clone(),
            running,
        },
        Feeds::default(),
        &crate::ERASE_REGIONS,
    )
}

fn agent(f: &Cut, running: u8) -> NodeAgent {
    let m = if running == SLOT_FACTORY {
        mode::AGENT
    } else {
        mode::APP
    };
    Agent::new(
        crate::config(m, crate::target_tag(TAG)),
        Ed25519Compact::new(*keypair().pk),
        store(f, running),
        chain(),
    )
}

/// The image the host writes into `slot` (one per slot: the states stay
/// few).
fn app(slot: u8) -> Vec<u8> {
    let body: Vec<u8> = (0..BODY).map(|i| (i as u8) ^ (slot + 1)).collect();
    image(slot + 1, &body, &keypair())
}

/// Whether `slot` holds an image whose header names its body.
fn whole(f: &Cut, slot: u8) -> bool {
    let Some(l) = crate::SLOT_LAYOUTS
        .iter()
        .find(|l| (l.target, l.slot) == (target::APP, slot))
    else {
        return false;
    };
    let mut h = [0u8; HEADER_SIZE];
    f.peek(header_addr(target::APP, slot).unwrap(), &mut h);
    let Ok(hdr) = ImageHeader::parse(&h) else {
        return false;
    };
    if hdr.payload_size > l.capacity {
        return false;
    }
    let mut body = vec![0u8; hdr.payload_size as usize];
    f.peek(l.start, &mut body);
    sha256(&body) == hdr.payload_sha256
}

#[derive(Debug)]
struct Broke(String);

fn set_state(f: &Cut, sector: usize, state: u32) {
    let at = OTADATA.offset + sector as u32 * SECTOR_LEN;
    let mut e = [0u8; ENTRY_LEN];
    f.peek(at, &mut e);
    e[24..28].copy_from_slice(&state.to_le_bytes());
    f.wipe(at, SECTOR_LEN);
    f.poke(at, &e);
}

/// The rollback-enabled vendor 2nd stage at a reset: the highest bootable
/// entry; New becomes PendingVerify (the trial), a PendingVerify left over
/// becomes Aborted and the next one is tried; nothing bootable starts
/// factory. Its own writes are not cut (not DAGON's operations).
fn second_stage(f: &Cut) -> Result<u8, Broke> {
    loop {
        let entries = otadata::read_entries(&mut f.copy(None)).unwrap();
        let pick = (0..2)
            .filter_map(|i| Counted::of(&entries[i]).map(|c| (i, c)))
            .filter(|(_, c)| c.bootable())
            .max_by_key(|(_, c)| c.seq);
        let Some((i, c)) = pick else {
            return Ok(SLOT_FACTORY);
        };
        match c.state {
            raw::NEW => set_state(f, i, raw::PENDING_VERIFY),
            raw::PENDING_VERIFY => {
                set_state(f, i, raw::ABORTED);
                continue;
            }
            _ => {}
        }
        if !whole(f, c.slot) {
            return Err(Broke(format!(
                "V1: slot {} starts without its image",
                c.slot
            )));
        }
        return Ok(c.slot);
    }
}

/// One node between two requests: its flash and what runs.
#[derive(Clone)]
struct Node {
    f: Cut,
    running: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// The host writes the slot's image (XFER_BEGIN..XFER_END).
    Transfer(u8),
    SetPending(u8),
    /// CONFIRM of the app on trial.
    Confirm,
    /// A reset.
    Boot,
}

const ACTS: [Act; 6] = [
    Act::Transfer(0),
    Act::Transfer(1),
    Act::SetPending(0),
    Act::SetPending(1),
    Act::Confirm,
    Act::Boot,
];

fn call(a: &mut NodeAgent, seq: &mut u8, cmd: Cmd, payload: &[u8]) -> Status {
    *seq = seq.wrapping_add(1);
    let mut buf = vec![0u8; MAX + 16];
    let n = Frame {
        flags: 0,
        seq: *seq,
        cmd: cmd as u8,
        payload,
    }
    .encode(&mut buf)
    .unwrap();
    let reply = a.handle(&buf[..n]).expect("reply");
    let (f, _) = Frame::decode(reply.frame, MAX).unwrap();
    split_status(f.payload).unwrap().0
}

fn transfer(a: &mut NodeAgent, seq: &mut u8, s: u8) -> Status {
    let img = app(s);
    let begin = XferBegin {
        target: target::APP,
        slot: s,
        total_len: img.len() as u32,
        hdr_len: IMAGE_HDR_LEN,
    };
    let st = call(a, seq, Cmd::XferBegin, &wire(|b| begin.encode(b)));
    if st != Status::Ok {
        return st;
    }
    let (hdr, body) = img.split_at(HEADER_SIZE);
    let size = MAX - XferChunk::OVERHEAD;
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, data) in part.chunks(size).enumerate() {
            let c = XferChunk {
                offset: (i * size) as u32,
                data,
            };
            let st = call(a, seq, cmd, &wire(|b| c.encode(b)));
            if st != Status::Ok {
                return st;
            }
        }
    }
    call(a, seq, Cmd::XferEnd, &[])
}

impl Node {
    fn new() -> Self {
        Node {
            f: Cut::new(),
            running: SLOT_FACTORY,
        }
    }

    fn status(&self) -> StatusResp {
        store(&self.f, self.running).status()
    }

    fn trying(&self) -> bool {
        let st = self.status();
        st.slots
            .get(usize::from(st.active))
            .is_some_and(|s| s.state == SlotState::Trying)
    }

    /// Whether a bootable otadata entry selects the running slot.
    fn backed(&self) -> bool {
        let entries = otadata::read_entries(&mut self.f.copy(None)).unwrap();
        entries
            .iter()
            .filter_map(Counted::of)
            .any(|c| c.bootable() && c.slot == self.running)
    }

    /// Whether the host's own checks let `act` happen.
    fn takes(&self, act: Act) -> bool {
        let st = self.status();
        match act {
            Act::Transfer(s) => s != st.active && s != st.confirmed,
            Act::SetPending(s) => {
                s != st.active
                    && st.slots[usize::from(s)].state != SlotState::Empty
                    && !self.trying()
            }
            // A trial whose otadata entry a failed operation destroyed is
            // not one the host can confirm (CONFIRM is refused; the next
            // reset falls back).
            Act::Confirm => self.trying() && self.backed(),
            Act::Boot => true,
        }
    }

    /// Runs `act` on the node's flash (cut where the flash says). `Ok`
    /// with the status of the host's request; a reset has none.
    fn run(&mut self, act: Act) -> Result<Option<Status>, Broke> {
        let mut seq = 0;
        let mut a = agent(&self.f, self.running);
        Ok(Some(match act {
            Act::Transfer(s) => transfer(&mut a, &mut seq, s),
            Act::SetPending(s) => {
                let p = SetPending {
                    target: target::APP,
                    slot: s,
                };
                call(&mut a, &mut seq, Cmd::SetPending, &wire(|b| p.encode(b)))
            }
            Act::Confirm => call(
                &mut a,
                &mut seq,
                Cmd::Confirm,
                &wire(|b| encode_confirm(target::APP, b)),
            ),
            Act::Boot => {
                self.running = second_stage(&self.f)?;
                return Ok(None);
            }
        }))
    }

    /// V2: STATUS says what the next reset starts.
    fn predicts(&self) -> Result<(), Broke> {
        let st = self.status();
        let pending = (0u8..2).find(|&s| st.slots[usize::from(s)].state == SlotState::Pending);
        let want = match pending {
            Some(s) => s,
            None if self.trying() => st.confirmed,
            None => st.active,
        };
        let next = second_stage(&self.f.copy(None))?;
        if next == want {
            Ok(())
        } else {
            Err(Broke(format!(
                "V2: the next reset starts slot {next}, STATUS says {want}: {st:?}"
            )))
        }
    }

    /// V3: with no further fault, the host updates the node.
    fn settles(&self) -> Result<(), Broke> {
        let mut n = Node {
            f: self.f.copy(None),
            running: self.running,
        };
        if n.trying() {
            n.run(Act::Boot)?;
        }
        let st = n.status();
        let Some(s) = (0u8..2).find(|&s| s != st.active && s != st.confirmed) else {
            return Err(Broke(format!("V3: no slot to update: {st:?}")));
        };
        for act in [
            Act::Transfer(s),
            Act::SetPending(s),
            Act::Boot,
            Act::Confirm,
            Act::Boot,
        ] {
            match n.run(act)? {
                Some(Status::Ok) | None => {}
                Some(st) => return Err(Broke(format!("V3: {act:?} answers {st:?}"))),
            }
        }
        let st = n.status();
        if (n.running, st.confirmed, st.slots[usize::from(s)].state) == (s, s, SlotState::Confirmed)
        {
            Ok(())
        } else {
            Err(Broke(format!(
                "V3: slot {s} does not run confirmed: {st:?}"
            )))
        }
    }

    fn holds(&self) -> Result<(), Broke> {
        if self.running != SLOT_FACTORY && !whole(&self.f, self.running) {
            return Err(Broke(format!(
                "V1: slot {} runs without its image",
                self.running
            )));
        }
        self.predicts()?;
        self.settles()
    }
}

/// The cuts made, by site and way; every one of them recovered.
#[derive(Default)]
struct Coverage(BTreeMap<Site, BTreeMap<Way, usize>>);

impl Coverage {
    fn table(&self) -> String {
        let mut t =
            String::from("| フラッシュ操作 | 未反映・電源断 | 半分が反映・電源断 | 復旧失敗 |\n|---|--:|--:|--:|\n");
        for (s, m) in &self.0 {
            let n = |w| m.get(&w).copied().unwrap_or(0);
            let _ = writeln!(
                t,
                "| {} | {} | {} | 0 |",
                s.label(),
                n(Way::None),
                n(Way::Half)
            );
        }
        t
    }
}

fn inventory() -> Vec<Site> {
    let mut v = Vec::new();
    for slot in 0..2 {
        for part in [Part::HeaderErase, Part::BodyErase, Part::Body, Part::Header] {
            v.push(Site::Slot { slot, part });
        }
    }
    for sector in 0..2 {
        v.push(Site::OtaErase { sector });
        v.push(Site::OtaEntry { sector });
    }
    v
}

/// Every state within [`DEPTH`] acts of `start`, each act also cut at each
/// of its flash operations both ways and followed by the reset the power
/// coming back is; with `stay`, each act instead with one of its flash
/// operations failing both ways and the node left running. Returns the
/// states checked and the faults made.
fn explore(start: Node, stay: bool) -> (usize, Coverage) {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    let mut cover = Coverage::default();
    seen.insert(start.f.key(start.running));
    queue.push_back((start, Vec::<(Act, Option<(usize, Way)>)>::new()));
    let mut checked = 0;
    while let Some((node, path)) = queue.pop_front() {
        if let Err(Broke(why)) = node.holds() {
            panic!("{why}\nafter {path:?}");
        }
        checked += 1;
        if path.len() == DEPTH {
            continue;
        }
        for act in ACTS {
            if !node.takes(act) {
                continue;
            }
            // Uncut first: its operations are the ones to cut.
            let mut whole_run = Node {
                f: node.f.copy(None),
                running: node.running,
            };
            let step = |n: &mut Node, path: &[(Act, Option<(usize, Way)>)]| {
                n.run(act)
                    .unwrap_or_else(|Broke(why)| panic!("{why}\nafter {path:?}"))
            };
            let mut next = path.clone();
            next.push((act, None));
            let st = step(&mut whole_run, &next);
            assert!(
                matches!(st, Some(Status::Ok) | None),
                "{act:?} answers {st:?} after {path:?}"
            );
            let sites = whole_run.f.trace.borrow().clone();
            let mut runs = vec![(whole_run, next)];
            for (n, site) in sites.iter().enumerate() {
                for way in WAYS {
                    let f = if stay {
                        node.f.failing((n, way))
                    } else {
                        node.f.copy(Some((n, way)))
                    };
                    let mut cut = Node {
                        f,
                        running: node.running,
                    };
                    let mut next = path.clone();
                    next.push((act, Some((n, way))));
                    step(&mut cut, &next);
                    assert!(cut.f.reached(), "operation {n} of {act:?} not reached");
                    assert_eq!(cut.f.powered(), stay, "{act:?} cut at {n}");
                    cut.f = cut.f.copy(None);
                    if !stay {
                        // The power comes back: a reset on what the flash holds.
                        cut.running = second_stage(&cut.f)
                            .unwrap_or_else(|Broke(why)| panic!("{why}\nafter {next:?}"));
                    }
                    *cover.0.entry(*site).or_default().entry(way).or_default() += 1;
                    runs.push((cut, next));
                }
            }
            for (n, p) in runs {
                if seen.insert(n.f.key(n.running)) {
                    queue.push_back((
                        Node {
                            f: n.f.copy(None),
                            running: n.running,
                        },
                        p,
                    ));
                }
            }
        }
    }
    (checked, cover)
}

/// A node whose first app runs confirmed from slot A.
fn installed() -> Node {
    let mut n = Node::new();
    for act in [
        Act::Transfer(0),
        Act::SetPending(0),
        Act::Boot,
        Act::Confirm,
        Act::Boot,
    ] {
        assert!(n.takes(act), "{act:?} refused");
        let st = n.run(act).unwrap_or_else(|Broke(why)| panic!("{why}"));
        assert!(matches!(st, Some(Status::Ok) | None), "{act:?}: {st:?}");
    }
    assert_eq!(n.running, 0);
    Node {
        f: n.f.copy(None),
        running: n.running,
    }
}

/// From the factory agent over blank otadata (the first install) and from
/// a confirmed app (an update, the fallback kept).
#[test]
fn every_state_of_a_node_boots_a_whole_app_and_takes_an_update() {
    let mut cover = Coverage::default();
    let mut checked = 0;
    for start in [Node::new(), installed()] {
        let (n, c) = explore(start, false);
        checked += n;
        for (s, m) in c.0 {
            for (w, k) in m {
                *cover.0.entry(s).or_default().entry(w).or_default() += k;
            }
        }
    }
    let cuts: usize = cover.0.values().flat_map(|m| m.values()).sum();
    eprintln!("{checked} states, {cuts} cuts");
    let inventory = inventory();
    let mut wrong = Vec::new();
    for s in &inventory {
        for w in WAYS {
            if cover.0.get(s).and_then(|m| m.get(&w)).is_none() {
                wrong.push(format!("not cut ({w:?}): {}", s.label()));
            }
        }
    }
    for s in cover.0.keys().filter(|s| !inventory.contains(s)) {
        wrong.push(format!("cut but not in the inventory: {}", s.label()));
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
    if let Ok(dir) = std::env::var("DAGON_POWERCUT_REPORT") {
        let body = format!(
            "# ESP32-S3: 全フラッシュ操作の電断\n\n{checked} 状態、電断 {cuts} 件、復旧失敗 0 件。\n\n{}",
            cover.table()
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/esp32s3.md"), body).unwrap();
    }
}

/// A flash operation that fails with the power left on (the part answers
/// an error, nothing or half of the operation landed): any one operation
/// of any act, the node left running what it ran. V1 to V3 hold from
/// there, as after a power loss.
#[test]
fn a_failed_flash_operation_leaves_a_node_that_takes_an_update() {
    let (mut checked, mut fails) = (0, 0);
    for start in [Node::new(), installed()] {
        let (n, c) = explore(start, true);
        checked += n;
        fails += c.0.values().flat_map(|m| m.values()).sum::<usize>();
    }
    eprintln!("{checked} states, {fails} failed operations");
    assert!(fails > 0);
}

/// The otadata entry of an app on trial is lost to a failed erase while
/// the host confirms it: the app keeps running, STATUS still says it is on
/// trial with factory as what the next reset starts, a repeated CONFIRM is
/// refused, and the reset starts factory.
#[test]
fn a_trial_whose_entry_a_failed_erase_destroyed_is_not_reported_confirmed() {
    let mut n = Node::new();
    for act in [Act::Transfer(0), Act::SetPending(0), Act::Boot] {
        assert!(matches!(n.run(act), Ok(Some(Status::Ok) | None)), "{act:?}");
    }
    assert!(n.trying() && n.backed());
    n.f = n.f.failing((0, Way::Half));
    assert!(matches!(n.run(Act::Confirm), Ok(Some(st)) if st != Status::Ok));
    n.f = n.f.copy(None);
    let st = n.status();
    assert_eq!((st.active, st.confirmed), (0, SLOT_FACTORY));
    assert_eq!(st.slots[0].state, SlotState::Trying);
    assert!(matches!(n.run(Act::Confirm), Ok(Some(st)) if st != Status::Ok));
    n.holds().unwrap_or_else(|Broke(why)| panic!("{why}"));
    n.run(Act::Boot)
        .unwrap_or_else(|Broke(why)| panic!("{why}"));
    assert_eq!(n.running, SLOT_FACTORY);
}
