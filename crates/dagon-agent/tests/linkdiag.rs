//! LINK_DIAG and the DTC scopes of LINK_DIAG_CLEAR at an agent on a fake
//! store: what a request may ask, the bytes of the elements, paging by
//! key and the size of an answer.

mod diag_bench;

use dagon_proto::diag::*;
use dagon_proto::table::link_diag::{report, store, FORMAT};
use dagon_proto::table::{dtc, link_diag};
use dagon_proto::{Cmd, Status};
use diag_bench::*;

fn ask(n: &mut Node<Counter>, q: &LinkDiagReq) -> (Status, Vec<u8>) {
    n.call(Cmd::LinkDiag, &enc(|b| q.encode(b)))
}

fn peers_req(cursor: Option<Key>, max_resp: u16) -> LinkDiagReq {
    LinkDiagReq {
        report: Report::Peers,
        endpoint: None,
        cursor,
        max_resp,
    }
}

fn entry(dtc: u32, link: u8, addr: u8, status: u8) -> Entry {
    Entry {
        dtc,
        link,
        addr,
        status,
        fdc: 0,
    }
}

#[test]
fn report_rules() {
    let (mut n, _) = counter_node([1, 2, 3, 4]);
    let raw = |n: &mut Node<Counter>, p: &[u8]| n.call(Cmd::LinkDiag, p).0;
    // MAX_RESP 200 is field 6: key 0x0D, length 2.
    let max = [0x0D, 2, 200, 0];
    let report3 = [&[3][..], &max].concat();
    assert_eq!(raw(&mut n, &report3), Status::Unsupported);
    let mask = [0x03, 1, 0xFF];
    let report2_mask = [&[report::EVER_COMMUNICATED][..], &mask, &max].concat();
    assert_eq!(raw(&mut n, &report2_mask), Status::Arg);
    let report1_no_mask = [&[report::DTC_BY_STATUS_MASK][..], &max].concat();
    assert_eq!(raw(&mut n, &report1_no_mask), Status::Arg);
    let link_only = [&[report::EVER_COMMUNICATED][..], &[0x07, 1, 1], &max].concat();
    assert_eq!(raw(&mut n, &link_only), Status::Arg);
    assert_eq!(
        raw(&mut n, &[report::EVER_COMMUNICATED]),
        Status::Arg,
        "no MAX_RESP"
    );
    let ok = [&[report::EVER_COMMUNICATED][..], &max].concat();
    assert_eq!(raw(&mut n, &ok), Status::Ok);
}

#[test]
fn iso_encodings() {
    let e = Entry {
        dtc: 0x12_3456,
        link: 1,
        addr: 2,
        status: 0x09,
        fdc: -1,
    };
    let b = e.to_bytes();
    assert_eq!(&b[..3], &[0x12, 0x34, 0x56], "DTC high byte first");
    assert_eq!(b[6], 0xFF, "FDC -1 in two's complement");
    assert_eq!(Entry::from_bytes(b), e);
    for (fdc, byte) in [(-128i8, 0x80), (127, 0x7F), (0, 0)] {
        assert_eq!(Entry { fdc, ..e }.to_bytes()[6], byte);
    }
    // The same bytes reach the wire.
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().add_entry(e);
    let q = LinkDiagReq {
        report: Report::Dtcs {
            mask: 0xFF,
            dtc: None,
        },
        endpoint: None,
        cursor: None,
        max_resp: 300,
    };
    let (st, body) = ask(&mut n, &q);
    assert_eq!(st, Status::Ok);
    let r = LinkDiagResp::parse(&body).unwrap();
    assert_eq!(r.elements, Elements::Entries(&b));
}

#[test]
fn element_layouts_golden() {
    let e = Entry {
        dtc: dtc::PEER_NOT_ANSWERING,
        link: 3,
        addr: 0x42,
        status: 0x2F,
        fdc: 127,
    };
    assert_eq!(e.to_bytes(), [0x00, 0x00, 0x01, 0x03, 0x42, 0x2F, 0x7F]);
    let absent = Peer {
        link: 3,
        addr: 0x42,
        instance: NO_INSTANCE,
    };
    let mut want = vec![0x03, 0x42];
    want.extend([0; 16]);
    assert_eq!(absent.to_bytes().to_vec(), want);
    assert_eq!(absent.instance_id(), None);
    let id: [u8; 16] = core::array::from_fn(|i| i as u8 + 1);
    let known = Peer {
        instance: id,
        ..absent
    };
    assert_eq!(&known.to_bytes()[2..], &id);
    assert_eq!(known.instance_id(), Some(id));
    // STORE: each bit where retained-formats and the table put it.
    assert_eq!(
        [
            store::RETENTION_NONE,
            store::RETENTION_RESET,
            store::RETENTION_POWER_CUT,
            store::FULL,
            store::LOST,
            store::UNKNOWN_FORMAT,
            store::RESERVED,
        ],
        [0, 1, 2, 0x04, 0x08, 0x10, 0xE0]
    );
    // PEERS is in key order, then NEXT; the response's bytes.
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().peers.insert(known.key(), known);
    let (st, body) = ask(&mut n, &peers_req(None, 300));
    assert_eq!(st, Status::Ok);
    let mut golden = vec![FORMAT, 0x02, 1, 0x7F, 0x04, 1, 0x02, 0x06, 1, 0x0E];
    golden.extend([0x08, 3, 32, 1, 40, 0x0C, 18, 0x03, 0x42]);
    golden.extend(id);
    assert_eq!(body, golden);
}

#[test]
fn paging_never_repeats_or_skips() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    for i in 0..40u8 {
        fake.0.borrow_mut().add_peer(1 + i / 20, i);
    }
    let throughout: Vec<Key> = fake.0.borrow().peers.keys().copied().collect();
    // Every page holds two peers at most.
    let max = (min_resp(report::EVER_COMMUNICATED) + Peer::LEN) as u16;
    let mut seen = Vec::new();
    let mut cursor = None;
    for page in 0.. {
        let (st, body) = ask(&mut n, &peers_req(cursor, max));
        assert_eq!(st, Status::Ok, "page {page}");
        assert!(body.len() < usize::from(max));
        let r = LinkDiagResp::parse(&body).unwrap();
        let got: Vec<Key> = r.elements.peers().map(|p| p.key()).collect();
        assert!(!got.is_empty() && got.windows(2).all(|w| w[0] < w[1]));
        seen.extend(got.iter().copied());
        // Between pages: one peer is deleted, one is inserted, around it.
        let mut s = fake.0.borrow_mut();
        if page == 3 {
            let gone = *s.peers.keys().nth(30).unwrap();
            s.peers.remove(&gone);
            s.add_peer(0, 0x77);
            s.add_peer(9, 0x01);
        }
        drop(s);
        match r.next {
            Some(k) => {
                assert_eq!(Some(&k), got.last());
                cursor = Some(k);
            }
            None => break,
        }
    }
    let deleted = throughout[30];
    for k in &throughout {
        let times = seen.iter().filter(|s| *s == k).count();
        assert_eq!(times, usize::from(*k != deleted), "{k:?}");
    }
    assert!(
        seen.contains(&Key::new(&[9, 0x01]).unwrap()),
        "inserted after the cursor"
    );
    // A cursor of the other report's length.
    let bad = [report::EVER_COMMUNICATED, 0x0B, 3, 1, 2, 3, 0x0D, 2, 100, 0];
    assert_eq!(n.call(Cmd::LinkDiag, &bad).0, Status::Arg);
}

/// LINK_DIAG is a read: a resend (same seq, RETRY) is read again, and
/// it leaves another command's kept reply alone.
#[test]
fn a_resend_is_read_again() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    let ask = enc(|b| peers_req(None, 300).encode(b));
    let first = n.send(5, 0, Cmd::LinkDiag, &ask);
    assert_eq!(first.0, Status::Ok);
    fake.0.borrow_mut().add_peer(1, 9);
    let again = n.send(5, dagon_proto::flags::RETRY, Cmd::LinkDiag, &ask);
    assert_eq!(again.0, Status::Ok);
    let r = LinkDiagResp::parse(&again.1).unwrap();
    assert_eq!(r.elements.peers().count(), 1, "read again");
    assert_ne!(first.1, again.1);
    // A refused read is answered in the same room.
    assert_eq!(n.send(6, 0, Cmd::LinkDiag, &[2]).0, Status::Arg);
}

#[test]
fn max_resp_bounds() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    for i in 0..60u8 {
        fake.0.borrow_mut().add_peer(1, i);
        fake.0
            .borrow_mut()
            .add_entry(entry(dtc::PEER_NOT_ANSWERING, 1, i, 0x09));
    }
    let dtcs = |max_resp| LinkDiagReq {
        report: Report::Dtcs {
            mask: 0x01,
            dtc: None,
        },
        endpoint: None,
        cursor: None,
        max_resp,
    };
    for (q, elem) in [(dtcs(0), Entry::LEN), (peers_req(None, 0), Peer::LEN)] {
        let least = q.report.min_resp() as u16;
        let below = LinkDiagReq {
            max_resp: least - 1,
            ..q
        };
        assert_eq!(ask(&mut n, &below).0, Status::Range);
        let (st, body) = ask(
            &mut n,
            &LinkDiagReq {
                max_resp: least,
                ..q
            },
        );
        assert_eq!(st, Status::Ok);
        assert_eq!(1 + body.len(), usize::from(least), "one element and NEXT");
        let r = LinkDiagResp::parse(&body).unwrap();
        assert!(r.next.is_some());
        let (st, body) = ask(
            &mut n,
            &LinkDiagReq {
                max_resp: 1000,
                ..q
            },
        );
        assert_eq!(st, Status::Ok);
        let r = LinkDiagResp::parse(&body).unwrap();
        let count = match r.elements {
            Elements::Entries(b) | Elements::Peers(b) => b.len() / elem,
            Elements::None => 0,
        };
        assert_eq!(count, 252 / elem, "a field of 252 bytes at most");
        assert!(r.next.is_some());
    }
    assert_eq!(252 / Entry::LEN, 36);
    assert_eq!(252 / Peer::LEN, 14);
    // The link the request came over bounds it too.
    assert!(LinkDiagResp::MAX_LEN < usize::from(config().max_payload));
    let _ = link_diag::ENTRIES;
}

#[test]
fn clear_scopes() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    let other = Endpoint { link: 2, addr: 7 };
    for at in [EP, other] {
        fake.0.borrow_mut().add_peer(at.link, at.addr);
        fake.0
            .borrow_mut()
            .add_entry(entry(dtc::PEER_NOT_ANSWERING, at.link, at.addr, 0x09));
    }
    assert_eq!(n.call(Cmd::LinkDiagClear, &[4]).0, Status::Unsupported);
    let with_link = [3, 0x05, 1, 1, 0x07, 1, 0x42];
    assert_eq!(n.call(Cmd::LinkDiagClear, &with_link).0, Status::Arg);
    assert_eq!(
        n.call(Cmd::LinkDiagClear, &[1, 0x05, 1, 1, 0x07, 1, 0x42])
            .0,
        Status::Arg
    );
    assert_eq!(n.clear(ClearScope::EndpointDtcs(EP), None), Status::Ok);
    let s = fake.0.borrow();
    assert_eq!(s.clears, [ClearScope::EndpointDtcs(EP)]);
    assert_eq!(s.peers.len(), 2, "ever-communicated kept");
    let left: Vec<Endpoint> = s.entries.values().map(|e| e.endpoint()).collect();
    assert_eq!(left, [other]);
}
