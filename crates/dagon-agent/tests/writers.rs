//! One update session for every writer: the app's agent and the boot core
//! run the same engine with their drivers injected, give the same verdict
//! on every image, and differ only in the slot each one runs from.

use std::cell::RefCell;
use std::collections::BTreeMap;

use dagon_agent::{Agent, Config, SlotStore, StoreError, Transition, Writer};
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

fn config(writer: Writer) -> Config {
    Config {
        mcu_id: FAKE_MCU,
        port_kind: port_kind::SELF_BOOT,
        target_tag: TAG,
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: 3,
        writer,
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
        let has = self.count.get(usize::from(t)).is_some_and(|&n| slot < n);
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

/// One node: the engine over the fake MCU's flash, as `writer` runs it.
struct Bench<V: SigVerifier> {
    mem: &'static RefCell<Mem>,
    agent: Agent<&'static RefCell<Mem>, Chain, V>,
    seq: u8,
}

impl<V: SigVerifier> Bench<V> {
    fn new(writer: Writer, verifier: V, mem: Mem) -> Self {
        let mem: &'static RefCell<Mem> = Box::leak(Box::new(RefCell::new(mem)));
        Bench {
            mem,
            agent: Agent::new(config(writer), verifier, mem, chain()),
            seq: 0,
        }
    }

    fn call(&mut self, cmd: Cmd, payload: &[u8]) -> Status {
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
        let (f, _) = Frame::decode(reply.frame, 300).unwrap();
        assert_eq!((f.seq, f.flags), (self.seq, flags::RESP));
        split_status(f.payload).unwrap().0
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
        self.call(Cmd::SetPending, &[t, slot])
    }

    fn state(&self, t: u8, slot: u8) -> SlotState {
        self.mem.borrow().status[usize::from(t)].slots[usize::from(slot)].state
    }
}

const WRITERS: [Writer; 2] = [Writer::App, Writer::BootCore];

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
        let verdicts: Vec<Status> = WRITERS
            .iter()
            .map(|&w| {
                let mut node = Bench::new(w, ed25519(&kp), Mem::new(2, 2));
                writers_run.push(w);
                node.transfer_as(target::APP, 1, &img, total)
            })
            .collect();
        let [app, core] = [0, 1].map(|i| format!("{:?}", verdicts[i]));
        println!("{name:<22} {app:>10} {core:>10}");
        assert_eq!(verdicts, [want, want], "{name}");
    }
    assert!(
        WRITERS.iter().all(|w| writers_run.contains(w)),
        "both writers ran the engine"
    );
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

    let mut app = Bench::new(Writer::App, ed25519(&kp), Mem::new(1, 1));
    assert_eq!(app.transfer(target::APP, 0, &a), Status::State, "running");
    assert_eq!(app.set_pending(target::APP, 0), Status::State, "running");
    let mut two = Mem::new(2, 1);
    two.status[0].active = 1;
    let mut app = Bench::new(Writer::App, ed25519(&kp), two);
    assert_eq!(app.transfer(target::APP, 0, &a), Status::State, "confirmed");

    let mut core = Bench::new(Writer::BootCore, ed25519(&kp), Mem::new(1, 1));
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
    let mut core = Bench::new(Writer::BootCore, ed25519(&kp), Mem::new(1, 1));
    assert_eq!(
        core.transfer(target::BOOTCORE, 0, &c),
        Status::State,
        "the boot core runs from its only slot"
    );
    let mut app = Bench::new(Writer::App, ed25519(&kp), Mem::new(1, 1));
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
    let mut core = Bench::new(Writer::BootCore, ed25519(&kp), mem);
    assert_eq!(core.call(Cmd::Confirm, &[target::APP]), Status::State);
    assert_eq!(core.agent.confirm_own(), Err(Status::State));
    assert_eq!(core.state(target::APP, 1), SlotState::Trying);
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
    for w in WRITERS {
        let mut node = Bench::new(w, FakeSig([0x33; 32]), Mem::new(2, 0));
        assert_eq!(node.transfer(target::APP, 1, &img), Status::Ok, "{w:?}");
        assert_eq!(node.set_pending(target::APP, 1), Status::Ok, "{w:?}");
        let mut other = Bench::new(w, FakeSig([0x34; 32]), Mem::new(2, 0));
        assert_eq!(other.transfer(target::APP, 1, &img), Status::Key, "{w:?}");
    }

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
