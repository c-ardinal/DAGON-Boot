//! The simulated hub as a test partner: requests from the PC side only.

use dagon_format::header::*;
use dagon_format::statelog::reason;
use dagon_format::{key_id, sha256, Version};
use dagon_port::I2cBus;
use dagon_proto::payload::{self, *};
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::rom::{cmd, ACK, BUSY, NACK, ROM_ADDR};
use dagon_sim::*;
use ed25519_compact::{KeyPair, Seed};
use payload::link;

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

fn kp() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

fn image(slot: u8, version: Version, body: &[u8]) -> Vec<u8> {
    let kp = kp();
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: Layout::STM32F401XE.slots[slot as usize].0 + 0x200,
        payload_size: body.len() as u32,
        version,
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: f401_config([0; 32]).target_tag,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], body].concat()
}

fn hub_with_f401() -> SimHub {
    let mut hub = demo_hub(*kp().pk, 42);
    let img = image(0, Version::new(1, 0, 0), &[0x5A; 3000]);
    hub.i2c_node(NODE_I2C_ADDR)
        .unwrap()
        .provision(&img, 0)
        .unwrap();
    hub
}

/// PC side: frames requests, checks the response header.
struct Pc {
    seq: u8,
}

impl Pc {
    fn frame(&mut self, cmd: Cmd, payload: &[u8]) -> Vec<u8> {
        self.seq = self.seq.wrapping_add(1);
        let mut b = vec![0u8; 8 + payload.len() + 4];
        let n = Frame {
            flags: 0,
            seq: self.seq,
            cmd: cmd as u8,
            payload,
        }
        .encode(&mut b)
        .unwrap();
        b.truncate(n);
        b
    }

    fn parse(resp: &[u8], seq: u8, cmd: Cmd) -> (Status, Vec<u8>) {
        let (f, _) = Frame::decode(resp, max_payload::USB).unwrap();
        assert_eq!((f.flags, f.seq, f.cmd), (flags::RESP, seq, cmd as u8));
        let (s, body) = split_status(f.payload).unwrap();
        (s, body.to_vec())
    }

    fn hub(&mut self, hub: &mut SimHub, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        let req = self.frame(cmd, payload);
        let resp = hub.request(&req).expect("hub answers");
        Pc::parse(&resp, self.seq, cmd)
    }

    /// RELAY to the I2C node; returns the relay status and the node's answer.
    fn node(
        &mut self,
        hub: &mut SimHub,
        addr: u8,
        cmd: Cmd,
        payload: &[u8],
    ) -> (Status, Option<(Status, Vec<u8>)>) {
        let inner = self.frame(cmd, payload);
        let inner_seq = self.seq;
        let mut p = vec![0u8; 4 + inner.len()];
        let n = Relay {
            link: link::I2C0,
            addr,
            timeout_ms: 200,
            inner: &inner,
        }
        .encode(&mut p)
        .unwrap();
        let (s, body) = self.hub(hub, Cmd::Relay, &p[..n]);
        (
            s,
            (s == Status::Ok).then(|| Pc::parse(&body, inner_seq, cmd)),
        )
    }

    fn ok(&mut self, hub: &mut SimHub, cmd: Cmd, payload: &[u8]) -> Vec<u8> {
        let (s, inner) = self.node(hub, NODE_I2C_ADDR, cmd, payload);
        assert_eq!(s, Status::Ok, "relay {cmd:?}");
        let (st, body) = inner.unwrap();
        assert_eq!(st, Status::Ok, "{cmd:?}");
        body
    }

    fn rom(&mut self, hub: &mut SimHub, write: &[u8], read_len: u16) -> Vec<u8> {
        let mut p = vec![0u8; 4 + write.len()];
        let n = I2cRaw {
            read_len,
            data: write,
        }
        .encode(&mut p)
        .unwrap();
        let (s, body) = self.hub(hub, Cmd::I2cRaw, &p[..n]);
        assert_eq!(s, Status::Ok);
        body
    }

    fn subctl(&mut self, hub: &mut SimHub, nrst: u8, boot0: u8) {
        let (s, _) = self.hub(hub, Cmd::SubCtl, &[nrst, boot0]);
        assert_eq!(s, Status::Ok);
    }
}

fn hello() -> Vec<u8> {
    let mut b = [0u8; 6];
    let n = HelloReq {
        proto_ver: 1,
        max_payload: max_payload::I2C as u16,
    }
    .encode(&mut b)
    .unwrap();
    b[..n].to_vec()
}

#[test]
fn hello_and_status_through_the_hub() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };

    let (s, body) = pc.hub(&mut hub, Cmd::Hello, &hello());
    assert_eq!(s, Status::Ok);
    let h = HelloResp::parse(&body).unwrap();
    assert_eq!((h.role, h.mcu_id), (role::HUB, mcu_id::RP2040));

    let h = HelloResp::parse(&pc.ok(&mut hub, Cmd::Hello, &hello())).unwrap();
    assert_eq!(
        (h.role, h.mcu_id, h.trial),
        (role::NODE, mcu_id::STM32F401XE, false)
    );
    assert_eq!(h.active_version, Version::new(1, 0, 0));
    assert_eq!(h.key_id, key_id(&kp().pk));

    let st = StatusResp::parse(&pc.ok(&mut hub, Cmd::Status, &[])).unwrap();
    assert_eq!((st.active, st.confirmed), (0, 0));
    assert_eq!(st.slots[0].state, SlotState::Confirmed);
    assert_eq!(st.slots[1].state, SlotState::Empty);
    assert_eq!(st.log_generation, 1);
}

#[test]
fn usb_byte_stream_carries_split_frames() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };
    let req = pc.frame(Cmd::Status, &[]);
    hub.usb_write(b"noise");
    hub.usb_write(&req[..5]);
    assert!(hub.usb_read().is_empty());
    hub.usb_write(&req[5..]);
    let (s, body) = Pc::parse(&hub.usb_read(), pc.seq, Cmd::Status);
    assert_eq!(s, Status::Ok);
    assert_eq!(
        StatusResp::parse(&body).unwrap().active,
        0xFF,
        "hub has no image"
    );
}

#[test]
fn hub_store_on_page_programmed_flash() {
    let mut hub = hub_with_f401();
    hub.node
        .provision(&image(0, Version::new(3, 0, 1), &[9; 1000]), 0)
        .unwrap();
    let mut pc = Pc { seq: 0 };
    let (_, body) = pc.hub(&mut hub, Cmd::Status, &[]);
    let st = StatusResp::parse(&body).unwrap();
    assert_eq!(
        (st.active, st.confirmed, st.slots[0].version),
        (0, 0, Version::new(3, 0, 1))
    );
    assert!(hub
        .node
        .flash()
        .ops()
        .iter()
        .all(|op| op.len % 256 == 0 || op.len % 0x1000 == 0));
}

#[test]
fn i2c_scan_finds_only_present_targets() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };
    assert_eq!(hub.bus.scan(), vec![NODE_I2C_ADDR]);
    assert_eq!(
        block_on(hub.bus.write(0x43, &[])),
        Err(dagon_port::BusError::Nack)
    );

    let t0 = hub.now_ms();
    let (s, _) = pc.node(&mut hub, 0x43, Cmd::Hello, &hello());
    assert_eq!(s, Status::Timeout, "absent address NACKs until the timeout");
    assert!(hub.now_ms() - t0 >= 200);

    pc.subctl(&mut hub, nrst::PULSE, boot0::HIGH);
    assert_eq!(
        hub.bus.scan(),
        vec![ROM_ADDR],
        "ROM loader replaces the app"
    );
    pc.subctl(&mut hub, nrst::PULSE, boot0::LOW);
    assert_eq!(hub.bus.scan(), vec![NODE_I2C_ADDR]);

    let mut empty = demo_hub(*kp().pk, 1);
    assert!(empty.bus.scan().is_empty(), "erased node stays silent");
}

#[test]
fn rom_get_and_get_id() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };
    let mut p = [0u8; 8];
    let n = I2cRaw {
        read_len: 1,
        data: &[cmd::GET, 0xFF],
    }
    .encode(&mut p)
    .unwrap();
    assert_eq!(
        pc.hub(&mut hub, Cmd::I2cRaw, &p[..n]).0,
        Status::State,
        "BOOT0 low"
    );

    pc.subctl(&mut hub, nrst::PULSE, boot0::HIGH);
    assert_eq!(pc.rom(&mut hub, &[cmd::GET, 0xFF], 1), [ACK]);
    let n = pc.rom(&mut hub, &[], 1)[0] as usize;
    let rest = pc.rom(&mut hub, &[], n as u16 + 1);
    assert_eq!(rest[0], 0x11, "I2C protocol V1.1");
    assert!(rest[1..].contains(&cmd::NS_WRITE_MEMORY) && rest[1..].contains(&cmd::NS_ERASE));
    assert_eq!(pc.rom(&mut hub, &[], 1), [ACK]);

    assert_eq!(
        pc.rom(&mut hub, &[cmd::GET_ID, 0xFD], 5),
        [ACK, 1, 0x04, 0x33, ACK]
    );
    assert_eq!(
        pc.rom(&mut hub, &[cmd::GET, 0x00], 1),
        [NACK],
        "bad complement"
    );
}

#[test]
fn rom_erase_write_read_with_busy_and_frame_timeout() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };
    pc.subctl(&mut hub, nrst::PULSE, boot0::HIGH);

    // No-Stretch erase of sector 5 (APP-A): BUSY until done.
    assert_eq!(pc.rom(&mut hub, &[cmd::NS_ERASE, 0xBA], 1), [ACK]);
    assert_eq!(pc.rom(&mut hub, &[0x00, 0x00, 0x00], 1), [ACK]);
    assert_eq!(pc.rom(&mut hub, &[0x00, 0x05, 0x05], 1), [BUSY]);
    let mut polls = 0;
    while pc.rom(&mut hub, &[], 1) == [BUSY] {
        hub.advance_ms(5);
        polls += 1;
    }
    assert!(polls > 10);

    // No-Stretch write of 4 bytes at APP-A, then read back.
    let addr = 0x0802_0000u32.to_be_bytes();
    let ax = addr.iter().fold(0, |a, b| a ^ b);
    assert_eq!(pc.rom(&mut hub, &[cmd::NS_WRITE_MEMORY, 0xCD], 1), [ACK]);
    assert_eq!(
        pc.rom(&mut hub, &[addr[0], addr[1], addr[2], addr[3], ax], 1),
        [ACK]
    );
    let data = [3u8, 0xDE, 0xAD, 0xBE, 0xEF];
    let x = data.iter().fold(0, |a, b| a ^ b);
    pc.rom(&mut hub, &[data.as_slice(), &[x]].concat(), 0);
    hub.advance_ms(10);
    assert_eq!(pc.rom(&mut hub, &[], 1), [ACK]);

    assert_eq!(pc.rom(&mut hub, &[cmd::READ_MEMORY, 0xEE], 1), [ACK]);
    assert_eq!(
        pc.rom(&mut hub, &[addr[0], addr[1], addr[2], addr[3], ax], 1),
        [ACK]
    );
    assert_eq!(pc.rom(&mut hub, &[3, !3], 5), [ACK, 0xDE, 0xAD, 0xBE, 0xEF]);

    // Over 1000 ms between frames of an open command: the loader resets.
    assert_eq!(pc.rom(&mut hub, &[cmd::READ_MEMORY, 0xEE], 1), [ACK]);
    hub.advance_ms(1001);
    assert_eq!(
        pc.rom(&mut hub, &[addr[0], addr[1], addr[2], addr[3], ax], 1),
        [NACK]
    );
    assert_eq!(hub.i2c_node(NODE_I2C_ADDR).unwrap().rom.timeouts, 1);
}

/// XFER_BEGIN .. XFER_END, SET_PENDING and REBOOT to the I2C node.
fn transfer(pc: &mut Pc, hub: &mut SimHub, slot: u8, img: &[u8]) {
    let mut b = [0u8; 8];
    let total_len = img.len() as u32;
    let begin = XferBegin {
        target: target::APP,
        slot,
        total_len,
        hdr_len: IMAGE_HDR_LEN,
    };
    let n = begin.encode(&mut b).unwrap();
    pc.ok(hub, Cmd::XferBegin, &b[..n]);
    let (hdr, body) = img.split_at(512);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(240).enumerate() {
            let mut p = vec![0u8; 4 + c.len()];
            let n = XferChunk {
                offset: (i * 240) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            pc.ok(hub, cmd, &p[..n]);
        }
    }
    assert_eq!(pc.ok(hub, Cmd::XferEnd, &[]), sha256(body));
    pc.ok(hub, Cmd::SetPending, &[target::APP, slot]);
    pc.ok(hub, Cmd::Reboot, &[reboot_mode::NORMAL]);
}

#[test]
fn update_through_relay_trial_confirm_and_rollback() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };
    transfer(
        &mut pc,
        &mut hub,
        1,
        &image(1, Version::new(1, 1, 0), &[0xA5; 5000]),
    );
    let h = HelloResp::parse(&pc.ok(&mut hub, Cmd::Hello, &hello())).unwrap();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 1, 0), true));
    pc.ok(&mut hub, Cmd::Confirm, &[target::APP]);
    hub.advance_ms(20_000);
    let st = StatusResp::parse(&pc.ok(&mut hub, Cmd::Status, &[])).unwrap();
    assert_eq!(
        (st.active, st.confirmed, st.slots[1].state),
        (1, 1, SlotState::Confirmed)
    );

    // A trial app that never confirms is rolled back by the trial watchdog.
    hub.i2c_node(NODE_I2C_ADDR).unwrap().trial_app = TrialApp::NeverConfirms;
    let mut pc2 = Pc { seq: 100 };
    transfer(
        &mut pc2,
        &mut hub,
        0,
        &image(0, Version::new(1, 2, 0), &[0x33; 700]),
    );
    let (_, inner) = pc2.node(&mut hub, NODE_I2C_ADDR, Cmd::Confirm, &[target::APP]);
    assert_eq!(inner.unwrap().0, Status::State, "self-check refuses");
    for _ in 0..4 {
        hub.advance_ms(10_000);
    }
    let st = StatusResp::parse(&pc2.ok(&mut hub, Cmd::Status, &[])).unwrap();
    assert_eq!((st.active, st.confirmed), (1, 1));
    assert_eq!(st.slots[0].state, SlotState::Reverted);
    assert_eq!(st.last_reason, reason::TRIAL_EXHAUSTED);
}

#[test]
fn power_cut_mid_transfer_keeps_the_confirmed_image() {
    let mut hub = hub_with_f401();
    let mut pc = Pc { seq: 0 };
    let node = hub.i2c_node(NODE_I2C_ADDR).unwrap();
    let before = node.flash().ops().len();
    node.flash().arm_cut(CutPoint::During(before + 3));

    let img = image(1, Version::new(1, 1, 0), &[0x77; 4000]);
    let mut b = [0u8; 8];
    let n = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: img.len() as u32,
        hdr_len: IMAGE_HDR_LEN,
    }
    .encode(&mut b)
    .unwrap();
    pc.ok(&mut hub, Cmd::XferBegin, &b[..n]);
    let mut lost = false;
    for (cmd, part) in [(Cmd::XferHdr, &img[..512]), (Cmd::XferData, &img[512..])] {
        for (i, c) in part.chunks(240).enumerate() {
            let mut p = vec![0u8; 4 + c.len()];
            let n = XferChunk {
                offset: (i * 240) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            if pc.node(&mut hub, NODE_I2C_ADDR, cmd, &p[..n]).0 == Status::Timeout {
                lost = true;
                break;
            }
        }
    }
    assert!(lost, "node went dark at the cut");
    let node = hub.i2c_node(NODE_I2C_ADDR).unwrap();
    assert_eq!(node.run(), Run::Off);
    node.power_on(0);
    let h = HelloResp::parse(&pc.ok(&mut hub, Cmd::Hello, &hello())).unwrap();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 0, 0), false));
}

#[test]
fn uart_node_behind_a_lossy_link() {
    let mut hub = hub_with_f401();
    let mut cfg = f401_config(*kp().pk);
    cfg.max_payload = max_payload::UART as u16;
    let mut node = SimNode::new(cfg, SimFlash::stm32f401xe(9), Layout::STM32F401XE);
    node.provision(&image(0, Version::new(2, 0, 0), &[1; 100]), 0)
        .unwrap();
    let mut link = SimLink::new(3);
    link.drop_p = 1.0;
    hub.set_uart(link, node);

    let mut pc = Pc { seq: 0 };
    let relay = |pc: &mut Pc, hub: &mut SimHub| {
        let inner = pc.frame(Cmd::Hello, &hello());
        let mut p = vec![0u8; 4 + inner.len()];
        let n = Relay {
            link: link::PIO_UART,
            addr: link::UART_ADDR,
            timeout_ms: 500,
            inner: &inner,
        }
        .encode(&mut p)
        .unwrap();
        pc.hub(hub, Cmd::Relay, &p[..n])
    };
    assert_eq!(relay(&mut pc, &mut hub).0, Status::Timeout);
    let (link, _) = hub.bus.uart.as_mut().unwrap();
    link.drop_p = 0.0;
    link.delay_ms = 20;
    let (s, body) = relay(&mut pc, &mut hub);
    assert_eq!(s, Status::Ok);
    let (st, h) = Pc::parse(&body, pc.seq - 1, Cmd::Hello);
    assert_eq!(st, Status::Ok);
    assert_eq!(
        HelloResp::parse(&h).unwrap().active_version,
        Version::new(2, 0, 0)
    );
}
