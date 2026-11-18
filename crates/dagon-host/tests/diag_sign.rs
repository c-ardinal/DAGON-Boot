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
use dagon_proto::diag::{
    ClearScope, DiagInfo, Elements, Endpoint, Entry, Key, LinkDiagReq, LinkDiagResp, Params, Peer,
    Seed, NO_INSTANCE,
};
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
    /// LINK_DIAG is answered with the body this makes of the request.
    forge: Option<Forge>,
    now: u64,
}

type Forge = Box<dyn FnMut(&LinkDiagReq) -> Vec<u8>>;

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
        let forged = wire.forge.as_mut().filter(|_| f.cmd == Cmd::LinkDiag as u8);
        if let Some(forge) = forged {
            let body = forge(&LinkDiagReq::parse(f.payload).unwrap());
            let mut out = vec![0u8; dagon_proto::frame_len(1 + body.len())];
            out[HEADER_LEN] = Status::Ok as u8;
            out[HEADER_LEN + 1..HEADER_LEN + 1 + body.len()].copy_from_slice(&body);
            let n = finish(&mut out, flags::RESP, f.seq, f.cmd, 1 + body.len()).unwrap();
            self.pending.borrow_mut().extend(&out[..n]);
            return Ok(());
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

fn peer(link: u8, addr: u8) -> Vec<u8> {
    Peer {
        link,
        addr,
        instance: NO_INSTANCE,
    }
    .to_bytes()
    .to_vec()
}

/// A LINK_DIAG answer body of a power-cut-retained store.
fn page(elements: Elements<'_>, next: Option<&[u8]>) -> Vec<u8> {
    let r = LinkDiagResp {
        format: dagon_proto::table::link_diag::FORMAT,
        info: DiagInfo {
            availability: 0x7F,
            store: dagon_proto::table::link_diag::store::RETENTION_POWER_CUT,
            policy: dagon_proto::payload::PolicySet::EMPTY.with(1),
            params: Params::default(),
        },
        elements,
        next: next.map(|k| Key::new(k).unwrap()),
    };
    let mut b = vec![0u8; 400];
    let n = r.encode(&mut b).unwrap();
    b.truncate(n);
    b
}

/// The clear's answer is lost across a reset, and the read-back gets the
/// pages `forge` makes: a malformed one is never taken for the end of the
/// records.
fn read_back(scope: ClearScope, forge: Forge) -> Result<ClearOutcome, Error<()>> {
    let (mut host, d, _) = bench();
    let mut w = d.wire.borrow_mut();
    w.lose_and_reset = Some((Cmd::LinkDiagClear as u8, 1));
    w.forge = Some(forge);
    drop(w);
    block_on(host.link_diag_clear(Target::Local, scope, Some(&Signer::new(keypair()))))
}

#[test]
fn a_malformed_read_back_is_never_cleared() {
    let other = Endpoint { link: 2, addr: 9 };
    let entry = Entry {
        dtc: 1,
        link: EP.link,
        addr: EP.addr,
        status: 0x09,
        fdc: 0,
    }
    .to_bytes();
    let two = [peer(1, 5), peer(1, 3)].concat();
    let cases: Vec<(&str, ClearScope, Forge)> = vec![
        (
            "ENTRIES in report 2",
            ClearScope::EndpointAll(EP),
            Box::new(move |_| page(Elements::Entries(&entry), None)),
        ),
        (
            "a 5-byte NEXT",
            ClearScope::All,
            Box::new(|_| page(Elements::Peers(&peer(1, 3)), Some(&[0, 0, 1, 1, 3]))),
        ),
        (
            "keys out of order",
            ClearScope::All,
            Box::new(move |_| page(Elements::Peers(&two), None)),
        ),
        (
            "NEXT not the last key",
            ClearScope::All,
            Box::new(|_| page(Elements::Peers(&peer(1, 3)), Some(&[1, 7]))),
        ),
        (
            "NEXT with no element",
            ClearScope::All,
            Box::new(|_| page(Elements::None, Some(&[1, 7]))),
        ),
        (
            "a peer of another endpoint",
            ClearScope::EndpointAll(EP),
            Box::new(move |_| page(Elements::Peers(&peer(other.link, other.addr)), None)),
        ),
        (
            "a key at or below the cursor",
            ClearScope::All,
            Box::new(|q: &LinkDiagReq| {
                let at = q.cursor.map_or(vec![1, 3], |c| c.as_bytes().to_vec());
                page(Elements::Peers(&peer(at[0], at[1])), Some(&at))
            }),
        ),
    ];
    for (why, scope, forge) in cases {
        let r = read_back(scope, forge);
        assert!(matches!(r, Err(Error::BadResponse)), "{why}: {r:?}");
    }
}

/// Pages that always name one more peer and a NEXT end when the keys do:
/// the read-back stops within the 65536 keys a peer can have.
#[test]
fn an_endless_read_back_ends_with_the_keys() {
    let pages = Rc::new(RefCell::new(0u32));
    let count = pages.clone();
    let r = read_back(
        ClearScope::All,
        Box::new(move |q: &LinkDiagReq| {
            *count.borrow_mut() += 1;
            let k = q.cursor.map_or(0, |c| {
                u16::from_be_bytes([c.as_bytes()[0], c.as_bytes()[1]])
            });
            let [link, addr] = k.wrapping_add(1).to_be_bytes();
            page(Elements::Peers(&peer(link, addr)), Some(&[link, addr]))
        }),
    );
    assert!(matches!(r, Err(Error::BadResponse)), "{r:?}");
    assert!(*pages.borrow() <= 1 << 16);
}
