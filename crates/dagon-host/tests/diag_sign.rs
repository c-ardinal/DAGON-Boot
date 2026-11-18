//! The host's signed clear of link diagnostics against an app agent on
//! fake ports: the key check before any seed is spent, what is signed,
//! and the read-back when an answer is lost across a reset.

#[path = "../../dagon-agent/tests/diag_bench/mod.rs"]
mod diag_bench;

use std::cell::RefCell;
use std::rc::Rc;

use dagon_format::{key_id, ClearSigner, SignError, DIAG_CLEAR_DOMAIN};
use dagon_host::diag::ClearOutcome;
use dagon_host::{Error, Host, HostClock, Target, Transport};
use dagon_proto::diag::{ClearScope, Seed};
use dagon_proto::field::FieldWriter;
use dagon_proto::payload::HelloResp;
use dagon_proto::table::hello;
use dagon_proto::{finish, flags, max_payload, split_status, Cmd, Frame, Status, HEADER_LEN};
use dagon_sim::block_on;
use diag_bench::*;
use ed25519_compact::KeyPair;

/// What the node side does with a frame.
#[derive(Default)]
struct Wire {
    /// Commands the node was sent, in order.
    sent: Vec<u8>,
    /// Answers of this command dropped, this many times; the node is
    /// reset after it ran each of them.
    lose_and_reset: Option<(u8, usize)>,
    /// HELLO answers lose their KEY_ID.
    no_key_id: bool,
    now: u64,
}

#[derive(Clone)]
struct Direct {
    node: Rc<RefCell<Node<Counter>>>,
    wire: Rc<RefCell<Wire>>,
    pending: Rc<RefCell<Vec<u8>>>,
}

/// The HELLO answer `frame` without its KEY_ID field.
fn without_key_id(frame: &[u8]) -> Vec<u8> {
    let (f, _) = Frame::decode(frame, max_payload::USB).unwrap();
    let (_, body) = split_status(f.payload).unwrap();
    let h = HelloResp::parse(body).unwrap();
    let mut out = vec![0u8; 128];
    let mut w = FieldWriter::new(&mut out[HEADER_LEN + 1..]);
    w.put(&h.proto_ver.to_le_bytes())
        .unwrap()
        .put(&h.max_payload.to_le_bytes())
        .unwrap()
        .field(&hello::ROLE, &[h.role])
        .unwrap()
        .field(&hello::PORT_KIND, &[h.port_kind])
        .unwrap()
        .field(&hello::MCU_ID, &h.mcu_id.to_le_bytes())
        .unwrap()
        .field(&hello::MODE, &[h.mode])
        .unwrap()
        .field(&hello::ACTIVE_VERSION, &h.active_version.to_le_bytes())
        .unwrap()
        .field(&hello::TRIAL, &[h.trial as u8])
        .unwrap()
        .field(&hello::CMDS, h.cmds.as_bytes())
        .unwrap();
    let n = 1 + w.len();
    out[HEADER_LEN] = Status::Ok as u8;
    let len = finish(&mut out, flags::RESP, f.seq, f.cmd, n).unwrap();
    out.truncate(len);
    out
}

impl Transport for Direct {
    type Error = ();
    fn max_payload(&self) -> usize {
        max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Stays
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let (f, _) = Frame::decode(bytes, max_payload::USB).unwrap();
        let mut wire = self.wire.borrow_mut();
        wire.sent.push(f.cmd);
        let mut node = self.node.borrow_mut();
        let reply = node.agent.handle(bytes).map(|r| r.frame.to_vec());
        if let Some((cmd, left)) = wire.lose_and_reset.as_mut() {
            if *cmd == f.cmd && *left > 0 {
                *left -= 1;
                node.agent.restart();
                return Ok(());
            }
        }
        let reply = match reply {
            Some(r) if wire.no_key_id && f.cmd == Cmd::Hello as u8 => without_key_id(&r),
            Some(r) => r,
            None => return Ok(()),
        };
        self.pending.borrow_mut().extend(reply);
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let mut p = self.pending.borrow_mut();
        let n = buf.len().min(p.len());
        buf[..n].copy_from_slice(&p[..n]);
        p.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl HostClock for Direct {
    fn now_ms(&self) -> u64 {
        self.wire.borrow().now
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.wire.borrow_mut().now += u64::from(ms);
    }
}

/// The project's signer of the test key, recording what it signs.
struct Signer {
    kp: KeyPair,
    signed: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
}

impl Signer {
    fn new(kp: KeyPair) -> Self {
        Signer {
            kp,
            signed: RefCell::default(),
        }
    }
}

impl ClearSigner for Signer {
    fn key_id(&self) -> [u8; 8] {
        key_id(&self.kp.pk)
    }
    fn sign(&self, domain: &[u8], msg: &[&[u8]]) -> Result<[u8; 64], SignError> {
        let m = msg.concat();
        self.signed.borrow_mut().push((domain.to_vec(), m.clone()));
        Ok(*self.kp.sk.sign([domain, &m].concat(), None))
    }
}

const ID: [u8; 4] = [1, 2, 3, 4];

fn bench() -> (Host<Direct, Direct>, Direct, Fake) {
    let (node, fake) = counter_node(ID);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let d = Direct {
        node: Rc::new(RefCell::new(node)),
        wire: Rc::default(),
        pending: Rc::default(),
    };
    (Host::new(d.clone(), d.clone()), d, fake)
}

#[test]
fn signed_clear_accepted() {
    let (mut host, _, fake) = bench();
    let signer = Signer::new(keypair());
    let scope = ClearScope::EndpointAll(EP);
    let r = block_on(host.link_diag_clear(Target::Local, scope, Some(&signer)));
    assert_eq!(r.unwrap(), ClearOutcome::Cleared);
    assert_eq!(fake.0.borrow().clears, [scope]);
    let signed = signer.signed.borrow();
    let seed = Seed::counter(1, MCU, &ID).unwrap();
    let mut want = vec![seed.as_bytes().len() as u8];
    want.extend(seed.as_bytes());
    want.extend([16, EP.link, EP.addr]);
    assert_eq!(signed.as_slice(), [(DIAG_CLEAR_DOMAIN.to_vec(), want)]);
    // A DTC scope needs no signer.
    let r = block_on(host.link_diag_clear(Target::Local, ClearScope::AllDtcs, None));
    assert_eq!(r.unwrap(), ClearOutcome::Cleared);
}

#[test]
fn wrong_project_key() {
    // The node names its key: the host stops before DIAG_SEED.
    let (mut host, d, fake) = bench();
    let signer = Signer::new(foreign());
    let scope = ClearScope::All;
    let r = block_on(host.link_diag_clear(Target::Local, scope, Some(&signer)));
    match r {
        Err(Error::KeyMismatch { signer: s, node: n }) => {
            assert_eq!(s, key_id(&foreign().pk));
            assert_eq!(n, key_id(&keypair().pk));
        }
        other => panic!("{other:?}"),
    }
    assert!(!d.wire.borrow().sent.contains(&(Cmd::DiagSeed as u8)));
    assert_eq!(fake.0.borrow().counter, 0, "no seed spent");
    // A node that does not name its key is asked, and refuses.
    let (mut host, d, fake) = bench();
    d.wire.borrow_mut().no_key_id = true;
    let r = block_on(host.link_diag_clear(Target::Local, scope, Some(&signer)));
    assert!(
        matches!(
            r,
            Err(Error::Rejected {
                cmd: Cmd::LinkDiagClear,
                status: Status::Signature
            })
        ),
        "{r:?}"
    );
    assert_eq!(fake.0.borrow().peers.len(), 1);
    // A signed scope without a signer sends nothing.
    let (mut host, d, _) = bench();
    let r = block_on(host.link_diag_clear(Target::Local, scope, None));
    assert!(matches!(r, Err(Error::NoSigner)), "{r:?}");
    assert!(d.wire.borrow().sent.is_empty());
}

/// The clear ran, its answer was lost and the node reset before the
/// resend: the resend gets E_STATE, and the host reads the records back.
#[test]
fn a_lost_answer_across_a_reset_is_read_back() {
    let scope = ClearScope::EndpointAll(EP);
    let (mut host, d, fake) = bench();
    d.wire.borrow_mut().lose_and_reset = Some((Cmd::LinkDiagClear as u8, 1));
    let signer = Signer::new(keypair());
    let r = block_on(host.link_diag_clear(Target::Local, scope, Some(&signer)));
    assert_eq!(r.unwrap(), ClearOutcome::Cleared);
    assert_eq!(fake.0.borrow().clears.len(), 1);
    assert!(d.wire.borrow().sent.contains(&(Cmd::LinkDiag as u8)));
    // Cut before the clear landed: the record is still there.
    let (mut host, d, fake) = bench();
    fake.0.borrow_mut().cut_clear = Some(Cut::Before);
    d.wire.borrow_mut().lose_and_reset = Some((Cmd::LinkDiagClear as u8, 1));
    let r = block_on(host.link_diag_clear(Target::Local, scope, Some(&signer)));
    assert_eq!(r.unwrap(), ClearOutcome::NotCleared);
}
