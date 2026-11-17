//! One update session for every writer: the app's agent and the boot core
//! run the same engine with their drivers injected, give the same verdict
//! on every image, and differ only in the slot each one runs from.

use std::cell::RefCell;
use std::collections::BTreeMap;

use dagon_agent::writer::{self, Writer};
use dagon_agent::{Agent, Config, SlotStore, StoreError, Transition};
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::{key_id, sha256, SigVerifier, Version};
use dagon_proto::payload::*;
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

/// A node of an MCU that only this file knows: its id, tag and slot size.
const FAKE_MCU: u16 = 0x7E57;
const TAG: [u8; 16] = target_tag("fake-node");
const CAP: u32 = 4096;
const BODY: usize = 1000;
/// The XFER target number the topology copy had: reserved, taken by no
/// writer.
#[allow(deprecated)]
const RESERVED_TARGET: u8 = target::TOPOLOGY;

type Chain = Composed<&'static Published<(), 16>, Nop, Nop, fn() -> u64>;

fn now() -> u64 {
    0
}

/// The app layer's chain with no extension: what the boot core carries.
fn chain() -> Chain {
    let events: &'static mut Published<(), 16> = Box::leak(Box::new(Published::new()));
    compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now as fn() -> u64, Watchdog::new(4000, 8000)),
    )
}

fn config() -> Config {
    Config {
        mcu_id: FAKE_MCU,
        port_kind: port_kind::SELF_BOOT,
        target_tag: TAG,
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: 3,
    }
}

/// The flash of the fake MCU: image slots of APP and BOOTCORE, each with
/// an inline header, and the STATUS its boot stage would fold.
struct Mem {
    /// Slots each target has (APP, BOOTCORE).
    count: [u8; 2],
    status: [StatusResp; 2],
    bodies: BTreeMap<(u8, u8), Vec<u8>>,
    headers: BTreeMap<(u8, u8), [u8; HEADER_SIZE]>,
    /// Times the session asked the store to make log room.
    rooms: usize,
    /// Room for the reserved target 3 at slot 0, as a store built before
    /// that target was removed still reports.
    reserved_room: bool,
    /// Its boot core has an update mode: a stay can be written for it.
    update_mode: bool,
    /// Stays written for the boot core (UPDATE_STAY).
    stays: usize,
    /// Making log room fails (a flash that does not erase).
    room_fails: bool,
}

impl Mem {
    /// `apps` APP slots and `cores` boot core slots; slot A of each holds
    /// the confirmed image, and runs.
    fn new(apps: u8, cores: u8) -> Self {
        let mut status = [StatusResp::default(); 2];
        for st in &mut status {
            st.slots[0].state = SlotState::Confirmed;
            st.slots[0].version = Version::new(1, 0, 0);
        }
        Mem {
            count: [apps, cores],
            status,
            bodies: BTreeMap::new(),
            headers: BTreeMap::new(),
            rooms: 0,
            reserved_room: false,
            update_mode: true,
            stays: 0,
            room_fails: false,
        }
    }

    fn st(&mut self, t: u8) -> Result<&mut StatusResp, StoreError> {
        self.status
            .get_mut(usize::from(t))
            .ok_or(StoreError::Unsupported)
    }
}

impl SlotStore for Mem {
    fn status(&mut self) -> StatusResp {
        self.status[0]
    }
    fn capacity(&self, t: u8, slot: u8) -> u32 {
        let has = self.count.get(usize::from(t)).is_some_and(|&n| slot < n)
            || (self.reserved_room && (t, slot) == (RESERVED_TARGET, 0));
        if has {
            CAP
        } else {
            0
        }
    }
    fn erase(&mut self, t: u8, slot: u8, _len: u32, _: &Transition) -> Result<(), StoreError> {
        self.bodies.insert((t, slot), vec![0xFF; CAP as usize]);
        self.headers.remove(&(t, slot));
        self.st(t)?.slots[usize::from(slot)].state = SlotState::Empty;
        Ok(())
    }
    fn write(
        &mut self,
        t: u8,
        slot: u8,
        off: u32,
        data: &[u8],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let body = self.bodies.get_mut(&(t, slot)).ok_or(StoreError::Flash)?;
        body[off as usize..][..data.len()].copy_from_slice(data);
        Ok(())
    }
    fn read(&mut self, t: u8, slot: u8, off: u32, buf: &mut [u8]) -> Result<(), StoreError> {
        let body = self.bodies.get(&(t, slot)).ok_or(StoreError::Flash)?;
        buf.copy_from_slice(&body[off as usize..][..buf.len()]);
        Ok(())
    }
    fn write_header(
        &mut self,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.write_target_header(target::APP, slot, h, t)
    }
    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.read_target_header(target::APP, slot, out)
    }
    fn set_pending(
        &mut self,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.set_target_pending(target::APP, slot, h, t)
    }
    fn confirm(
        &mut self,
        slot: u8,
        _: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        let st = self.st(target::APP)?;
        st.slots[usize::from(slot)].state = SlotState::Confirmed;
        st.confirmed = slot;
        Ok(())
    }
    fn keep_room(&mut self) -> Result<(), StoreError> {
        if self.room_fails {
            return Err(StoreError::Flash);
        }
        self.rooms += 1;
        Ok(())
    }
    fn request_update_mode(&mut self) -> Result<(), StoreError> {
        if !self.update_mode {
            return Err(StoreError::Unsupported);
        }
        self.stays += 1;
        Ok(())
    }
    fn target_status(&mut self, t: u8) -> Option<StatusResp> {
        let n = *self.count.get(usize::from(t))?;
        (n > 0).then(|| self.status[usize::from(t)])
    }
    fn write_target_header(
        &mut self,
        t: u8,
        slot: u8,
        h: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.headers.insert((t, slot), *h);
        let info = &mut self.st(t)?.slots[usize::from(slot)];
        info.state = SlotState::Written;
        info.version = ImageHeader::parse(h).map_or(Version::default(), |h| h.version);
        Ok(())
    }
    fn read_target_header(
        &mut self,
        t: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        *out = self
            .headers
            .get(&(t, slot))
            .copied()
            .unwrap_or([0xFF; HEADER_SIZE]);
        Ok(())
    }
    fn set_target_pending(
        &mut self,
        t: u8,
        slot: u8,
        _: &[u8; HEADER_SIZE],
        _: &Transition,
    ) -> Result<(), StoreError> {
        self.st(t)?.slots[usize::from(slot)].state = SlotState::Pending;
        Ok(())
    }
}

/// How an image is signed, as the verifier the node is built with checks.
trait Signer {
    fn key_id(&self) -> [u8; 8];
    fn sign(&self, msg: &[u8]) -> [u8; 64];
}

impl Signer for KeyPair {
    fn key_id(&self) -> [u8; 8] {
        key_id(&self.pk)
    }
    fn sign(&self, msg: &[u8]) -> [u8; 64] {
        *self.sk.sign(msg, None)
    }
}

/// What the image carries besides its body.
#[derive(Clone, Copy)]
struct Shape {
    kind: u8,
    slot: u8,
    version: Version,
    tag: [u8; 16],
}

impl Shape {
    fn app(slot: u8) -> Self {
        Shape {
            kind: image_kind::APP,
            slot,
            version: Version::new(1, 1, 0),
            tag: TAG,
        }
    }
}

fn image(s: Shape, body: &[u8], signer: &impl Signer) -> Vec<u8> {
    let mut h = ImageHeader {
        image_kind: s.kind,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: FAKE_MCU,
        slot_binding: s.slot,
        placement: placement::INLINE,
        load_addr: 0,
        payload_size: body.len() as u32,
        version: s.version,
        build_flags: build_flags::TEST_KEY | verifying(s.kind, build_flags::VERIFY_ENFORCED),
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(body),
        key_id: signer.key_id(),
        target_tag: s.tag,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB | verifying(s.kind, capabilities::SIG_VERIFIER),
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = signer.sign(&msg);
    [&h.to_bytes()[..], body].concat()
}

/// `bit` for an image that verifies others (a boot core), else 0.
fn verifying(kind: u8, bit: u32) -> u32 {
    if kind == image_kind::APP {
        0
    } else {
        bit
    }
}

fn body() -> Vec<u8> {
    (0..BODY as u32).map(|i| (i * 7) as u8).collect()
}

/// One node: the engine over the fake MCU's flash, as writer `W` runs it.
struct Bench<V: SigVerifier, W: Writer> {
    mem: &'static RefCell<Mem>,
    agent: Agent<&'static RefCell<Mem>, Chain, V, W>,
    seq: u8,
}

impl<V: SigVerifier, W: Writer> Bench<V, W> {
    /// The session in the mode its writer writes in: the app's, or the
    /// boot core's update mode.
    fn new(verifier: V, mem: Mem) -> Self {
        let m = if W::APP { mode::APP } else { mode::UPDATE };
        Self::in_mode(verifier, mem, m)
    }

    /// The session answering HELLO with `mode` (`payload::mode`).
    fn in_mode(verifier: V, mem: Mem, mode: u8) -> Self {
        let mem: &'static RefCell<Mem> = Box::leak(Box::new(RefCell::new(mem)));
        let cfg = Config { mode, ..config() };
        Bench {
            mem,
            agent: Agent::for_writer(cfg, verifier, mem, chain()),
            seq: 0,
        }
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> Status {
        self.exchange(cmd, payload).0
    }

    /// The reply's status, body and whether the node resets after it.
    fn exchange(&mut self, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>, bool) {
        self.seq = self.seq.wrapping_add(1);
        let mut buf = vec![0u8; 300];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = self.agent.handle(&buf[..n]).expect("reply");
        let reboot = reply.reboot;
        let (f, _) = Frame::decode(reply.frame, 300).unwrap();
        assert_eq!((f.seq, f.flags), (self.seq, flags::RESP));
        let (st, body) = split_status(f.payload).unwrap();
        let body = bulk_of(f.cmd, st, body);
        (st, body.to_vec(), reboot)
    }

    /// XFER of `img` (`total` bytes announced) into `slot` of `t`: the
    /// status of the first step that is not OK.
    fn transfer_as(&mut self, t: u8, slot: u8, img: &[u8], total: u32) -> Status {
        let mut p = [0u8; 16];
        let n = XferBegin {
            target: t,
            slot,
            total_len: total,
            hdr_len: HEADER_SIZE as u16,
        }
        .encode(&mut p)
        .unwrap();
        let st = self.call(Cmd::XferBegin, &p[..n]);
        if st != Status::Ok {
            return st;
        }
        let (hdr, body) = img.split_at(HEADER_SIZE);
        for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
            for (i, chunk) in part.chunks(200).enumerate() {
                let mut p = [0u8; 256];
                let n = XferChunk {
                    offset: (i * 200) as u32,
                    data: chunk,
                }
                .encode(&mut p)
                .unwrap();
                let st = self.call(cmd, &p[..n]);
                if st != Status::Ok {
                    return st;
                }
            }
        }
        self.call(Cmd::XferEnd, &[])
    }

    fn transfer(&mut self, t: u8, slot: u8, img: &[u8]) -> Status {
        self.transfer_as(t, slot, img, img.len() as u32)
    }

    fn set_pending(&mut self, t: u8, slot: u8) -> Status {
        self.call(
            Cmd::SetPending,
            &wire(|b| dagon_proto::payload::SetPending { target: t, slot }.encode(b)),
        )
    }

    fn state(&self, t: u8, slot: u8) -> SlotState {
        self.mem.borrow().status[usize::from(t)].slots[usize::from(slot)].state
    }
}

fn keypair() -> KeyPair {
    KeyPair::from_seed(Seed::new([0x5A; 32]))
}

fn ed25519(kp: &KeyPair) -> Ed25519Compact {
    Ed25519Compact::new(*kp.pk)
}

/// The image cases: name, what is sent, the bytes announced.
fn cases(kp: &KeyPair) -> Vec<(&'static str, Vec<u8>, u32)> {
    let good = image(Shape::app(1), &body(), kp);
    let mut bad_sig = good.clone();
    bad_sig[0x1C0] ^= 1;
    let other = Shape {
        tag: target_tag("another-node"),
        ..Shape::app(1)
    };
    // The node takes an older version: refusing a downgrade is the host's
    // (the release's ALLOW_DOWNGRADE), not the session's.
    let older = Shape {
        version: Version::new(0, 9, 0),
        ..Shape::app(1)
    };
    let full = HEADER_SIZE as u32 + CAP;
    vec![
        ("good", good.clone(), good.len() as u32),
        ("bad signature", bad_sig.clone(), bad_sig.len() as u32),
        (
            "wrong target",
            image(other, &body(), kp),
            (HEADER_SIZE + BODY) as u32,
        ),
        ("too large", good, full + 1),
        (
            "rolled-back version",
            image(older, &body(), kp),
            (HEADER_SIZE + BODY) as u32,
        ),
    ]
}

/// Every image case gives the same verdict through the app's writer and
/// the boot core's: the same engine type, checks and verifier.
#[test]
fn every_image_case_gets_the_same_verdict_from_every_writer() {
    let kp = keypair();
    let want = [
        Status::Ok,
        Status::Signature,
        Status::Compat,
        Status::Range,
        Status::Ok,
    ];
    let mut writers_run = Vec::new();
    println!("{:<22} {:>10} {:>10}", "image", "app", "boot core");
    for ((name, img, total), want) in cases(&kp).into_iter().zip(want) {
        let verdicts = [
            verdict::<writer::App>(&kp, &img, total, &mut writers_run),
            verdict::<writer::BootCore>(&kp, &img, total, &mut writers_run),
        ];
        let [app, core] = verdicts.map(|v| format!("{v:?}"));
        println!("{name:<22} {app:>10} {core:>10}");
        assert_eq!(verdicts, [want, want], "{name}");
    }
    writers_run.sort();
    writers_run.dedup();
    assert_eq!(
        writers_run.len(),
        2,
        "both writers ran the engine: {writers_run:?}"
    );
}

/// The verdict on `img` (`total` bytes announced) for APP slot B through
/// the engine as writer `W` runs it; the writer's type name goes to `run`.
fn verdict<W: Writer>(kp: &KeyPair, img: &[u8], total: u32, run: &mut Vec<&str>) -> Status {
    let mut node = Bench::<_, W>::new(ed25519(kp), Mem::new(2, 2));
    run.push(std::any::type_name::<W>());
    node.transfer_as(target::APP, 1, img, total)
}

/// The app never writes or sets pending the slot it runs from, nor the
/// confirmed slot it goes back to; the boot core runs from neither APP
/// slot, so it writes the confirmed slot of an app that has only one, and
/// sets it pending. The boot core never writes the boot core slot it runs
/// from, which the app writes in place when the MCU has only that one.
#[test]
fn each_writer_is_kept_from_the_slot_it_runs_from() {
    let kp = keypair();
    let a = image(Shape::app(slot_binding::A), &body(), &kp);

    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(1, 1));
    assert_eq!(app.transfer(target::APP, 0, &a), Status::State, "running");
    assert_eq!(app.set_pending(target::APP, 0), Status::State, "running");
    let mut two = Mem::new(2, 1);
    two.status[0].active = 1;
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), two);
    assert_eq!(app.transfer(target::APP, 0, &a), Status::State, "confirmed");

    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(1, 1));
    assert_eq!(core.transfer(target::APP, 0, &a), Status::Ok);
    assert_eq!(core.state(target::APP, 0), SlotState::Written);
    assert_eq!(core.set_pending(target::APP, 0), Status::Ok);
    assert_eq!(core.state(target::APP, 0), SlotState::Pending);

    let c = image(
        Shape {
            kind: image_kind::BOOTCORE,
            ..Shape::app(slot_binding::A)
        },
        &body(),
        &kp,
    );
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(1, 1));
    assert_eq!(
        core.transfer(target::BOOTCORE, 0, &c),
        Status::Unsupported,
        "no MCU's [writers] lists the boot core for a boot core slot"
    );
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(1, 1));
    assert_eq!(
        app.transfer(target::BOOTCORE, 0, &c),
        Status::Ok,
        "the app writes the only boot core slot in place"
    );
}

/// Only the app confirms the trial it runs: the boot core runs while no
/// app does, so a CONFIRM there leaves the slot on trial.
#[test]
fn only_the_app_confirms_its_trial() {
    let kp = keypair();
    let mut mem = Mem::new(2, 1);
    mem.status[0].active = 1;
    mem.status[0].slots[1].state = SlotState::Trying;
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), mem);
    assert_eq!(
        core.call(
            Cmd::Confirm,
            &wire(|b| dagon_proto::payload::encode_confirm(target::APP, b))
        ),
        Status::State
    );
    assert_eq!(core.agent.confirm_own(), Err(Status::State));
    assert_eq!(core.state(target::APP, 1), SlotState::Trying);
}

/// The boot core makes no log room when its session is made (nor when it
/// starts again): outside a write the boot stages never compact or erase.
/// The app makes room at its start, as before.
#[test]
fn only_the_app_makes_log_room_when_its_session_starts() {
    let kp = keypair();
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(2, 1));
    assert_eq!(app.mem.borrow().rooms, 1);
    app.agent.restart();
    assert_eq!(app.mem.borrow().rooms, 2);
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(2, 1));
    assert_eq!(core.mem.borrow().rooms, 0);
    core.agent.restart();
    assert_eq!(core.mem.borrow().rooms, 0);
}

/// What only the app's writer serves is not in the boot core's session:
/// SHOW and the recovery agent's target are unsupported or refused there,
/// as the app answers them.
#[test]
fn the_boot_core_serves_none_of_the_app_only_requests() {
    let kp = keypair();
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(2, 1));
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(2, 1));
    // Core: the phase; field 2: the percent.
    let show = [1, 0x04, 1, 42];
    assert_eq!(app.call(Cmd::Show, &show), Status::Ok);
    assert_eq!(core.call(Cmd::Show, &show), Status::Unsupported);
    let p = begin(target::AGENT, 2, HEADER_SIZE as u16);
    // This store keeps no agent slot; the app finds no room for it.
    assert_eq!(app.call(Cmd::XferBegin, &p), Status::Unsupported);
    // No MCU's [writers] lists the boot core for the recovery agent.
    assert_eq!(core.call(Cmd::XferBegin, &p), Status::Unsupported);
}

/// XFER_BEGIN of 600 bytes into `slot` of `t` with `hdr` header bytes.
fn begin(t: u8, slot: u8, hdr: u16) -> Vec<u8> {
    let mut p = [0u8; 16];
    let n = XferBegin {
        target: t,
        slot,
        total_len: 600,
        hdr_len: hdr,
    }
    .encode(&mut p)
    .unwrap();
    p[..n].to_vec()
}

/// Target 3 is reserved: no writer takes it, with or without a header,
/// even from a store that still reports room for it; the refusal is the
/// one for any target the writer does not take, and nothing is erased.
#[test]
fn no_writer_takes_the_reserved_target() {
    let kp = keypair();
    let room = || {
        let mut m = Mem::new(2, 1);
        m.reserved_room = true;
        m
    };
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), room());
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), room());
    for hdr in [0, HEADER_SIZE as u16] {
        let p = begin(RESERVED_TARGET, 0, hdr);
        assert_eq!(app.call(Cmd::XferBegin, &p), Status::Arg, "app, {hdr}");
        assert_eq!(core.call(Cmd::XferBegin, &p), Status::Arg, "core, {hdr}");
    }
    assert!(app.mem.borrow().bodies.is_empty());
    assert!(core.mem.borrow().bodies.is_empty());
}

/// A signature scheme no DAGON code knows: SHA-256 of the key, the domain
/// and the message, twice.
struct FakeSig([u8; 32]);

impl FakeSig {
    fn digest(&self, domain: &[u8], msg: &[&[u8]]) -> [u8; 64] {
        let mut h = dagon_format::Sha256::new();
        h.update(&self.0);
        h.update(domain);
        for m in msg {
            h.update(m);
        }
        let d = h.finalize();
        let mut out = [0u8; 64];
        out[..32].copy_from_slice(&d);
        out[32..].copy_from_slice(&d);
        out
    }
}

impl SigVerifier for FakeSig {
    fn key_id(&self) -> [u8; 8] {
        key_id(&self.0)
    }
    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        self.digest(domain, msg) == *sig
    }
}

impl Signer for FakeSig {
    fn key_id(&self) -> [u8; 8] {
        SigVerifier::key_id(self)
    }
    fn sign(&self, msg: &[u8]) -> [u8; 64] {
        let domain_len = SIGN_DOMAIN.len();
        self.digest(&msg[..domain_len], &[&msg[domain_len..]])
    }
}

/// A new MCU is drivers, not engine code: the engine runs for both writers
/// on an MCU of its own id, flash and signature scheme, and the engine's
/// sources name no MCU a definition gives.
#[test]
fn the_engine_runs_on_an_mcu_only_its_drivers_know() {
    let key = FakeSig([0x33; 32]);
    let img = image(Shape::app(1), &body(), &key);
    fake_mcu_writes::<writer::App>(&img);
    fake_mcu_writes::<writer::BootCore>(&img);

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let names = mcu_names(&root.join("targets/mcu"));
    assert!(names.len() >= 3, "{names:?}");
    for engine in [
        "crates/dagon-agent/src",
        "crates/dagon-core/src",
        "crates/dagon-port/src",
    ] {
        for (file, text) in sources(&root.join(engine)) {
            let text = text.to_lowercase();
            for n in &names {
                assert!(!text.contains(n), "{file} names the MCU {n}");
            }
        }
    }
}

/// `img` goes into the fake MCU's APP slot B under writer `W`, and is
/// refused under another key.
fn fake_mcu_writes<W: Writer>(img: &[u8]) {
    let w = std::any::type_name::<W>();
    let mut node = Bench::<_, W>::new(FakeSig([0x33; 32]), Mem::new(2, 0));
    assert_eq!(node.transfer(target::APP, 1, img), Status::Ok, "{w}");
    assert_eq!(node.set_pending(target::APP, 1), Status::Ok, "{w}");
    let mut other = Bench::<_, W>::new(FakeSig([0x34; 32]), Mem::new(2, 0));
    assert_eq!(other.transfer(target::APP, 1, img), Status::Key, "{w}");
}

/// The `id` and `name` of every MCU definition, lower case.
fn mcu_names(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let def = e.unwrap().path().join("dagon-mcu.toml");
        let Ok(text) = std::fs::read_to_string(&def) else {
            continue;
        };
        for line in text.lines().take_while(|l| !l.starts_with('[')) {
            for key in ["id", "name"] {
                let value = line
                    .strip_prefix(key)
                    .and_then(|r| r.trim_start().strip_prefix('='))
                    .map(|v| v.trim().trim_matches('"').to_lowercase());
                out.extend(value);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Every `.rs` file under `dir`, with its text.
fn sources(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(sources(&p));
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            ));
        }
    }
    out
}

/// REBOOT mode 1 to the app: the store writes the stay for the boot core
/// (UPDATE_STAY) and the node resets after the reply. A node whose boot
/// core has no update mode refuses it and does not reset.
#[test]
fn the_app_hands_over_to_the_boot_core_with_reboot_mode_1() {
    let kp = keypair();
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(2, 1));
    let (st, _, reboot) = app.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, b)),
    );
    assert_eq!((st, reboot), (Status::Ok, true));
    assert_eq!(app.mem.borrow().stays, 1);

    let mut none = Mem::new(2, 1);
    none.update_mode = false;
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), none);
    let (st, _, reboot) = app.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, b)),
    );
    assert_eq!((st, reboot), (Status::Unsupported, false));

    let (st, _, reboot) = app.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
    );
    assert_eq!((st, reboot), (Status::Ok, true), "mode 0 as before");
    let (st, _, reboot) = app.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(2, b)),
    );
    assert_eq!((st, reboot), (Status::Arg, false), "a mode not defined");
}

/// STAY is the boot core's: while the app runs it is refused (E_STATE);
/// the boot core in update mode takes it, and REBOOT mode 1 there keeps
/// it in update mode (no reset, no stay written).
#[test]
fn stay_and_reboot_mode_1_keep_the_boot_core_in_update_mode() {
    let kp = keypair();
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(2, 1));
    assert_eq!(app.call(Cmd::Stay, &[]), Status::State);
    assert_eq!(app.call(Cmd::Stay, &[0]), Status::Arg);

    let mut core =
        Bench::<_, writer::BootCore>::in_mode(ed25519(&kp), Mem::new(2, 1), mode::UPDATE);
    let (st, _, reboot) = core.exchange(Cmd::Stay, &[]);
    assert_eq!((st, reboot), (Status::Ok, false));
    let (st, _, reboot) = core.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, b)),
    );
    assert_eq!((st, reboot), (Status::Ok, false));
    assert_eq!(core.mem.borrow().stays, 0);
    let (st, _, reboot) = core.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
    );
    assert_eq!((st, reboot), (Status::Ok, true), "leaves update mode");
}

/// The boot core in update mode makes log room with the shared algorithm
/// (the store's reservation, which compacts) when a write starts, before anything is
/// erased; when it cannot, the write is refused and nothing is erased.
/// The app's write path is unchanged: its room was made at its start.
#[test]
fn the_boot_core_makes_log_room_before_a_write_starts() {
    let kp = keypair();
    let a = image(Shape::app(slot_binding::A), &body(), &kp);
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(2, 1));
    assert_eq!(core.mem.borrow().rooms, 0);
    assert_eq!(core.transfer(target::APP, 0, &a), Status::Ok);
    assert_eq!(core.mem.borrow().rooms, 1);

    let mut stuck = Mem::new(2, 1);
    stuck.room_fails = true;
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), stuck);
    assert_eq!(core.transfer(target::APP, 0, &a), Status::Flash);
    assert!(core.mem.borrow().bodies.is_empty(), "nothing erased");
    assert_eq!(core.state(target::APP, 0), SlotState::Confirmed);

    let b = image(Shape::app(slot_binding::B), &body(), &kp);
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(2, 1));
    assert_eq!(app.mem.borrow().rooms, 1);
    assert_eq!(app.transfer(target::APP, 1, &b), Status::Ok);
    assert_eq!(app.mem.borrow().rooms, 1, "the app path is unchanged");
}

/// Outside update and recovery mode the boot core's session writes nothing:
/// transfers, SET_PENDING and REBOOT mode 1 are refused with E_STATE and
/// nothing is erased, written or reserved in the log; HELLO, STATUS,
/// VERSION, STAY and REBOOT mode 0 are answered. In update and recovery
/// mode the same requests are taken.
#[test]
fn the_boot_core_writes_only_in_update_or_recovery_mode() {
    let kp = keypair();
    let b = image(Shape::app(slot_binding::B), &body(), &kp);
    for m in [mode::UPDATE, mode::RECOVERY] {
        let mut core = Bench::<_, writer::BootCore>::in_mode(ed25519(&kp), Mem::new(2, 1), m);
        assert_eq!(core.transfer(target::APP, 1, &b), Status::Ok, "mode {m}");
        assert_eq!(core.mem.borrow().rooms, 1, "mode {m}");
        assert_eq!(core.set_pending(target::APP, 1), Status::Ok, "mode {m}");
        assert_eq!(core.state(target::APP, 1), SlotState::Pending, "mode {m}");
        let (st, _, reboot) = core.exchange(
            Cmd::Reboot,
            &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, b)),
        );
        assert_eq!((st, reboot), (Status::Ok, false), "mode {m}");
    }
    for m in [mode::APP, mode::AGENT] {
        // Slot B holds a verified image, written in update mode.
        let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(2, 1));
        assert_eq!(core.transfer(target::APP, 1, &b), Status::Ok);
        let mem = core.mem.replace(Mem::new(2, 1));
        let (bodies, headers, rooms) = (mem.bodies.clone(), mem.headers.clone(), mem.rooms);
        let mut core = Bench::<_, writer::BootCore>::in_mode(ed25519(&kp), mem, m);

        // Transfer: refused at XFER_BEGIN; no session for the rest.
        assert_eq!(core.transfer(target::APP, 0, &b), Status::State, "mode {m}");
        assert_eq!(core.call(Cmd::XferEnd, &[]), Status::State, "mode {m}");
        // Pending: the verified image in slot B is not selected.
        assert_eq!(core.set_pending(target::APP, 1), Status::State, "mode {m}");
        assert_eq!(core.state(target::APP, 1), SlotState::Written, "mode {m}");
        // Log: no room reserved, no stay kept.
        let (st, _, reboot) = core.exchange(
            Cmd::Reboot,
            &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, b)),
        );
        assert_eq!((st, reboot), (Status::State, false), "mode {m}");
        {
            let mem = core.mem.borrow();
            assert_eq!(mem.bodies, bodies, "nothing erased or written, mode {m}");
            assert_eq!(mem.headers, headers, "mode {m}");
            assert_eq!((mem.rooms, mem.stays), (rooms, 0), "mode {m}");
            assert_eq!(mem.status[0].slots[0].state, SlotState::Confirmed);
        }

        let mut p = [0u8; 32];
        let n = HelloReq {
            proto_ver: 1,
            max_payload: 256,
        }
        .encode(&mut p)
        .unwrap();
        assert_eq!(core.call(Cmd::Hello, &p[..n]), Status::Ok, "mode {m}");
        assert_eq!(core.call(Cmd::Status, &[]), Status::Ok, "mode {m}");
        assert_eq!(core.call(Cmd::Version, &[]), Status::Ok, "mode {m}");
        assert_eq!(core.call(Cmd::Stay, &[]), Status::Ok, "mode {m}");
        let (st, _, reboot) = core.exchange(
            Cmd::Reboot,
            &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
        );
        assert_eq!((st, reboot), (Status::Ok, true), "mode {m}");
    }
}

/// One request a host sent and the status the node gave.
type Step = (Cmd, Status);

/// Sends `cmd` and records the status; whether it was OK.
fn step<W: Writer>(
    n: &mut Bench<Ed25519Compact, W>,
    log: &mut Vec<Step>,
    cmd: Cmd,
    p: &[u8],
) -> bool {
    let st = n.call(cmd, p);
    log.push((cmd, st));
    st == Status::Ok
}

/// The write of `img` into `slot` of `t` as a host that predates update
/// mode sends it (dagon-host `transfer`): one SHOW first, whose
/// E_UNSUPPORTED only stops the telling, then XFER_BEGIN, XFER_HDR and
/// XFER_DATA in chunks of `chunk` bytes, and XFER_END, whose digest it
/// checks. It stops at the first refusal.
fn old_transfer<W: Writer>(
    n: &mut Bench<Ed25519Compact, W>,
    log: &mut Vec<Step>,
    (t, slot, img): (u8, u8, &[u8]),
    chunk: usize,
) -> bool {
    let mut p = [0u8; 64];
    let len = Show {
        node: 1,
        phase: phase::TRANSFER,
        percent: 0,
        version: Version::new(1, 1, 0),
        text: &[],
    }
    .encode(&mut p)
    .unwrap();
    step(n, log, Cmd::Show, &p[..len]);
    let len = XferBegin {
        target: t,
        slot,
        total_len: img.len() as u32,
        hdr_len: HEADER_SIZE as u16,
    }
    .encode(&mut p)
    .unwrap();
    if !step(n, log, Cmd::XferBegin, &p[..len]) {
        return false;
    }
    let (hdr, body) = img.split_at(HEADER_SIZE);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(chunk).enumerate() {
            let offset = (i * chunk) as u32;
            let p = wire(|b| XferChunk { offset, data: c }.encode(b));
            if !step(n, log, cmd, &p) {
                return false;
            }
        }
    }
    let (st, d, _) = n.exchange(Cmd::XferEnd, &[]);
    log.push((Cmd::XferEnd, st));
    st == Status::Ok && parse_digest(&d) == Ok(sha256(body))
}

/// What a host that predates update mode sends to write a node (dagon-host
/// `update_node`, run by `dagon deploy` and by `dagon recover` once the
/// node answers HELLO): its session's opening HELLO, HELLO, STATUS; with a
/// boot core in the release that the node does not run, STATUS of the
/// core slots, HELLO and the core's write into the slot it picks; then the
/// app's write into the slot it picks, SET_PENDING and REBOOT mode 0, in
/// chunks of the HELLO's max payload less the 4-byte offset. It stops at
/// the first refusal. The steps, and whether the node resets.
fn old_host_write<W: Writer>(
    n: &mut Bench<Ed25519Compact, W>,
    core: Option<(u8, &[u8])>,
    (slot, app): (u8, &[u8]),
) -> (Vec<Step>, bool) {
    let mut log = Vec::new();
    let mut p = [0u8; 32];
    let len = HelloReq {
        proto_ver: 1,
        max_payload: max_payload::USB as u16,
    }
    .encode(&mut p)
    .unwrap();
    let hello = &p[..len];
    step(n, &mut log, Cmd::Hello, hello);
    let (st, h, _) = n.exchange(Cmd::Hello, hello);
    log.push((Cmd::Hello, st));
    let chunk = usize::from(HelloResp::parse(&h).unwrap().max_payload) - XferChunk::OVERHEAD;
    if !step(n, &mut log, Cmd::Status, &[]) {
        return (log, false);
    }
    if let Some((cs, img)) = core {
        let ok = step(
            n,
            &mut log,
            Cmd::Status,
            &wire(|b| dagon_proto::payload::encode_status_req(target::BOOTCORE, b)),
        ) && step(n, &mut log, Cmd::Hello, hello)
            && old_transfer(n, &mut log, (target::BOOTCORE, cs, img), chunk);
        if !ok {
            return (log, false);
        }
    }
    let ok = old_transfer(n, &mut log, (target::APP, slot, app), chunk)
        && step(
            n,
            &mut log,
            Cmd::SetPending,
            &wire(|b| {
                dagon_proto::payload::SetPending {
                    target: target::APP,
                    slot,
                }
                .encode(b)
            }),
        );
    if !ok {
        return (log, false);
    }
    let (st, _, reboot) = n.exchange(
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
    );
    log.push((Cmd::Reboot, st));
    (log, reboot)
}

/// A host that predates update mode, writing a node held in its boot core
/// (update or recovery mode; two app slots and one boot core slot, slot A
/// of each running and confirmed), reads HELLO's mode 1 or 2 as an app
/// node's. With only an app in the release its usual sequence writes app
/// slot B, sets it pending and resets the node out of the mode; its one
/// SHOW gets E_UNSUPPORTED and it tells nothing more. With a new boot core
/// in the release it first asks to write the only core slot, the one the
/// boot core runs from: refused with E_STATE before anything is erased,
/// so it stops there and the node stays in its mode with its slots as
/// they were.
#[test]
fn an_old_hosts_write_through_the_boot_core_writes_the_app_but_never_the_running_core() {
    let kp = keypair();
    let app = image(Shape::app(slot_binding::B), &body(), &kp);
    let new_core = Shape {
        kind: image_kind::BOOTCORE,
        ..Shape::app(slot_binding::A)
    };
    let new_core = image(new_core, &body(), &kp);
    let ok = |c| (c, Status::Ok);
    let chunk = max_payload::I2C - XferChunk::OVERHEAD;
    for m in [mode::UPDATE, mode::RECOVERY] {
        let mut core = Bench::<_, writer::BootCore>::in_mode(ed25519(&kp), Mem::new(2, 1), m);
        let (log, reboot) = old_host_write(&mut core, None, (1, &app));
        let mut want = vec![ok(Cmd::Hello), ok(Cmd::Hello), ok(Cmd::Status)];
        want.extend([(Cmd::Show, Status::Unsupported), ok(Cmd::XferBegin)]);
        want.extend(std::iter::repeat_n(
            ok(Cmd::XferHdr),
            HEADER_SIZE.div_ceil(chunk),
        ));
        want.extend(std::iter::repeat_n(ok(Cmd::XferData), BODY.div_ceil(chunk)));
        want.extend([ok(Cmd::XferEnd), ok(Cmd::SetPending), ok(Cmd::Reboot)]);
        assert_eq!(log, want, "mode {m}");
        assert!(reboot, "the node resets out of mode {m}");
        assert_eq!(core.state(target::APP, 1), SlotState::Pending, "mode {m}");

        let mut core = Bench::<_, writer::BootCore>::in_mode(ed25519(&kp), Mem::new(2, 1), m);
        let (log, reboot) = old_host_write(&mut core, Some((0, &new_core)), (1, &app));
        let want = vec![
            ok(Cmd::Hello),
            ok(Cmd::Hello),
            ok(Cmd::Status),
            ok(Cmd::Status),
            ok(Cmd::Hello),
            (Cmd::Show, Status::Unsupported),
            // The boot core writes no boot core slot ([writers]).
            (Cmd::XferBegin, Status::Unsupported),
        ];
        assert_eq!(log, want, "mode {m}");
        assert!(!reboot, "the node stays in mode {m}");
        let mem = core.mem.borrow();
        assert!(
            mem.bodies.is_empty() && mem.headers.is_empty(),
            "nothing erased"
        );
        assert_eq!(mem.rooms, 0, "mode {m}");
        assert_eq!(mem.status[1].slots[0].state, SlotState::Confirmed);
        assert_eq!(mem.status[0].slots[0].state, SlotState::Confirmed);
    }
}

/// A writer writes only the image kinds the MCU definitions' `[writers]`
/// list it for: the boot core no boot core slot, not even the inactive one
/// of two, refused at XFER_BEGIN before anything is erased, and its
/// SET_PENDING for one refused too.
#[test]
fn the_boot_core_writes_only_what_writers_lists_for_it() {
    let kp = keypair();
    let c = image(
        Shape {
            kind: image_kind::BOOTCORE,
            ..Shape::app(slot_binding::B)
        },
        &body(),
        &kp,
    );
    let mut core = Bench::<_, writer::BootCore>::new(ed25519(&kp), Mem::new(2, 2));
    assert_eq!(core.transfer(target::BOOTCORE, 1, &c), Status::Unsupported);
    assert!(
        !core
            .mem
            .borrow()
            .bodies
            .contains_key(&(target::BOOTCORE, 1)),
        "nothing erased"
    );
    assert_eq!(core.set_pending(target::BOOTCORE, 1), Status::Unsupported);
    let mut app = Bench::<_, writer::App>::new(ed25519(&kp), Mem::new(2, 2));
    assert_eq!(
        app.transfer(target::BOOTCORE, 1, &c),
        Status::Ok,
        "the app writes it"
    );
}

/// Each writer's targets are what the built-in MCU definitions' `[writers]`
/// list that kind for, no more and no fewer.
#[test]
fn the_writers_targets_are_the_definitions() {
    use dagon_topology::defs::{ImageKind, Registry};
    use std::collections::BTreeSet;
    let reg = Registry::builtin();
    let target_of = |k: ImageKind| match k {
        ImageKind::App => target::APP,
        ImageKind::Bootcore => target::BOOTCORE,
        ImageKind::Agent => target::AGENT,
        ImageKind::Stage0 => unreachable!("stage0 is not written in the field"),
    };
    let listed = |w: ImageKind| -> BTreeSet<u8> {
        reg.mcus()
            .iter()
            .flat_map(|m| m.writers.iter())
            .filter(|(_, by)| by.contains(&w))
            .map(|(&k, _)| target_of(k))
            .collect()
    };
    let of = |t: &[u8]| t.iter().copied().collect::<BTreeSet<u8>>();
    assert_eq!(of(<writer::App as Writer>::WRITES), listed(ImageKind::App));
    assert_eq!(
        of(<writer::BootCore as Writer>::WRITES),
        listed(ImageKind::Bootcore)
    );
}

/// The request payload `enc` writes.
#[allow(dead_code)]
fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 1100];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}

/// The bulk of an OK RELAY or I2C_RAW response; any other body as it is.
#[allow(dead_code)]
fn bulk_of(cmd: u8, status: dagon_proto::Status, body: &[u8]) -> &[u8] {
    match dagon_proto::Cmd::from_u8(cmd) {
        Some(c @ (dagon_proto::Cmd::Relay | dagon_proto::Cmd::I2cRaw))
            if status == dagon_proto::Status::Ok =>
        {
            dagon_proto::payload::parse_bulk_resp(c, body).unwrap()
        }
        _ => body,
    }
}
