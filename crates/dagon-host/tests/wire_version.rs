//! A peer that reads another wire version refuses the request frame; the
//! host reports that version, directly and through RELAY, and still
//! reports a peer that says nothing as a timeout.

use dagon_host::{Error, Host, HostClock, Reconnect, Target, Transport};
use dagon_proto::payload::{encode_bulk_resp, Relay};
use dagon_proto::{finish, flags, frame_len, max_payload, Cmd, Frame, Status, PROTO_VERSION};
use dagon_sim::block_on;

const THEIRS: u8 = PROTO_VERSION + 1;

/// The refusal a peer of version [`THEIRS`] sends for `request`: in its own
/// version, echoing seq and cmd where every version keeps them.
fn refusal(request: &Frame<'_>) -> Vec<u8> {
    let mut out = vec![0u8; frame_len(1)];
    out[8] = Status::Frame as u8;
    finish(&mut out, flags::RESP, request.seq, request.cmd, 1).unwrap();
    out[2] = THEIRS;
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Peer {
    /// The node on the cable reads another version.
    Hub,
    /// The hub relays; the node behind it reads another version.
    Node,
    Silent,
}

struct Link {
    peer: Peer,
    pending: Vec<u8>,
    now: u64,
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
        let (f, _) = Frame::decode(bytes, max_payload::USB).unwrap();
        match self.peer {
            Peer::Hub => self.pending.extend(refusal(&f)),
            Peer::Node => {
                let r = Relay::parse(f.payload).unwrap();
                let (inner, _) = Frame::decode(r.inner, max_payload::USB).unwrap();
                let mut out = vec![0u8; frame_len(64)];
                out[8] = Status::Ok as u8;
                let n = encode_bulk_resp(&refusal(&inner), &mut out[9..]).unwrap();
                let len = finish(&mut out, flags::RESP, f.seq, f.cmd, 1 + n).unwrap();
                self.pending.extend(&out[..len]);
            }
            Peer::Silent => {}
        }
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

impl HostClock for Link {
    fn now_ms(&self) -> u64 {
        self.now
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.now += u64::from(ms);
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

fn host(peer: Peer) -> Host<Link, Clock> {
    let link = Link {
        peer,
        pending: Vec::new(),
        now: 0,
    };
    Host::new(link, Clock(0))
}

fn linked() -> Target {
    // Any link and address: the stand-in hub relays whatever it is given.
    let (link, addr) = (7, 0x42);
    Target::Linked {
        link,
        addr,
        max_payload: max_payload::I2C as u16,
    }
}

#[test]
fn a_hub_of_another_wire_version_is_named_not_timed_out() {
    let r = block_on(host(Peer::Hub).hello(Target::Local));
    assert!(
        matches!(r, Err(Error::WireVersion { peer: THEIRS })),
        "{r:?}"
    );
    let r = block_on(host(Peer::Hub).call(Target::Local, Cmd::Status, &[], 300));
    assert!(
        matches!(r, Err(Error::WireVersion { peer: THEIRS })),
        "{r:?}"
    );
}

#[test]
fn a_relayed_node_of_another_wire_version_is_named_too() {
    let r = block_on(host(Peer::Node).hello(linked()));
    assert!(
        matches!(r, Err(Error::WireVersion { peer: THEIRS })),
        "{r:?}"
    );
}

#[test]
fn a_peer_that_says_nothing_is_still_a_timeout() {
    let r = block_on(host(Peer::Silent).hello(Target::Local));
    assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
}
