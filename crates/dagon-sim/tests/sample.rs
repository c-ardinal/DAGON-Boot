//! The sample hub: three nodes on 1.0.0 and the pre-signed 1.1.0 release.

use dagon_format::bundle::BundleView;
use dagon_format::header::{mcu_id, ImageHeader, HEADER_SIZE};
use dagon_format::{sha256, Version};
use dagon_proto::payload::{self, *};
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::*;

struct Pc {
    hub: SimHub,
    seq: u8,
}

impl Pc {
    fn frame(&mut self, cmd: Cmd, p: &[u8]) -> Vec<u8> {
        self.seq = self.seq.wrapping_add(1);
        let mut b = vec![0u8; 12 + p.len()];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload: p,
        }
        .encode(&mut b)
        .unwrap();
        b.truncate(n);
        b
    }

    fn body(resp: &[u8]) -> (Status, Vec<u8>) {
        let (f, _) = Frame::decode(resp, max_payload::USB).unwrap();
        let (s, b) = split_status(f.payload).unwrap();
        (s, b.to_vec())
    }

    /// `None` = the hub itself, `Some(link, addr)` = through RELAY.
    fn call(&mut self, to: Option<(u8, u8)>, cmd: Cmd, p: &[u8]) -> Vec<u8> {
        let inner = self.frame(cmd, p);
        let req = match to {
            None => inner,
            Some((link, addr)) => {
                let mut rp = vec![0u8; 4 + inner.len()];
                let r = Relay {
                    link,
                    addr,
                    timeout_ms: 500,
                    inner: &inner,
                };
                let n = r.encode(&mut rp).unwrap();
                self.frame(Cmd::Relay, &rp[..n])
            }
        };
        let (s, body) = Pc::body(&self.hub.request(&req).expect("hub answers"));
        assert_eq!(s, Status::Ok, "{cmd:?} via hub");
        let body = if to.is_some() {
            Pc::body(&body)
        } else {
            (s, body)
        };
        assert_eq!(body.0, Status::Ok, "{cmd:?}");
        body.1
    }
}

const SEG: Option<(u8, u8)> = Some((payload::link::I2C0, NODE_I2C_ADDR));
const WATCH: Option<(u8, u8)> = Some((payload::link::PIO_UART, payload::link::UART_ADDR));

fn hello() -> Vec<u8> {
    let mut b = [0u8; 6];
    let req = HelloReq {
        proto_ver: 1,
        max_payload: max_payload::I2C as u16,
    };
    let n = req.encode(&mut b).unwrap();
    b[..n].to_vec()
}

#[test]
fn three_nodes_on_1_0_0() {
    let mut pc = Pc {
        hub: sample_hub(1),
        seq: 0,
    };
    let v = Version::new(1, 0, 0);
    for (to, mcu, role) in [
        (None, mcu_id::RP2040, role::HUB),
        (SEG, mcu_id::STM32F401XE, role::NODE),
        (WATCH, mcu_id::ESP32S3, role::NODE),
    ] {
        let h = HelloResp::parse(&pc.call(to, Cmd::Hello, &hello())).unwrap();
        assert_eq!(
            (h.mcu_id, h.role, h.active_version, h.trial),
            (mcu, role, v, false)
        );
        let st = StatusResp::parse(&pc.call(to, Cmd::Status, &[])).unwrap();
        assert_eq!((st.active, st.confirmed), (0, 0));
        assert_eq!(st.slots[0].state, SlotState::Confirmed);
    }
    let st = StatusResp::parse(&pc.call(WATCH, Cmd::Status, &[])).unwrap();
    assert_eq!(st.slots[2].state, SlotState::Written, "factory slot");
    assert_eq!(st.slots[2].version, v);
}

#[test]
fn release_verifies_and_updates_the_watch() {
    let bundle = BundleView::parse(sample::RELEASE_1_1_0).unwrap();
    bundle.verify(&sample::PUBKEY).unwrap();
    let watch_img = (0..bundle.item_count())
        .map(|i| bundle.item_bytes(i))
        .find(|b| {
            let h: Option<&[u8; HEADER_SIZE]> =
                b.get(..HEADER_SIZE).and_then(|h| h.try_into().ok());
            h.and_then(|h| ImageHeader::parse(h).ok())
                .is_some_and(|h| h.mcu_id == mcu_id::ESP32S3)
        })
        .expect("watch image in the release");

    let mut pc = Pc {
        hub: sample_hub(2),
        seq: 0,
    };
    let mut b = [0u8; 8];
    let begin = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: watch_img.len() as u32,
        hdr_len: IMAGE_HDR_LEN,
    };
    let n = begin.encode(&mut b).unwrap();
    pc.call(WATCH, Cmd::XferBegin, &b[..n]);
    let (hdr, body) = watch_img.split_at(HEADER_SIZE);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(1000).enumerate() {
            let mut p = vec![0u8; 4 + c.len()];
            let n = XferChunk {
                offset: (i * 1000) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            pc.call(WATCH, cmd, &p[..n]);
        }
    }
    assert_eq!(pc.call(WATCH, Cmd::XferEnd, &[]), sha256(body));
    pc.call(WATCH, Cmd::SetPending, &[target::APP, 1]);
    pc.call(WATCH, Cmd::Reboot, &[reboot_mode::NORMAL]);
    let h = HelloResp::parse(&pc.call(WATCH, Cmd::Hello, &hello())).unwrap();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 1, 0), true));
    pc.call(WATCH, Cmd::Confirm, &[target::APP]);
    let st = StatusResp::parse(&pc.call(WATCH, Cmd::Status, &[])).unwrap();
    assert_eq!((st.active, st.confirmed), (1, 1));
}
