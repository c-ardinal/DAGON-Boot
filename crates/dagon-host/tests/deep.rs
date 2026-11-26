//! A node behind a relaying node that is itself linked: the host puts a
//! RELAY inside a RELAY for each hop, opens every relaying node on the way,
//! lets each wait longer than what it carries, and asks no more of a node
//! than every tunnel to it carries.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dagon_host::{Error, Hop, Host, HostClock, Reconnect, Silence, Target, Transport, MAX_HOPS};
use dagon_port::timeout::REQUEST_MS;
use dagon_proto::payload::{encode_bulk_resp, HelloReq, Relay};
use dagon_proto::{finish, flags, frame_len, max_payload, Cmd, Frame, Status};
use dagon_sim::block_on;

/// What one stand-in node saw of one request.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    cmd: u8,
    /// RELAY: the wait it was given.
    relay_wait_ms: Option<u16>,
    /// HELLO: the largest payload the host said it sends.
    hello_max: Option<u16>,
}

/// A stand-in node: it answers every request but RELAY with OK and its
/// name, and relays to the nodes behind it as the protocol says (an inner
/// frame its tunnel does not carry is refused, an address nothing answers
/// at times out).
struct Node {
    name: u8,
    /// The largest payload of the link the PC's side reaches this node on.
    ingress: usize,
    /// The nodes behind each (link, address), with that link's payload.
    behind: BTreeMap<(u8, u8), (usize, Node)>,
    seen: Rc<RefCell<Vec<(u8, Seen)>>>,
}

fn answer(request: &Frame<'_>, status: Status, body: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; frame_len(1 + body.len())];
    out[8] = status as u8;
    out[9..9 + body.len()].copy_from_slice(body);
    let n = finish(
        &mut out,
        flags::RESP,
        request.seq,
        request.cmd,
        1 + body.len(),
    )
    .unwrap();
    out.truncate(n);
    out
}

impl Node {
    fn handle(&mut self, frame: &[u8]) -> Vec<u8> {
        let (f, _) = Frame::decode(frame, self.ingress).expect("a whole frame of this link");
        let mut seen = Seen {
            cmd: f.cmd,
            relay_wait_ms: None,
            hello_max: None,
        };
        let out = if f.cmd == Cmd::Relay as u8 {
            let r = Relay::parse(f.payload).unwrap();
            seen.relay_wait_ms = r.timeout_ms;
            match self.behind.get_mut(&(r.link, r.addr)) {
                None => answer(&f, Status::Timeout, &[]),
                Some((egress, _)) if !r.admits(self.ingress, *egress) => {
                    answer(&f, Status::Arg, &[])
                }
                Some((_, node)) => {
                    let inner = node.handle(r.inner);
                    let mut body = vec![0u8; inner.len() + 8];
                    let n = encode_bulk_resp(&inner, &mut body).unwrap();
                    answer(&f, Status::Ok, &body[..n])
                }
            }
        } else {
            if f.cmd == Cmd::Hello as u8 {
                seen.hello_max = HelloReq::parse(f.payload).ok().map(|h| h.max_payload);
            }
            answer(&f, Status::Ok, &[self.name, f.cmd])
        };
        self.seen.borrow_mut().push((self.name, seen));
        out
    }
}

struct Link {
    hub: Node,
    pending: Vec<u8>,
}

impl Transport for Link {
    type Error = ();
    fn max_payload(&self) -> usize {
        max_payload::USB
    }
    fn reconnect_kind(&self) -> Reconnect {
        Reconnect::Stays
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        let out = self.hub.handle(bytes);
        self.pending.extend(out);
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

struct Clock(u64);

impl HostClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.0 += u64::from(ms);
    }
}

const UART: usize = max_payload::UART;
const I2C: usize = max_payload::I2C;
/// The hops from the PC: a UART link to node 1, its UART link to node 2,
/// that node's I2C bus to node 3.
const HOPS: [Hop; 3] = [
    Hop {
        link: 1,
        addr: 0xFF,
        max_payload: UART as u16,
    },
    Hop {
        link: 4,
        addr: 0xFF,
        max_payload: UART as u16,
    },
    Hop {
        link: 0,
        addr: 0x42,
        max_payload: I2C as u16,
    },
];

type Log = Rc<RefCell<Vec<(u8, Seen)>>>;

/// The connected node 0 with nodes 1, 2 and 3 in a chain behind it.
fn chain() -> (Host<Link, Clock>, Log) {
    let seen: Log = Rc::default();
    let node = |name, ingress, behind: Option<((u8, u8), (usize, Node))>| Node {
        name,
        ingress,
        behind: behind.into_iter().collect(),
        seen: seen.clone(),
    };
    let key = |h: Hop| (h.link, h.addr);
    let n3 = node(3, I2C, None);
    let n2 = node(2, UART, Some((key(HOPS[2]), (I2C, n3))));
    let n1 = node(1, UART, Some((key(HOPS[1]), (UART, n2))));
    let hub = node(0, max_payload::USB, Some((key(HOPS[0]), (UART, n1))));
    let link = Link {
        hub,
        pending: Vec::new(),
    };
    (Host::new(link, Clock(0)), seen)
}

fn to(hops: usize) -> Target {
    Target::through(&HOPS[..hops]).unwrap()
}

/// What node `name` saw, in order.
fn seen_by(log: &Log, name: u8) -> Vec<Seen> {
    log.borrow()
        .iter()
        .filter(|(n, _)| *n == name)
        .map(|(_, s)| s.clone())
        .collect()
}

#[test]
fn a_request_reaches_a_node_behind_two_relaying_nodes_and_its_answer_comes_back() {
    let (mut h, _) = chain();
    for hops in 0..=3 {
        let body = block_on(h.call(to(hops), Cmd::Status, &[], REQUEST_MS)).unwrap();
        assert_eq!(body, [hops as u8, Cmd::Status as u8], "{hops} hops");
    }
}

#[test]
fn every_relaying_node_on_the_way_is_opened_once_before_a_relay_goes_through_it() {
    let (mut h, log) = chain();
    block_on(h.call(to(3), Cmd::Status, &[], REQUEST_MS)).unwrap();
    block_on(h.call(to(3), Cmd::Status, &[], REQUEST_MS)).unwrap();
    let cmds = |name| -> Vec<u8> { seen_by(&log, name).iter().map(|s| s.cmd).collect() };
    let (hello, relay, status) = (Cmd::Hello as u8, Cmd::Relay as u8, Cmd::Status as u8);
    // Node 1 answers HELLO before anything goes through it; so does node 2.
    assert_eq!(cmds(1), [hello, relay, relay, relay, relay]);
    assert_eq!(cmds(2), [hello, relay, relay, relay]);
    assert_eq!(cmds(3), [hello, status, status]);
}

#[test]
fn each_relay_waits_one_request_longer_than_what_it_carries() {
    let (mut h, log) = chain();
    let wait = 2_000;
    block_on(h.call(to(3), Cmd::Status, &[], wait)).unwrap();
    let last_wait = |name| {
        seen_by(&log, name)
            .last()
            .and_then(|s| s.relay_wait_ms)
            .map(u32::from)
    };
    assert_eq!(last_wait(2), Some(wait));
    assert_eq!(last_wait(1), Some(wait + REQUEST_MS));
    assert_eq!(last_wait(0), Some(wait + 2 * REQUEST_MS));
}

#[test]
fn a_node_is_told_the_largest_payload_every_tunnel_to_it_carries() {
    let (mut h, log) = chain();
    block_on(h.call(to(3), Cmd::Status, &[], REQUEST_MS)).unwrap();
    let told = |name| seen_by(&log, name)[0].hello_max.map(usize::from).unwrap();
    // Each tunnel carries one frame of its own link and no more than a
    // RELAY carries on the tunnel before it.
    let through = |up, egress| Relay::tunnel_mtu(up, egress).saturating_sub(frame_len(0));
    let one = through(max_payload::USB, UART);
    let two = through(one, UART);
    let three = through(two, I2C);
    assert_eq!((told(1), told(2), told(3)), (one, two, three));
    assert!(three < two && two < one, "{one} {two} {three}");
    // A request of that size goes through; the relaying nodes refuse none.
    let body = block_on(h.call(to(3), Cmd::Status, &vec![0u8; three], REQUEST_MS)).unwrap();
    assert_eq!(body[0], 3);
}

#[test]
fn a_silent_deep_node_is_the_timeout_of_the_relaying_node_before_it() {
    let (mut h, log) = chain();
    let mut hops = HOPS;
    hops[2].addr = 0x43;
    let nobody = Target::through(&hops).unwrap();
    let e = block_on(h.call(nobody, Cmd::Status, &[], REQUEST_MS)).unwrap_err();
    assert!(matches!(e, Error::Relay(Status::Timeout)), "{e:?}");
    // Its history is asked of the node it is linked to, not of the
    // connected one (the stand-in's answer is no record at all).
    log.borrow_mut().clear();
    assert_eq!(block_on(h.silence(nobody)).unwrap(), Silence::HistoryUnknown);
    let peers = Cmd::Peers as u8;
    assert!(seen_by(&log, 2).iter().any(|s| s.cmd == peers));
    assert!(seen_by(&log, 0).iter().all(|s| s.cmd != peers));
}

#[test]
fn a_target_is_its_hops_and_knows_the_node_that_reaches_it() {
    assert_eq!(Target::through(&[]), Some(Target::Local));
    assert_eq!(
        to(1),
        Target::Linked {
            link: 1,
            addr: 0xFF,
            max_payload: UART as u16
        }
    );
    for hops in 0..=3 {
        assert_eq!(to(hops).hops(), HOPS[..hops]);
    }
    assert_eq!(to(0).parent(), None);
    assert_eq!(to(1).parent(), Some((Target::Local, HOPS[0])));
    assert_eq!(to(3).parent(), Some((to(2), HOPS[2])));
    // Equal hops are one target; no target is further than MAX_HOPS.
    assert_eq!(to(3), Target::through(&HOPS).unwrap());
    assert!(Target::through(&[HOPS[0]; MAX_HOPS]).is_some());
    assert_eq!(Target::through(&[HOPS[0]; MAX_HOPS + 1]), None);
}
