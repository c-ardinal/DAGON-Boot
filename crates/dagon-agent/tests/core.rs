//! Boot core slots through the agent: XFER and SET_PENDING with target
//! BOOTCORE into the idle boot core slot, STATUS of the boot core slots, the
//! boot core image machine's events, and the refusals (a running or
//! confirmed core slot, a board without boot core slots, an image that is
//! not a verifying boot core, a board that keeps no boot core slots).

use std::cell::RefCell;
use std::rc::Rc;

use dagon_agent::{core_image, Agent, Config, SlotStore, StoreError, Transition};
use dagon_core::log::{Log, LogLayout, SlotRef};
use dagon_ed25519::Ed25519Compact;
use dagon_events::{
    compose, Bounds, Composed, Ctx, Event, Extension, Layer, Nop, Published, Watchdog,
};
use dagon_format::header::*;
use dagon_format::statelog::{kind, FaceHeader};
use dagon_format::{key_id, sha256, Version};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::*;
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const TAG: [u8; 16] = target_tag("hub-rp2040-lcd");
/// Two boot core slots of 4 KiB (header inline) and the boot core log.
const CORE: [u32; 2] = [0x0000, 0x1000];
const CORE_LEN: u32 = 0x1000;
/// The stage0 version mark the store reads.
const STAGE0: Version = Version::new(0, 1, 0);
const LOG: LogLayout = LogLayout {
    faces: [0x2000, 0x2400],
    face_len: 0x400,
};

fn kp() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn core_image(slot: u8, minor: u8, kind: u8, flags: u32, caps: u32) -> Vec<u8> {
    let body: Vec<u8> = (0..900u32).map(|i| (i as u8) ^ minor).collect();
    let mut h = ImageHeader {
        image_kind: kind,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::RP2040,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: CORE[slot as usize] + HEADER_SIZE as u32,
        payload_size: body.len() as u32,
        version: Version::new(1, minor, 0),
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::default(),
        payload_sha256: sha256(&body),
        key_id: key_id(&kp().pk),
        target_tag: TAG,
        proto_version: 1,
        capabilities: caps,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp().sk.sign(msg, None);
    [&h.to_bytes()[..], &body].concat()
}

fn bootcore(slot: u8, minor: u8) -> Vec<u8> {
    core_image(
        slot,
        minor,
        image_kind::BOOTCORE,
        build_flags::VERIFY_ENFORCED,
        capabilities::SIG_VERIFIER,
    )
}

struct Mem(Vec<u8>);

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        buf.copy_from_slice(&self.0[addr as usize..][..buf.len()]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.0[addr as usize..][..len as usize].fill(0xFF);
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.0[addr as usize..][..data.len()].copy_from_slice(data);
        Ok(())
    }
}

/// An app confirmed in slot A (no APP transfers here) and two boot core
/// slots behind the boot core log, as a stage0 MCU keeps them. `cores`
/// false: a board without boot core slots (the trait's defaults).
struct Store {
    flash: Mem,
    cores: bool,
    /// How many of the boot core slots the board keeps (A first).
    core_slots: u8,
    /// The app in slot A runs on trial.
    app_trial: bool,
    /// The boot core slot that started this app.
    running: u8,
}

impl Store {
    fn head(&mut self, slot: u8) -> Option<SlotRef> {
        let mut raw = [0u8; HEADER_SIZE];
        self.flash.read(CORE[slot as usize], &mut raw).ok()?;
        let h = ImageHeader::parse(&raw).ok()?;
        Some(SlotRef {
            slot,
            version: h.version,
            digest16: header_digest16(&raw),
        })
    }
    fn log(&mut self) -> Log {
        Log::load(&mut self.flash, &LOG).expect("log")
    }
}

fn app_status(trial: bool) -> StatusResp {
    let mut st = StatusResp {
        active: 0,
        confirmed: 0,
        ..Default::default()
    };
    st.slots[0].state = if trial {
        SlotState::Trying
    } else {
        SlotState::Confirmed
    };
    st
}

impl SlotStore for Store {
    fn status(&mut self) -> StatusResp {
        app_status(self.app_trial)
    }
    fn capacity(&self, t: u8, slot: u8) -> u32 {
        if t == target::BOOTCORE && self.cores && slot < self.core_slots {
            CORE_LEN - HEADER_SIZE as u32
        } else {
            0
        }
    }
    fn erase(&mut self, t: u8, slot: u8, _: u32, _: &Transition) -> Result<(), StoreError> {
        assert_eq!(t, target::BOOTCORE);
        self.flash.erase(CORE[slot as usize], CORE_LEN).unwrap();
        Ok(())
    }
    fn write(
        &mut self,
        _: u8,
        slot: u8,
        o: u32,
        d: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let at = CORE[slot as usize] + HEADER_SIZE as u32 + o;
        self.flash.write(at, d).map_err(|_| StoreError::Flash)
    }
    fn read(&mut self, _: u8, slot: u8, o: u32, b: &mut [u8]) -> Result<(), StoreError> {
        let at = CORE[slot as usize] + HEADER_SIZE as u32 + o;
        self.flash.read(at, b).map_err(|_| StoreError::Flash)
    }
    fn write_header(
        &mut self,
        _: u8,
        _: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        unreachable!("no APP transfer here")
    }
    fn read_header(&mut self, _: u8, _: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        Err(StoreError::Flash)
    }
    fn set_pending(
        &mut self,
        _: u8,
        _: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        unreachable!("no APP switch here")
    }
    fn confirm(&mut self, _: u8, _: &[u8; HEADER_SIZE], _: &Transition) -> Result<(), StoreError> {
        unreachable!("no APP confirm here")
    }
    /// A board with boot core slots boots behind a marked stage0.
    fn stage0_version(&mut self) -> Option<Version> {
        self.cores.then_some(STAGE0)
    }
    fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        match t {
            target::APP => Some(app_status(self.app_trial)),
            target::BOOTCORE if self.cores => {
                let log = self.log();
                let heads = [self.head(0), self.head(1)];
                let heads = &heads[..usize::from(self.core_slots)];
                Some(log.core.status(heads, self.running, log.generation))
            }
            _ => None,
        }
    }
    fn write_target_header(
        &mut self,
        t: u8,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        assert_eq!(t, target::BOOTCORE);
        self.flash
            .write(CORE[slot as usize], h)
            .map_err(|_| StoreError::Flash)
    }
    fn read_target_header(
        &mut self,
        t: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        assert_eq!(t, target::BOOTCORE);
        self.flash
            .read(CORE[slot as usize], out)
            .map_err(|_| StoreError::Flash)
    }
    fn set_target_pending(
        &mut self,
        t: u8,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        assert_eq!(t, target::BOOTCORE);
        let at = SlotRef {
            slot,
            version: ImageHeader::parse(h).unwrap().version,
            digest16: header_digest16(h),
        };
        let mut log = self.log();
        log.append_core(&mut self.flash, kind::SLOT_PENDING, at, 0)
            .map_err(|_| StoreError::Flash)
    }
}

/// Flash with boot core A installed and recorded (PROVISION, CONFIRMED).
fn installed(cores: bool) -> Store {
    let mut s = Store {
        flash: Mem(vec![0xFF; 0x2800]),
        cores,
        core_slots: 2,
        app_trial: false,
        running: 0,
    };
    let a = bootcore(0, 0);
    s.flash.write(CORE[0], &a).unwrap();
    s.flash
        .write(LOG.faces[0], &FaceHeader { generation: 1 }.to_bytes())
        .unwrap();
    let r = s.head(0).unwrap();
    let mut log = s.log();
    log.append_core(&mut s.flash, kind::PROVISION, r, 0)
        .unwrap();
    log.append_core(&mut s.flash, kind::CONFIRMED, r, 0)
        .unwrap();
    s
}

#[derive(Clone, Default)]
struct Probe(Rc<RefCell<Vec<String>>>);

impl Extension for Probe {
    fn id(&self) -> &'static str {
        "probe"
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        let k = if e.is_replayed() {
            format!("replay:{}", kind_name(e))
        } else {
            kind_name(e).to_string()
        };
        self.0.borrow_mut().push(k);
    }
}

type Chain = Composed<&'static Published<(), 32>, Nop, Probe, fn() -> u64>;

fn now() -> u64 {
    0
}

fn config() -> Config {
    Config {
        mcu_id: mcu_id::RP2040,
        port_kind: port_kind::SELF_BOOT,
        target_tag: TAG,
        role: role::HUB,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        trial_attempts: 3,
    }
}

fn verifier() -> Ed25519Compact {
    Ed25519Compact::new(*kp().pk)
}

struct Node {
    store: &'static RefCell<Store>,
    agent: Agent<&'static RefCell<Store>, Chain, Ed25519Compact>,
    seen: Probe,
    seq: u8,
}

impl Node {
    fn new(store: Store) -> Self {
        let store: &'static RefCell<Store> = Box::leak(Box::new(RefCell::new(store)));
        let seen = Probe::default();
        Node {
            store,
            agent: Agent::new(config(), verifier(), store, Self::chain(&seen)),
            seen,
            seq: 0,
        }
    }

    fn chain(seen: &Probe) -> Chain {
        let events: &'static mut Published<(), 32> = Box::leak(Box::new(Published::new()));
        compose(
            Layer::App,
            events,
            Nop,
            seen.clone(),
            Bounds::new(now as fn() -> u64, Watchdog::new(4000, 8000)),
        )
    }

    /// A reset after `boot` (stage0 and the boot core) changed the flash.
    fn reset(&mut self, boot: impl FnOnce(&mut Store)) {
        boot(&mut self.store.borrow_mut());
        self.seen.0.borrow_mut().clear();
        self.agent = Agent::new(config(), verifier(), self.store, Self::chain(&self.seen));
    }

    fn events(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in std::mem::take(&mut *self.seen.0.borrow_mut()) {
            if out.last() != Some(&e) {
                out.push(e);
            }
        }
        out
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        self.seq = self.seq.wrapping_add(1);
        let mut buf = vec![0u8; 1200];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = self.agent.handle(&buf[..n]).expect("reply");
        let (f, _) = Frame::decode(reply.frame, 1200).unwrap();
        assert_eq!(f.flags, flags::RESP);
        let (st, body) = split_status(f.payload).unwrap();
        (st, body.to_vec())
    }

    fn transfer(&mut self, t: u8, slot: u8, img: &[u8]) -> Status {
        let mut p = [0u8; 8];
        XferBegin {
            target: t,
            slot,
            total_len: img.len() as u32,
            hdr_len: IMAGE_HDR_LEN,
        }
        .encode(&mut p)
        .unwrap();
        let st = self.call(Cmd::XferBegin, &p).0;
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, c) in part.chunks(256).enumerate() {
                let mut b = vec![0u8; 4 + c.len()];
                XferChunk {
                    offset: (i * 256) as u32,
                    data: c,
                }
                .encode(&mut b)
                .unwrap();
                let st = self.call(cmd, &b).0;
                if st != Status::Ok {
                    return st;
                }
            }
        }
        self.call(Cmd::XferEnd, &[]).0
    }

    fn core_status(&mut self) -> (Status, Option<StatusResp>) {
        let (st, body) = self.call(Cmd::Status, &[target::BOOTCORE]);
        (st, StatusResp::parse(&body).ok())
    }
}

#[test]
fn a_new_boot_core_goes_to_the_idle_slot_and_is_recorded_pending() {
    let mut n = Node::new(installed(true));
    n.events();
    let (st, s) = n.core_status();
    let s = s.unwrap();
    assert_eq!(st, Status::Ok);
    assert_eq!((s.active, s.confirmed), (0, 0));
    assert_eq!(s.slots[0].state, SlotState::Confirmed);
    assert_eq!(s.slots[0].version, Version::new(1, 0, 0));
    assert_eq!(s.slots[1].state, SlotState::Empty);

    let b = bootcore(1, 1);
    assert_eq!(n.transfer(target::BOOTCORE, 1, &b), Status::Ok);
    let ev = n.events();
    for k in [
        "update_offered",
        "header_verified",
        "core_transfer_started",
        "verify_ok",
        "core_written",
    ] {
        assert!(ev.contains(&k.to_string()), "{k} in {ev:?}");
    }
    assert_eq!(n.agent.core_state(1), Some(core_image::State::Written));
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 1]).0,
        Status::Ok
    );
    assert!(n.events().contains(&"core_pending_set".to_string()));
    let s = n.core_status().1.unwrap();
    assert_eq!(s.slots[1].state, SlotState::Pending);
    assert_eq!(s.slots[1].version, Version::new(1, 1, 0));
    let log = n.store.borrow_mut().log();
    assert_eq!(log.core.pending.map(|p| p.slot), Some(1));
    assert_eq!(log.state.pending, None, "the app slots are untouched");
}

#[test]
fn the_new_cores_own_confirmation_and_a_rollback_are_seen_after_the_reset() {
    let mut n = Node::new(installed(true));
    assert_eq!(n.transfer(target::BOOTCORE, 1, &bootcore(1, 1)), Status::Ok);
    n.call(Cmd::SetPending, &[target::BOOTCORE, 1]);
    // The new core confirmed itself (dagon_core::bootcore::settle).
    n.reset(|s| {
        let p = s.log().core.pending.unwrap();
        let mut log = s.log();
        log.append_core(&mut s.flash, kind::CONFIRMED, p, 0)
            .unwrap();
        s.running = 1;
    });
    n.events();
    let s = n.core_status().1.unwrap();
    assert_eq!((s.active, s.confirmed), (1, 1));
    assert_eq!(
        (s.slots[0].state, s.slots[1].state),
        (SlotState::Written, SlotState::Confirmed)
    );
    assert_eq!(n.agent.core_state(1), Some(core_image::State::Confirmed));

    // Another update that stage0 gives up on: the old core records ROLLBACK.
    assert_eq!(n.transfer(target::BOOTCORE, 0, &bootcore(0, 2)), Status::Ok);
    n.call(Cmd::SetPending, &[target::BOOTCORE, 0]);
    n.reset(|s| {
        let p = s.log().core.pending.unwrap();
        let mut log = s.log();
        let back = SlotRef { slot: 1, ..p };
        log.append_core(&mut s.flash, kind::ROLLBACK, back, 3)
            .unwrap();
    });
    assert!(
        n.events().contains(&"replay:core_rolled_back".to_string()),
        "the rollback the boot stage made is replayed"
    );
    let s = n.core_status().1.unwrap();
    assert_eq!(s.slots[0].state, SlotState::Reverted);
    assert_eq!((s.confirmed, s.last_reason), (1, 3));
}

#[test]
fn the_running_and_the_confirmed_core_slots_are_never_written() {
    let mut n = Node::new(installed(true));
    assert_eq!(
        n.transfer(target::BOOTCORE, 0, &bootcore(0, 1)),
        Status::State
    );
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 0]).0,
        Status::State
    );
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 1]).0,
        Status::State,
        "an empty slot is not set pending"
    );
    assert_eq!(
        n.call(Cmd::Confirm, &[target::BOOTCORE]).0,
        Status::Arg,
        "a boot core confirms itself"
    );
}

/// With one boot core slot the app's agent writes that slot, the one the
/// running core came from: the app runs from an APP slot, not from it.
/// The new core is tried and, confirmed, is the only core again.
#[test]
fn the_only_boot_core_slot_is_written_by_the_app() {
    let mut n = Node::new(Store {
        core_slots: 1,
        ..installed(true)
    });
    let s = n.core_status().1.unwrap();
    assert_eq!(
        (s.active, s.confirmed, s.slots[0].state),
        (0, 0, SlotState::Confirmed)
    );
    assert_eq!(
        n.transfer(target::BOOTCORE, 1, &bootcore(1, 1)),
        Status::Unsupported,
        "no slot B"
    );
    assert_eq!(n.transfer(target::BOOTCORE, 0, &bootcore(0, 1)), Status::Ok);
    assert_eq!(n.agent.core_state(0), Some(core_image::State::Written));
    // The confirmed core is gone from its slot: no core version runs.
    assert_eq!(version(&mut n).boot_core, None);
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 0]).0,
        Status::Ok
    );
    let s = n.core_status().1.unwrap();
    assert_eq!(
        (s.slots[0].state, s.slots[0].version),
        (SlotState::Pending, Version::new(1, 1, 0))
    );
    // The new core confirmed itself after stage0 tried it.
    n.reset(|s| {
        let p = s.log().core.pending.unwrap();
        let mut log = s.log();
        log.append_core(&mut s.flash, kind::CONFIRMED, p, 0)
            .unwrap();
    });
    let s = n.core_status().1.unwrap();
    assert_eq!(
        (s.confirmed, s.slots[0].state, s.slots[0].version),
        (0, SlotState::Confirmed, Version::new(1, 1, 0))
    );
    // And it can be written again.
    assert_eq!(n.transfer(target::BOOTCORE, 0, &bootcore(0, 2)), Status::Ok);
}

/// With one boot core slot, neither a transfer into it nor its pending
/// setting is taken while the running app is on trial (stage0 starts no
/// app then, so a cut write would leave nothing to start); with two, the
/// idle slot is written as before.
#[test]
fn the_only_boot_core_slot_is_left_alone_while_the_app_is_on_trial() {
    let mut n = Node::new(Store {
        core_slots: 1,
        app_trial: true,
        ..installed(true)
    });
    assert_eq!(
        n.transfer(target::BOOTCORE, 0, &bootcore(0, 1)),
        Status::State
    );
    assert_eq!(n.store.borrow_mut().log().core.pending, None);

    // Written before the trial, it is not set pending during it.
    let mut n = Node::new(Store {
        core_slots: 1,
        ..installed(true)
    });
    assert_eq!(n.transfer(target::BOOTCORE, 0, &bootcore(0, 1)), Status::Ok);
    n.reset(|s| s.app_trial = true);
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 0]).0,
        Status::State
    );
    // Confirmed (off trial), it is.
    n.reset(|s| s.app_trial = false);
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 0]).0,
        Status::Ok
    );

    let mut n = Node::new(Store {
        app_trial: true,
        ..installed(true)
    });
    assert_eq!(n.transfer(target::BOOTCORE, 1, &bootcore(1, 1)), Status::Ok);
}

#[test]
fn only_a_verifying_boot_core_for_this_node_is_taken() {
    let mut n = Node::new(installed(true));
    let unverifying = core_image(1, 1, image_kind::BOOTCORE, 0, capabilities::SIG_VERIFIER);
    assert_eq!(
        n.transfer(target::BOOTCORE, 1, &unverifying),
        Status::Compat
    );
    let sim_only = core_image(
        1,
        1,
        image_kind::BOOTCORE,
        build_flags::VERIFY_ENFORCED | build_flags::SIM_ONLY,
        capabilities::SIG_VERIFIER,
    );
    assert_eq!(n.transfer(target::BOOTCORE, 1, &sim_only), Status::Compat);
    let app = core_image(1, 1, image_kind::APP, 0, 0);
    assert_eq!(n.transfer(target::BOOTCORE, 1, &app), Status::Compat);
    let mut tampered = bootcore(1, 1);
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert_eq!(n.transfer(target::BOOTCORE, 1, &tampered), Status::Digest);
    assert_eq!(n.core_status().1.unwrap().slots[1].state, SlotState::Empty);
}

/// An MCU without boot core slots in its definition (the ESP32-S3 boots
/// through the vendor's 2nd stage): its store keeps none, and the agent
/// says so (`Unsupported`, reported as `app.update_refused`).
#[test]
fn a_board_without_boot_core_slots_refuses_with_the_reason() {
    let mut n = Node::new(installed(false));
    n.events();
    assert_eq!(n.core_status(), (Status::Unsupported, None));
    assert_eq!(
        n.transfer(target::BOOTCORE, 1, &bootcore(1, 1)),
        Status::Unsupported
    );
    assert!(n.events().contains(&"update_refused".to_string()));
    assert_eq!(
        n.call(Cmd::SetPending, &[target::BOOTCORE, 1]).0,
        Status::Unsupported
    );
}

#[test]
fn status_of_an_unknown_target_or_shape_is_refused() {
    let mut n = Node::new(installed(true));
    assert_eq!(n.call(Cmd::Status, &[9]).0, Status::Unsupported);
    assert_eq!(n.call(Cmd::Status, &[1, 1]).0, Status::Arg);
    assert_eq!(n.call(Cmd::Status, &[]).0, Status::Ok, "APP as before");
}

fn version(n: &mut Node) -> VersionResp {
    let (st, body) = n.call(Cmd::Version, &[]);
    assert_eq!(st, Status::Ok);
    VersionResp::parse(&body).unwrap()
}

#[test]
fn version_names_stage0_the_boot_core_slots_and_the_app() {
    let mut n = Node::new(installed(true));
    let v = version(&mut n);
    assert_eq!((v.wire, v.mcu_id), (Some(1), Some(mcu_id::RP2040)));
    assert_eq!(v.stage0, Some(STAGE0));
    let core = v.core_slots.expect("boot core slots");
    assert_eq!(core.active, 0);
    assert_eq!(v.boot_core, Some(Version::new(1, 0, 0)));
    assert_eq!(core.slots[1].state, SlotState::Empty);
    assert_eq!(v.app_slots, Some(VersionSlots::of(&app_status(false))));
    assert_eq!(
        n.call(Cmd::Version, &[0x21, 0, 0, 8]).0,
        Status::Unsupported
    );

    // A new boot core written to B shows there, A still running.
    assert_eq!(n.transfer(target::BOOTCORE, 1, &bootcore(1, 1)), Status::Ok);
    let core = version(&mut n).core_slots.unwrap();
    assert_eq!(core.slots[1].version, Version::new(1, 1, 0));
    assert_eq!(core.active, 0);
}

#[test]
fn version_leaves_out_what_the_node_cannot_read() {
    // No stage0 mark and no boot core slots: neither is sent.
    let mut n = Node::new(installed(false));
    let v = version(&mut n);
    assert_eq!((v.stage0, v.boot_core, v.core_slots), (None, None, None));
    assert!(v.app_slots.is_some() && v.mcu_id.is_some());
}

/// The kind's name as a log display shows it, looked up from its code.
fn kind_name(e: &dagon_events::Event<'_>) -> &'static str {
    [&dagon_agent::MACHINES, &dagon_events::MACHINES]
        .iter()
        .find_map(|m| m.entry(e.layer(), e.kind()))
        .map_or("?", |x| x.kind)
}
