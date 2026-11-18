//! What the host reads from a LINK_DIAG answer, and the MAX_RESP it asks
//! with on each route.

use dagon_host::diag::{history, History};
use dagon_host::{Host, Target};
use dagon_proto::diag::*;
use dagon_proto::payload::{PolicySet, Relay};
use dagon_proto::table::link_diag::{store, FORMAT};
use dagon_proto::{frame_len, max_payload};

const EP: Endpoint = Endpoint {
    link: 1,
    addr: 0x42,
};

fn info(store: u8) -> DiagInfo {
    DiagInfo {
        availability: 0x7F,
        store,
        policy: PolicySet::EMPTY.with(1).with(2).with(3),
        params: Params::default(),
    }
}

fn answer(store: u8, peers: &[u8]) -> LinkDiagResp<'_> {
    LinkDiagResp {
        format: FORMAT,
        info: info(store),
        elements: if peers.is_empty() {
            Elements::None
        } else {
            Elements::Peers(peers)
        },
        next: None,
    }
}

#[test]
fn unknown_format_is_unknown_history() {
    let mut b = [0u8; 64];
    let n = answer(store::RETENTION_POWER_CUT, &[])
        .encode(&mut b)
        .unwrap();
    b[0] = 2;
    let r = LinkDiagResp::parse(&b[..n]).unwrap();
    assert_eq!(r.format, 2);
    assert_eq!(history(&r, EP), History::Unknown);
}

#[test]
fn element_layouts_golden() {
    let absent = Peer {
        link: EP.link,
        addr: EP.addr,
        instance: NO_INSTANCE,
    };
    let known = Peer {
        instance: [0xA5; 16],
        ..absent
    };
    let power_cut = store::RETENTION_POWER_CUT;
    for p in [absent, known] {
        let bytes = p.to_bytes();
        assert_eq!(&bytes[..2], &[EP.link, EP.addr]);
        assert_eq!(
            history(&answer(power_cut, &bytes), EP),
            History::Communicated
        );
    }
    assert_eq!(history(&answer(power_cut, &[]), EP), History::Never);
    // A reserved STORE bit, or retention 3: nothing is read.
    for bad in [
        power_cut | 1 << 5,
        power_cut | 1 << 6,
        power_cut | 1 << 7,
        store::RETENTION,
    ] {
        let r = answer(bad, &[]);
        assert!(!r.readable());
        assert_eq!(history(&r, EP), History::Unknown, "{bad:#x}");
    }
    // A store that may have lost what it held.
    for unsure in [
        store::RETENTION_NONE,
        store::RETENTION_RESET,
        power_cut | store::FULL,
        power_cut | store::LOST,
        power_cut | store::UNKNOWN_FORMAT,
    ] {
        assert_eq!(
            history(&answer(unsure, &[]), EP),
            History::Unknown,
            "{unsure:#x}"
        );
    }
}

#[derive(Clone)]
struct Up(usize);

impl dagon_host::Transport for Up {
    type Error = ();
    fn max_payload(&self) -> usize {
        self.0
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Stays
    }
    async fn write(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    async fn read(&mut self, _: &mut [u8]) -> Result<usize, ()> {
        Ok(0)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl dagon_host::HostClock for Up {
    fn now_ms(&self) -> u64 {
        0
    }
    async fn sleep_ms(&mut self, _: u32) {}
}

#[test]
fn max_resp_for_route() {
    let linked = |max_payload| Target::Linked {
        link: EP.link,
        addr: EP.addr,
        max_payload,
    };
    let smallest = max_payload::SMALLEST;
    // A 64-byte link one hop deep, from a node whose own link is 64 too.
    let h = Host::new(Up(smallest), Up(smallest));
    assert_eq!(h.max_resp(linked(smallest as u16)), 43);
    // An I2C link from the PC's USB.
    let h = Host::new(Up(max_payload::USB), Up(0));
    let i2c = max_payload::I2C;
    let inner = Relay::tunnel_mtu(max_payload::USB, i2c) - frame_len(0);
    assert_eq!(usize::from(h.max_resp(linked(i2c as u16))), inner);
    assert_eq!(inner, i2c);
    // The node the PC is connected to: its link.
    assert_eq!(usize::from(h.max_resp(Target::Local)), max_payload::USB);
    for report in [1, 2] {
        assert!(min_resp(report) <= 43);
    }
}
