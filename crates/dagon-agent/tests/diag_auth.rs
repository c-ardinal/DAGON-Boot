//! Clears of link diagnostics and their seed and key exchange, after UDS
//! SecurityAccess (0x27): a DTC clear needs no key; a clear of
//! ever-communicated records needs the signature of the project's key
//! over a seed the node issued and holds, used once.

mod diag_bench;

use dagon_format::SIGN_DOMAINS;
use dagon_port::diag::{DiagStore, NoDiag, NoSeed, Sample};
use dagon_proto::diag::*;
use dagon_proto::payload::HelloResp;
use dagon_proto::{Cmd, Status};
use diag_bench::*;

fn hello_cmds<Q: dagon_port::diag::SeedSource, D: DiagStore>(n: &mut Node<Q, D>) -> Vec<u8> {
    let ask = enc(|b| {
        dagon_proto::payload::HelloReq {
            proto_ver: 2,
            max_payload: 1088,
        }
        .encode(b)
    });
    let (st, body) = n.call(Cmd::Hello, &ask);
    assert_eq!(st, Status::Ok);
    HelloResp::parse(&body).unwrap().cmds.iter().collect()
}

const DIAG_CMDS: [Cmd; 3] = [Cmd::LinkDiag, Cmd::LinkDiagClear, Cmd::DiagSeed];

#[test]
fn default_ports_unsupported() {
    // `Agent::new` gives the defaults; `with_diag` of the same is the
    // same type.
    let _: Plain = plain().with_diag(NoSeed, NoDiag);
    let mut node = Node::new(NoSeed, NoDiag);
    for cmd in DIAG_CMDS {
        let (st, _) = node.call(cmd, &[1]);
        assert_eq!(st, Status::Unsupported, "{cmd:?}");
    }
    let cmds = hello_cmds(&mut node);
    for cmd in DIAG_CMDS {
        assert!(!cmds.contains(&(cmd as u8)), "{cmd:?} named in HELLO");
    }
    // A store without seeds serves the reads and the DTC clears.
    let (mut n, _) = seedless();
    assert_eq!(n.seed(), Err(Status::Unsupported));
    assert_eq!(n.clear(ClearScope::AllDtcs, None), Status::Ok);
    let cmds = hello_cmds(&mut n);
    assert!(cmds.contains(&(Cmd::LinkDiag as u8)) && !cmds.contains(&(Cmd::DiagSeed as u8)));
    // Seeds without a store: nothing is served.
    let mut s = storeless();
    for cmd in DIAG_CMDS {
        assert_eq!(s.call(cmd, &[1]).0, Status::Unsupported, "{cmd:?}");
    }
    let (mut full, _) = counter_node([1, 2, 3, 4]);
    let cmds = hello_cmds(&mut full);
    for cmd in DIAG_CMDS {
        assert!(cmds.contains(&(cmd as u8)), "{cmd:?} not named");
    }
}

#[test]
fn dtc_clear_needs_no_key() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    assert_eq!(n.clear(ClearScope::AllDtcs, None), Status::Ok);
    assert_eq!(fake.0.borrow().clears, [ClearScope::AllDtcs]);
}

#[test]
fn seed_key_success() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    fake.0.borrow_mut().add_peer(2, 0x10);
    let seed = n.seed().unwrap();
    let scope = ClearScope::EndpointAll(EP);
    let key = sign(&keypair(), &seed, &scope);
    assert_eq!(n.clear(scope, Some(key)), Status::Ok);
    let s = fake.0.borrow();
    assert_eq!(s.clears, [scope], "once, for that endpoint");
    assert_eq!(s.peers.len(), 1, "the other peer is kept");
}

#[test]
fn domain_is_distinct() {
    for (i, a) in SIGN_DOMAINS.iter().enumerate() {
        for b in &SIGN_DOMAINS[i + 1..] {
            assert_ne!(a, b);
            assert!(!a.starts_with(b) && !b.starts_with(a), "{a:?} {b:?}");
        }
    }
    assert!(SIGN_DOMAINS.contains(&dagon_format::DIAG_CLEAR_DOMAIN));
    assert!(SIGN_DOMAINS.contains(&dagon_format::header::SIGN_DOMAIN));
    assert!(SIGN_DOMAINS.contains(&dagon_format::manifest::SIGN_DOMAIN));
}

/// A signed clear with no seed held.
fn key_gets_state<Q: dagon_port::diag::SeedSource>(n: &mut Node<Q>) {
    let scope = ClearScope::All;
    assert_eq!(n.clear(scope, Some([0x11; 64])), Status::State);
}

#[test]
fn seed_errors() {
    // No seed source.
    let (mut n, _) = seedless();
    assert_eq!(n.seed(), Err(Status::Unsupported));
    key_gets_state(&mut n);
    // The random driver fails.
    let (mut n, _) = random_node(vec![]);
    assert_eq!(n.seed(), Err(Status::Unsupported));
    key_gets_state(&mut n);
    // The factory ID cannot be read: no counter value is spent.
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    n.seed().unwrap();
    let id = Id(std::rc::Rc::new(std::cell::RefCell::new(None)));
    let mut unreadable = Node::new(
        dagon_port::diag::CounterSeed { mcu_id: MCU, id },
        fake.clone(),
    );
    let before = fake.0.borrow().counter;
    assert_eq!(unreadable.seed(), Err(Status::Unsupported));
    assert_eq!(fake.0.borrow().counter, before);
    key_gets_state(&mut unreadable);
    // The counter is at its end: it never wraps.
    fake.0.borrow_mut().counter = u32::MAX;
    assert_eq!(n.seed(), Err(Status::Unsupported));
    key_gets_state(&mut n);
    // The counter's append fails.
    fake.0.borrow_mut().counter = 5;
    fake.0.borrow_mut().cut_counter = Some(Cut::Before);
    assert_eq!(n.seed(), Err(Status::Flash));
    key_gets_state(&mut n);
}

/// The random source claims only the birthday bound: a driver that
/// repeats itself repeats a seed, and a request captured under the first
/// is taken under the second. The docs state the bound, not uniqueness.
#[test]
fn random_repeat_is_the_documented_limit() {
    let (mut n, fake) = random_node(vec![[9; 16], [9; 16]]);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let first = n.seed().unwrap();
    let scope = ClearScope::EndpointAll(EP);
    let captured = sign(&keypair(), &first, &scope);
    let second = n.seed().unwrap();
    assert_eq!(first, second);
    assert_eq!(n.clear(scope, Some(captured)), Status::Ok);
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
    for page in [
        "site/src/content/docs/explanation/retained-formats.md",
        "site/src/content/docs/en/explanation/retained-formats.md",
    ] {
        let text = std::fs::read_to_string(format!("{root}{page}")).unwrap();
        assert!(text.contains("n(n-1)/2^129"), "{page} states the bound");
    }
}

#[test]
fn counter_seed_injective() {
    // Exhaustive on 1-byte widths of the counter, `mcu_id` and ID bytes
    // that vary.
    let mut seen = std::collections::HashSet::new();
    for counter in 0..=255u32 {
        for mcu in 0..=3u16 {
            for id in 0..=255u8 {
                let s = Seed::counter(counter, mcu, &[id]).unwrap();
                assert_eq!(s.as_bytes().len(), 7);
                assert!(seen.insert(s.as_bytes().to_vec()), "{counter} {mcu} {id}");
            }
        }
    }
    // Full widths: seeds that differ in one part only differ.
    let mut x = 0x9E37_79B9u32;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    for width in 1..=16usize {
        for _ in 0..200 {
            let id: Vec<u8> = (0..width).map(|_| next() as u8).collect();
            let (c, m) = (next(), next() as u16);
            let base = Seed::counter(c, m, &id).unwrap();
            assert_eq!(base.as_bytes().len(), 6 + width);
            let mut other = id.clone();
            other[next() as usize % width] ^= 1 << (next() % 8);
            for s in [
                Seed::counter(c ^ (1 << (next() % 32)), m, &id),
                Seed::counter(c, m ^ (1 << (next() % 16)), &id),
                Seed::counter(c, m, &other),
            ] {
                assert_ne!(s.unwrap(), base);
            }
        }
    }
    assert!(Seed::counter(1, 1, &[]).is_none() && Seed::counter(1, 1, &[0; 17]).is_none());
    // A 6-byte SEED is refused.
    let six = [0x02, 6, 1, 0, 0, 0, 2, 0];
    assert_eq!(Seed::parse(&six), Err(Status::Arg));
    assert!(Seed::new(&[0; 6]).is_none() && Seed::new(&[0; 23]).is_none());
}

#[test]
fn seed_after_reset() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let before: Vec<Seed> = (0..3).map(|_| n.seed().unwrap()).collect();
    let scope = ClearScope::EndpointAll(EP);
    let key = sign(&keypair(), before.last().unwrap(), &scope);
    n.agent.restart();
    let after = n.seed().unwrap();
    assert!(!before.contains(&after));
    // The old seed is gone with the reset (the new one is outstanding).
    assert_eq!(n.clear(scope, Some(key)), Status::Signature);
    let fresh = n.seed().unwrap();
    n.agent.restart();
    let key = sign(&keypair(), &fresh, &scope);
    assert_eq!(n.clear(scope, Some(key)), Status::State);
    assert_eq!(fake.0.borrow().peers.len(), 1);
}

#[test]
fn two_nodes_differ() {
    let (mut a, fa) = counter_node([1, 2, 3, 4]);
    let (mut b, fb) = counter_node([1, 2, 3, 5]);
    fa.0.borrow_mut().add_peer(EP.link, EP.addr);
    fb.0.borrow_mut().add_peer(EP.link, EP.addr);
    let (sa, sb) = (a.seed().unwrap(), b.seed().unwrap());
    assert_eq!((fa.0.borrow().counter, fb.0.borrow().counter), (1, 1));
    assert_ne!(sa, sb);
    let scope = ClearScope::EndpointAll(EP);
    assert_eq!(
        b.clear(scope, Some(sign(&keypair(), &sa, &scope))),
        Status::Signature
    );
    assert_eq!(fb.0.borrow().peers.len(), 1);
}

#[test]
fn failure_and_replay() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let scope = ClearScope::EndpointAll(EP);
    // A wrong signature voids the seed: the right one is then too late.
    let seed = n.seed().unwrap();
    assert_eq!(
        n.clear(scope, Some(sign(&foreign(), &seed, &scope))),
        Status::Signature
    );
    assert_eq!(
        n.clear(scope, Some(sign(&keypair(), &seed, &scope))),
        Status::State
    );
    // A request signed under a seed since replaced, never sent: a bad
    // signature for the seed held.
    let seed = n.seed().unwrap();
    let stale = sign(&keypair(), &seed, &scope);
    let _ = n.seed().unwrap();
    assert_eq!(n.clear(scope, Some(stale)), Status::Signature);
    // A KEY with no seed held.
    assert_eq!(n.clear(scope, Some(stale)), Status::State);
    // A signature for another scope under the right seed.
    let seed = n.seed().unwrap();
    let other = sign(&keypair(), &seed, &ClearScope::All);
    assert_eq!(n.clear(scope, Some(other)), Status::Signature);
    assert!(fake.0.borrow().clears.is_empty());
    assert_eq!(fake.0.borrow().peers.len(), 1);
    // A good request captured on the wire, then a new seed: the capture
    // is a bad signature for the new seed, which it voids.
    let seed = n.seed().unwrap();
    let captured = sign(&keypair(), &seed, &scope);
    assert_eq!(n.clear(scope, Some(captured)), Status::Ok);
    let seed = n.seed().unwrap();
    assert_eq!(n.clear(scope, Some(captured)), Status::Signature);
    let all = ClearScope::All;
    assert_eq!(
        n.clear(all, Some(sign(&keypair(), &seed, &all))),
        Status::State
    );
    assert_eq!(fake.0.borrow().clears, [scope]);
}

/// A seed asked for over one link ends what another link keeps of its
/// accepted clear: its resend, the same seq and bytes, is checked again
/// against the new seed, gets E_SIGNATURE and voids that seed.
#[test]
fn a_seed_on_one_link_ends_a_clear_kept_on_another() {
    let fake = Fake::default();
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let seeds = dagon_port::diag::CounterSeed {
        mcu_id: MCU,
        id: Id(std::rc::Rc::new(std::cell::RefCell::new(Some([
            1, 2, 3, 4,
        ])))),
    };
    let mut agent: TwoLinks = dagon_agent::Agent::with_links(config(), verifier(), Slots, chain())
        .with_diag(seeds, fake.clone());
    let send = |agent: &mut TwoLinks, on: usize, seq: u8, fl: u8, cmd: Cmd, p: &[u8]| {
        let mut buf = vec![0u8; dagon_proto::frame_len(p.len())];
        let n = dagon_proto::Frame {
            flags: fl,
            seq,
            cmd: cmd as u8,
            payload: p,
        }
        .encode(&mut buf)
        .unwrap();
        let reply = agent.handle_on(on, &buf[..n]).unwrap();
        let (f, _) = dagon_proto::Frame::decode(reply.frame, 1088).unwrap();
        let (st, body) = dagon_proto::split_status(f.payload).unwrap();
        (st, body.to_vec())
    };
    let ask = enc(|b| DiagSeedReq { level: 1 }.encode(b));
    let (st, body) = send(&mut agent, 0, 1, 0, Cmd::DiagSeed, &ask);
    assert_eq!(st, Status::Ok);
    let seed = Seed::parse(&body).unwrap();
    let scope = ClearScope::EndpointAll(EP);
    let req = clear_req(scope, Some(sign(&keypair(), &seed, &scope)));
    assert_eq!(
        send(&mut agent, 0, 2, 0, Cmd::LinkDiagClear, &req).0,
        Status::Ok
    );
    // Kept for a resend on link 0 until a seed is asked for anywhere.
    let retry = dagon_proto::flags::RETRY;
    assert_eq!(
        send(&mut agent, 0, 2, retry, Cmd::LinkDiagClear, &req).0,
        Status::Ok
    );
    let (st, body) = send(&mut agent, 1, 7, 0, Cmd::DiagSeed, &ask);
    assert_eq!(st, Status::Ok);
    let fresh = Seed::parse(&body).unwrap();
    assert_eq!(
        send(&mut agent, 0, 2, retry, Cmd::LinkDiagClear, &req).0,
        Status::Signature
    );
    let good = clear_req(scope, Some(sign(&keypair(), &fresh, &scope)));
    assert_eq!(
        send(&mut agent, 1, 8, 0, Cmd::LinkDiagClear, &good).0,
        Status::State
    );
    assert_eq!(fake.0.borrow().clears, [scope]);
}

type TwoLinks = dagon_agent::Agent<
    Slots,
    Chain,
    dagon_ed25519::Ed25519Compact,
    dagon_agent::writer::App,
    2,
    dagon_port::diag::CounterSeed<Id>,
    Fake,
>;

/// Only the same bytes again are the accepted request: the same clear
/// with one more field, even one the node skips, is another request under
/// a used seed. Fields in another order are no request at all.
#[test]
fn retry_of_other_bytes_gets_state() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let seed = n.seed().unwrap();
    let scope = ClearScope::EndpointAll(EP);
    let req = clear_req(scope, Some(sign(&keypair(), &seed, &scope)));
    assert_eq!(n.call(Cmd::LinkDiagClear, &req).0, Status::Ok);
    // Optional field 300 (key D8 04), empty, after the KEY.
    let more = [&req[..], &[0xD8, 0x04, 0]].concat();
    assert_eq!(n.call(Cmd::LinkDiagClear, &more).0, Status::State);
    // ADDR (3 bytes) before LINK (3 bytes), right after the core.
    let swapped = [&req[..1], &req[4..7], &req[1..4], &req[7..]].concat();
    assert_eq!(n.call(Cmd::LinkDiagClear, &swapped).0, Status::Arg);
    assert_eq!(n.call(Cmd::LinkDiagClear, &req).0, Status::Ok);
    assert_eq!(fake.0.borrow().clears.len(), 1);
}

#[test]
fn retry_answers_as_first() {
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    let seed = n.seed().unwrap();
    let scope = ClearScope::EndpointAll(EP);
    let req = clear_req(scope, Some(sign(&keypair(), &seed, &scope)));
    assert_eq!(n.call(Cmd::LinkDiagClear, &req).0, Status::Ok);
    // The peer answers again before the resend arrives.
    let mut store = fake.clone();
    store
        .record(Sample {
            link: EP.link,
            addr: EP.addr,
            pass: true,
        })
        .unwrap();
    // A resend of another session (another seq): answered as the first,
    // and nothing cleared again.
    assert_eq!(n.call(Cmd::LinkDiagClear, &req).0, Status::Ok);
    // The same seq with the RETRY flag: the stored reply.
    assert_eq!(fake.0.borrow().clears.len(), 1);
    assert_eq!(fake.0.borrow().peers.len(), 1, "the new record survives");
    // Anything else under the used seed.
    let other = clear_req(
        ClearScope::All,
        Some(sign(&keypair(), &seed, &ClearScope::All)),
    );
    assert_eq!(n.call(Cmd::LinkDiagClear, &other).0, Status::State);
    // A new seed ends the answer kept for it: sent again, the request is
    // a bad signature for the new seed, which it voids.
    let seed = n.seed().unwrap();
    assert_eq!(n.call(Cmd::LinkDiagClear, &req).0, Status::Signature);
    let good = clear_req(scope, Some(sign(&keypair(), &seed, &scope)));
    assert_eq!(n.call(Cmd::LinkDiagClear, &good).0, Status::State);
    assert_eq!(fake.0.borrow().clears.len(), 1);
}

#[test]
fn power_cut_at_agent() {
    let scope = ClearScope::EndpointAll(EP);
    // Cut before the counter commits: no seed, the value is not spent.
    let (mut n, fake) = counter_node([1, 2, 3, 4]);
    fake.0.borrow_mut().cut_counter = Some(Cut::Before);
    assert_eq!(n.seed(), Err(Status::Flash));
    n.agent.restart();
    fake.0.borrow_mut().cut_counter = None;
    key_gets_state(&mut n);
    assert_eq!(fake.0.borrow().counter, 0);
    // Cut after it commits: that value is burned, the next seed is new.
    fake.0.borrow_mut().cut_counter = Some(Cut::After);
    assert_eq!(n.seed(), Err(Status::Flash));
    n.agent.restart();
    fake.0.borrow_mut().cut_counter = None;
    key_gets_state(&mut n);
    let next = n.seed().unwrap();
    assert_eq!(next, Seed::counter(2, MCU, &[1, 2, 3, 4]).unwrap());
    // Cut before the clear commits: the records stay; no seed after.
    fake.0.borrow_mut().add_peer(EP.link, EP.addr);
    fake.0.borrow_mut().cut_clear = Some(Cut::Before);
    assert_eq!(
        n.clear(scope, Some(sign(&keypair(), &next, &scope))),
        Status::Flash
    );
    n.agent.restart();
    assert_eq!(fake.0.borrow().peers.len(), 1);
    key_gets_state(&mut n);
    // Cut after it commits: the records are gone; no seed after.
    let seed = n.seed().unwrap();
    fake.0.borrow_mut().cut_clear = Some(Cut::After);
    assert_eq!(
        n.clear(scope, Some(sign(&keypair(), &seed, &scope))),
        Status::Flash
    );
    n.agent.restart();
    assert!(fake.0.borrow().peers.is_empty());
    key_gets_state(&mut n);
}
