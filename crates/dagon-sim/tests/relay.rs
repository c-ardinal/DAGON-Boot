//! The simulated relaying node as a test partner: requests from the PC side
//! only, on the sample's wiring.

use dagon_format::header::*;
use dagon_format::statelog::reason;
use dagon_format::{key_id, sha256, Version};
use dagon_port::I2cBus;
use dagon_proto::payload::{self, *};
use dagon_proto::{flags, max_payload, plant, split_status, Cmd, Frame, Status};
use dagon_sim::rom::{cmd, ACK, BUSY, NACK};
use dagon_sim::*;
use ed25519_compact::{KeyPair, Seed};
use payload::link;

/// The relay link id the relaying node's MCU definition gives its port `port`.
fn link_on(port: &str) -> u8 {
    store::relay_link_id(store::Layout::RP2040_HUB.mcu, port)
}

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
        target_tag: f401_config().target_tag,
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    [&h.to_bytes()[..], body].concat()
}

fn relay_with_f401() -> SimRelay {
    let mut relay = demo_hub(*kp().pk, 42);
    let img = image(0, Version::new(1, 0, 0), &[0x5A; 3000]);
    relay
        .i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR)
        .unwrap()
        .provision(&img, 0)
        .unwrap();
    relay
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
        let body = bulk_of(f.cmd, s, body);
        (s, body.to_vec())
    }

    fn ask(&mut self, relay: &mut SimRelay, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        let req = self.frame(cmd, payload);
        let resp = relay.request(&req).expect("the relaying node answers");
        Pc::parse(&resp, self.seq, cmd)
    }

    /// The status a request of any command number is answered with.
    fn status(&mut self, relay: &mut SimRelay, cmd: u8, payload: &[u8]) -> Status {
        self.seq = self.seq.wrapping_add(1);
        let mut req = vec![0u8; 8 + payload.len() + 4];
        req[8..8 + payload.len()].copy_from_slice(payload);
        dagon_proto::finish(&mut req, 0, self.seq, cmd, payload.len()).unwrap();
        let resp = relay.request(&req).expect("the relaying node answers");
        let (f, _) = Frame::decode(&resp, max_payload::USB).unwrap();
        assert_eq!((f.flags, f.seq, f.cmd), (flags::RESP, self.seq, cmd));
        split_status(f.payload).unwrap().0
    }

    /// RELAY to the I2C node; returns the relay status and the node's answer.
    fn node(
        &mut self,
        relay: &mut SimRelay,
        addr: u8,
        cmd: Cmd,
        payload: &[u8],
    ) -> (Status, Option<(Status, Vec<u8>)>) {
        let inner = self.frame(cmd, payload);
        let inner_seq = self.seq;
        let mut p = vec![0u8; 9 + inner.len()];
        let n = Relay {
            link: link_on(NODE_I2C_PORT),
            addr,
            timeout_ms: Some(200),
            inhibit: false,
            inner: &inner,
        }
        .encode(&mut p)
        .unwrap();
        let (s, body) = self.ask(relay, Cmd::Relay, &p[..n]);
        (
            s,
            (s == Status::Ok).then(|| Pc::parse(&body, inner_seq, cmd)),
        )
    }

    fn ok(&mut self, relay: &mut SimRelay, cmd: Cmd, payload: &[u8]) -> Vec<u8> {
        let (s, inner) = self.node(relay, NODE_I2C_ADDR, cmd, payload);
        assert_eq!(s, Status::Ok, "relay {cmd:?}");
        let (st, body) = inner.unwrap();
        assert_eq!(st, Status::Ok, "{cmd:?}");
        body
    }

    fn rom(&mut self, relay: &mut SimRelay, write: &[u8], read_len: u16) -> Vec<u8> {
        let mut p = vec![0u8; 16 + write.len()];
        let n = I2cRaw {
            read_len,
            data: write,
        }
        .encode(&mut p)
        .unwrap();
        let (s, body) = self.ask(relay, Cmd::I2cRaw, &p[..n]);
        assert_eq!(s, Status::Ok);
        body
    }

    fn subctl(&mut self, relay: &mut SimRelay, nrst: u8, boot0: u8) {
        let (s, _) = self.ask(
            relay,
            Cmd::SubCtl,
            &wire(|b| dagon_proto::payload::SubCtl { nrst, boot0 }.encode(b)),
        );
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
fn hello_and_status_through_the_relaying_node() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };

    let (s, body) = pc.ask(&mut relay, Cmd::Hello, &hello());
    assert_eq!(s, Status::Ok);
    let h = HelloResp::parse(&body).unwrap();
    assert_eq!((h.role, h.mcu_id), (role::HUB, mcu_id::RP2040));

    let h = HelloResp::parse(&pc.ok(&mut relay, Cmd::Hello, &hello())).unwrap();
    assert_eq!(
        (h.role, h.mcu_id, h.trial),
        (role::NODE, mcu_id::STM32F401XE, false)
    );
    assert_eq!(h.active_version, Version::new(1, 0, 0));
    assert_eq!(h.key_id, key_id(&kp().pk));

    let st = StatusResp::parse(&pc.ok(&mut relay, Cmd::Status, &[])).unwrap();
    assert_eq!((st.active, st.confirmed), (0, 0));
    assert_eq!(st.slots[0].state, SlotState::Confirmed);
    assert_eq!(st.slots[1].state, SlotState::Empty);
    assert_eq!(st.log_generation, 1);
}

fn set_of(cmds: &[Cmd]) -> CmdSet {
    cmds.iter().fold(CmdSet::EMPTY, |s, c| s.with(*c as u8))
}

/// The commands an app's agent serves, in the command table's order.
const APP: [Cmd; 13] = [
    Cmd::Hello,
    Cmd::Status,
    Cmd::Version,
    Cmd::Caps,
    Cmd::Stay,
    Cmd::XferBegin,
    Cmd::XferData,
    Cmd::XferEnd,
    Cmd::XferHdr,
    Cmd::SetPending,
    Cmd::Reboot,
    Cmd::Confirm,
    Cmd::Show,
];

/// The simulated relaying node says it serves its agent's commands and
/// the relay's, a node behind it says its agent's, and a request with an
/// unknown must field, a field sent twice or a bad length is refused in
/// every command of the relaying node with no port and no recovery line
/// used and its slots as they were.
#[test]
fn the_relaying_node_says_what_it_serves_and_reads_before_it_acts() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    let (st, body) = pc.ask(&mut relay, Cmd::Hello, &hello());
    assert_eq!(st, Status::Ok);
    let cmds = HelloResp::parse(&body).unwrap().cmds;
    let own = [Cmd::Relay, Cmd::SubCtl, Cmd::I2cRaw];
    assert_eq!(cmds, set_of(&APP).union(set_of(&own)));
    let behind = pc.ok(&mut relay, Cmd::Hello, &hello());
    assert_eq!(HelloResp::parse(&behind).unwrap().cmds, set_of(&APP));

    let (ops, slots) = (relay.bus.ops, relay.node.status());
    let mut served = CmdSet::EMPTY;
    plant::each_probe(|n, p| {
        if pc.status(&mut relay, n, p) == Status::Field {
            served = served.with(n);
        }
    });
    assert_eq!(served, cmds, "what answers is what HELLO says");
    let commands = plant::each(&cmds, |n, plant, p| {
        assert_eq!(
            pc.status(&mut relay, n, p),
            plant.status(),
            "{n:#x} {plant:?}"
        );
        assert_eq!(relay.bus.ops, ops, "{n:#x} {plant:?}: a port was used");
    });
    assert_eq!(commands, APP.len() + own.len(), "every command it serves");
    assert_eq!(relay.node.status(), slots);

    // CAPS about RELAY is the relay's, with the tunnel of the link asked
    // about for this requester (its HELLO named the payload of I2C);
    // about another command it is the agent's.
    let bus = link_on(NODE_I2C_PORT);
    let caps = |cmd: Cmd, link: Option<u8>| {
        wire(|b| {
            CapsReq {
                cmd: cmd as u8,
                link,
            }
            .encode(b)
        })
    };
    let (st, body) = pc.ask(&mut relay, Cmd::Caps, &caps(Cmd::Relay, Some(bus)));
    let tunnel = Relay::tunnel_mtu(max_payload::I2C, max_payload::I2C);
    assert_eq!(st, Status::Ok);
    assert_eq!(
        CapsResp::parse(&body).unwrap(),
        CapsResp {
            tunnel_mtu: Some(tunnel as u16),
            ..CapsResp::of(Cmd::Relay)
        }
    );
    let (st, body) = pc.ask(&mut relay, Cmd::Caps, &caps(Cmd::Status, None));
    assert_eq!(st, Status::Ok);
    assert_eq!(CapsResp::parse(&body), Ok(CapsResp::of(Cmd::Status)));
    let none = (1..=u8::MAX).find(|id| relay.links().iter().all(|l| l.id != *id));
    let (st, _) = pc.ask(&mut relay, Cmd::Caps, &caps(Cmd::Relay, none));
    assert_eq!(st, Status::Arg, "not a link of the node");

    // The requester's HELLO names the smallest payload a link offers: an
    // answer that would not fit it is refused before a port is used,
    // I2C_RAW by the bytes it asks to read, RELAY by the tunnel.
    pc.subctl(&mut relay, nrst::PULSE, boot0::HIGH);
    let floor = max_payload::SMALLEST;
    let said = HelloReq {
        proto_ver: 2,
        max_payload: floor as u16,
    };
    let (st, _) = pc.ask(&mut relay, Cmd::Hello, &wire(|b| said.encode(b)));
    assert_eq!(st, Status::Ok);
    let (_, body) = pc.ask(&mut relay, Cmd::Caps, &caps(Cmd::Relay, Some(bus)));
    let tunnel = Relay::tunnel_mtu(floor, max_payload::I2C);
    assert_eq!(
        CapsResp::parse(&body).unwrap().tunnel_mtu,
        Some(tunnel as u16)
    );
    let ops = relay.bus.ops;
    let raw = |read_len: u16| {
        wire(|b| {
            I2cRaw {
                read_len,
                data: &[],
            }
            .encode(b)
        })
    };
    let most = (floor - 2) as u16;
    let (st, _) = pc.ask(&mut relay, Cmd::I2cRaw, &raw(most + 1));
    assert_eq!((st, relay.bus.ops), (Status::Arg, ops));
    let inner = |len: usize| {
        let mut f = vec![0u8; len];
        dagon_proto::finish(&mut f, 0, 0x5A, 0x7F, len - 12).unwrap();
        f
    };
    let over = |inner: &[u8]| {
        wire(|b| {
            Relay {
                link: bus,
                addr: NODE_I2C_ADDR,
                timeout_ms: None,
                inhibit: false,
                inner,
            }
            .encode(b)
        })
    };
    let (st, _) = pc.ask(&mut relay, Cmd::Relay, &over(&inner(tunnel + 1)));
    assert_eq!((st, relay.bus.ops), (Status::Arg, ops), "over the tunnel");
    // At the ceiling both use the port.
    let (st, read) = pc.ask(&mut relay, Cmd::I2cRaw, &raw(most));
    assert_eq!((st, read.len()), (Status::Ok, usize::from(most)));
    let (st, _) = pc.ask(&mut relay, Cmd::Relay, &over(&inner(tunnel)));
    assert_ne!(st, Status::Arg);
    assert!(relay.bus.ops > ops);

    // A relaying node with no node on its recovery lines serves RELAY alone
    // of the relay's commands.
    let mut bare = SimRelay::new(SimNode::new(
        hub_config(),
        *kp().pk,
        SimFlash::rp2040_2m(1),
        Layout::RP2040_HUB,
    ));
    let (_, body) = pc.ask(&mut bare, Cmd::Hello, &hello());
    let cmds = HelloResp::parse(&body).unwrap().cmds;
    assert_eq!(cmds, set_of(&APP).union(set_of(&[Cmd::Relay])));
    let mut served = CmdSet::EMPTY;
    plant::each_probe(|n, p| {
        if pc.status(&mut bare, n, p) == Status::Field {
            served = served.with(n);
        }
    });
    assert_eq!(served, cmds);
}

#[test]
fn usb_byte_stream_carries_split_frames() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    let req = pc.frame(Cmd::Status, &[]);
    relay.usb_write(b"noise");
    relay.usb_write(&req[..5]);
    assert!(relay.usb_read().is_empty());
    relay.usb_write(&req[5..]);
    let (s, body) = Pc::parse(&relay.usb_read(), pc.seq, Cmd::Status);
    assert_eq!(s, Status::Ok);
    assert_eq!(
        StatusResp::parse(&body).unwrap().active,
        0xFF,
        "the relaying node has no image"
    );
}

#[test]
fn own_store_on_page_programmed_flash() {
    let mut relay = relay_with_f401();
    relay
        .node
        .provision(&image(0, Version::new(3, 0, 1), &[9; 1000]), 0)
        .unwrap();
    let mut pc = Pc { seq: 0 };
    let (_, body) = pc.ask(&mut relay, Cmd::Status, &[]);
    let st = StatusResp::parse(&body).unwrap();
    assert_eq!(
        (st.active, st.confirmed, st.slots[0].version),
        (0, 0, Version::new(3, 0, 1))
    );
    assert!(relay
        .node
        .flash()
        .ops()
        .iter()
        .all(|op| op.len % 256 == 0 || op.len % 0x1000 == 0));
}

#[test]
fn i2c_scan_finds_only_present_targets() {
    let mut relay = relay_with_f401();
    // Where the F401's ROM loader answers: its MCU definition's address.
    let rom = relay
        .i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR)
        .unwrap()
        .rom_addr
        .unwrap();
    let mut pc = Pc { seq: 0 };
    let port = relay.port(NODE_I2C_PORT, dagon_port::LinkKind::I2c);
    assert_eq!(relay.bus.scan(port), vec![NODE_I2C_ADDR]);
    assert_eq!(
        block_on(relay.bus.port(port).write(0x43, &[])),
        Err(dagon_port::BusError::Nack)
    );

    let t0 = relay.now_ms();
    let (s, _) = pc.node(&mut relay, 0x43, Cmd::Hello, &hello());
    assert_eq!(s, Status::Timeout, "absent address NACKs until the timeout");
    assert!(relay.now_ms() - t0 >= 200);

    pc.subctl(&mut relay, nrst::PULSE, boot0::HIGH);
    assert_eq!(
        relay.bus.scan(port),
        vec![rom],
        "ROM loader replaces the app"
    );
    pc.subctl(&mut relay, nrst::PULSE, boot0::LOW);
    assert_eq!(relay.bus.scan(port), vec![NODE_I2C_ADDR]);

    // Same lines through the port trait, as relay logic would drive them.
    block_on(dagon_port::SubControl::pulse_reset(&mut relay.bus, true)).unwrap();
    assert_eq!(relay.bus.scan(port), vec![rom]);
    block_on(dagon_port::SubControl::pulse_reset(&mut relay.bus, false)).unwrap();
    assert_eq!(relay.bus.scan(port), vec![NODE_I2C_ADDR]);

    let mut empty = demo_hub(*kp().pk, 1);
    assert!(empty.bus.scan(port).is_empty(), "erased node stays silent");
}

#[test]
fn rom_get_and_get_id() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    let mut p = [0u8; 32];
    let n = I2cRaw {
        read_len: 1,
        data: &[cmd::GET, 0xFF],
    }
    .encode(&mut p)
    .unwrap();
    assert_eq!(
        pc.ask(&mut relay, Cmd::I2cRaw, &p[..n]).0,
        Status::State,
        "BOOT0 low"
    );

    pc.subctl(&mut relay, nrst::PULSE, boot0::HIGH);
    assert_eq!(pc.rom(&mut relay, &[cmd::GET, 0xFF], 1), [ACK]);
    // The data frame comes whole in one read; extra bytes read as 0xFF.
    let data = pc.rom(&mut relay, &[], 32);
    let n = data[0] as usize;
    assert_eq!(data[1], 0x11, "I2C protocol V1.1");
    let cmds = &data[2..2 + n];
    assert!(cmds.contains(&cmd::NS_WRITE_MEMORY) && cmds.contains(&cmd::NS_ERASE));
    assert!(data[2 + n..].iter().all(|&b| b == 0xFF));
    assert_eq!(pc.rom(&mut relay, &[], 1), [ACK]);

    assert_eq!(pc.rom(&mut relay, &[cmd::GET_ID, 0xFD], 1), [ACK]);
    assert_eq!(pc.rom(&mut relay, &[], 3), [1, 0x04, 0x33]);
    assert_eq!(pc.rom(&mut relay, &[], 1), [ACK]);
    assert_eq!(
        pc.rom(&mut relay, &[cmd::GET, 0x00], 1),
        [NACK],
        "bad complement"
    );
}

/// I2C_RAW reads and writes up to the recovery link's buffer (one frame of
/// its max_payload); one byte more is E_ARG, never a shortened transfer.
#[test]
fn i2c_raw_above_the_recovery_links_buffer_is_refused() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    pc.subctl(&mut relay, nrst::PULSE, boot0::HIGH);
    let limit = dagon_proto::frame_len(max_payload::I2C);
    let raw = |read_len: u16, data: &[u8]| {
        let mut p = vec![0u8; 16 + data.len()];
        let n = I2cRaw { read_len, data }.encode(&mut p).unwrap();
        p.truncate(n);
        p
    };
    let over_read = raw(limit as u16 + 1, &[]);
    assert_eq!(pc.ask(&mut relay, Cmd::I2cRaw, &over_read).0, Status::Arg);
    let over_write = raw(0, &vec![0xFF; limit + 1]);
    assert_eq!(pc.ask(&mut relay, Cmd::I2cRaw, &over_write).0, Status::Arg);
    assert_eq!(pc.rom(&mut relay, &[], limit as u16).len(), limit);
}

#[test]
fn rom_erase_write_read_with_busy_and_frame_timeout() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    pc.subctl(&mut relay, nrst::PULSE, boot0::HIGH);

    // No-Stretch erase of sector 5 (APP-A): BUSY until done.
    assert_eq!(pc.rom(&mut relay, &[cmd::NS_ERASE, 0xBA], 1), [ACK]);
    assert_eq!(pc.rom(&mut relay, &[0x00, 0x00, 0x00], 1), [ACK]);
    assert_eq!(pc.rom(&mut relay, &[0x00, 0x05, 0x05], 1), [BUSY]);
    let mut polls = 0;
    while pc.rom(&mut relay, &[], 1) == [BUSY] {
        relay.advance_ms(5);
        polls += 1;
    }
    assert!(polls > 10);

    // No-Stretch write of 4 bytes at APP-A, then read back.
    let addr = 0x0802_0000u32.to_be_bytes();
    let ax = addr.iter().fold(0, |a, b| a ^ b);
    assert_eq!(pc.rom(&mut relay, &[cmd::NS_WRITE_MEMORY, 0xCD], 1), [ACK]);
    assert_eq!(
        pc.rom(&mut relay, &[addr[0], addr[1], addr[2], addr[3], ax], 1),
        [ACK]
    );
    let data = [3u8, 0xDE, 0xAD, 0xBE, 0xEF];
    let x = data.iter().fold(0, |a, b| a ^ b);
    pc.rom(&mut relay, &[data.as_slice(), &[x]].concat(), 0);
    relay.advance_ms(10);
    assert_eq!(pc.rom(&mut relay, &[], 1), [ACK]);

    assert_eq!(pc.rom(&mut relay, &[cmd::READ_MEMORY, 0xEE], 1), [ACK]);
    assert_eq!(
        pc.rom(&mut relay, &[addr[0], addr[1], addr[2], addr[3], ax], 1),
        [ACK]
    );
    assert_eq!(pc.rom(&mut relay, &[3, !3], 1), [ACK]);
    assert_eq!(pc.rom(&mut relay, &[], 4), [0xDE, 0xAD, 0xBE, 0xEF]);

    // Over 1000 ms between frames of an open command: the loader resets.
    assert_eq!(pc.rom(&mut relay, &[cmd::READ_MEMORY, 0xEE], 1), [ACK]);
    relay.advance_ms(1001);
    assert_eq!(
        pc.rom(&mut relay, &[addr[0], addr[1], addr[2], addr[3], ax], 1),
        [NACK]
    );
    assert_eq!(
        relay
            .i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR)
            .unwrap()
            .rom
            .timeouts,
        1
    );
}

#[test]
fn rom_stretched_erase_through_i2c_raw() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    pc.subctl(&mut relay, nrst::PULSE, boot0::HIGH);

    assert_eq!(pc.rom(&mut relay, &[cmd::ERASE, 0xBB], 1), [ACK]);
    assert_eq!(pc.rom(&mut relay, &[0x00, 0x00, 0x00], 1), [ACK]);
    let t0 = relay.now_ms();
    assert_eq!(
        pc.rom(&mut relay, &[0x00, 0x05, 0x05], 1),
        [ACK],
        "sector 5"
    );
    assert!(relay.now_ms() - t0 >= 2048);

    assert_eq!(pc.rom(&mut relay, &[cmd::ERASE, 0xBB], 1), [ACK]);
    assert_eq!(
        pc.rom(&mut relay, &[0xFF, 0xFF, 0x00], 1),
        [ACK],
        "mass erase"
    );
    let node = relay.i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR).unwrap();
    assert_eq!(node.rom.timeouts, 0);
    let f = node.flash();
    let erased = (0..8).all(|s| {
        let (a, l) = f.sector_addr(s).unwrap();
        f.peek(a, l).unwrap().iter().all(|&b| b == 0xFF)
    });
    assert!(erased, "boot core, log and app sectors are blank");
}

/// XFER_BEGIN .. XFER_END, SET_PENDING and REBOOT to the I2C node.
fn transfer(pc: &mut Pc, relay: &mut SimRelay, slot: u8, img: &[u8]) {
    let mut b = [0u8; 32];
    let total_len = img.len() as u32;
    let begin = XferBegin {
        target: target::APP,
        slot,
        total_len,
        hdr_len: IMAGE_HDR_LEN,
    };
    let n = begin.encode(&mut b).unwrap();
    pc.ok(relay, Cmd::XferBegin, &b[..n]);
    let (hdr, body) = img.split_at(512);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(240).enumerate() {
            let mut p = vec![0u8; 16 + c.len()];
            let n = XferChunk {
                offset: (i * 240) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            pc.ok(relay, cmd, &p[..n]);
        }
    }
    assert_eq!(
        dagon_proto::payload::parse_digest(&pc.ok(relay, Cmd::XferEnd, &[])),
        Ok(sha256(body))
    );
    pc.ok(
        relay,
        Cmd::SetPending,
        &wire(|b| {
            dagon_proto::payload::SetPending {
                target: target::APP,
                slot,
            }
            .encode(b)
        }),
    );
    pc.ok(
        relay,
        Cmd::Reboot,
        &wire(|b| dagon_proto::payload::encode_reboot(reboot_mode::NORMAL, b)),
    );
}

#[test]
fn update_through_relay_trial_confirm_and_rollback() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    transfer(
        &mut pc,
        &mut relay,
        1,
        &image(1, Version::new(1, 1, 0), &[0xA5; 5000]),
    );
    let h = HelloResp::parse(&pc.ok(&mut relay, Cmd::Hello, &hello())).unwrap();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 1, 0), true));
    pc.ok(
        &mut relay,
        Cmd::Confirm,
        &wire(|b| dagon_proto::payload::encode_confirm(target::APP, b)),
    );
    relay.advance_ms(20_000);
    let st = StatusResp::parse(&pc.ok(&mut relay, Cmd::Status, &[])).unwrap();
    assert_eq!(
        (st.active, st.confirmed, st.slots[1].state),
        (1, 1, SlotState::Confirmed)
    );

    // A trial app that never confirms is rolled back by the trial watchdog.
    relay
        .i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR)
        .unwrap()
        .trial_app = TrialApp::NeverConfirms;
    let mut pc2 = Pc { seq: 100 };
    transfer(
        &mut pc2,
        &mut relay,
        0,
        &image(0, Version::new(1, 2, 0), &[0x33; 700]),
    );
    let (_, inner) = pc2.node(
        &mut relay,
        NODE_I2C_ADDR,
        Cmd::Confirm,
        &wire(|b| dagon_proto::payload::encode_confirm(target::APP, b)),
    );
    assert_eq!(inner.unwrap().0, Status::State, "self-check refuses");
    for _ in 0..4 {
        relay.advance_ms(10_000);
    }
    let st = StatusResp::parse(&pc2.ok(&mut relay, Cmd::Status, &[])).unwrap();
    assert_eq!((st.active, st.confirmed), (1, 1));
    assert_eq!(st.slots[0].state, SlotState::Reverted);
    assert_eq!(st.last_reason, reason::TRIAL_EXHAUSTED);
}

#[test]
fn power_cut_mid_transfer_keeps_the_confirmed_image() {
    let mut relay = relay_with_f401();
    let mut pc = Pc { seq: 0 };
    let node = relay.i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR).unwrap();
    let before = node.flash().ops().len();
    node.flash().arm_cut(CutPoint::During(before + 3));

    let img = image(1, Version::new(1, 1, 0), &[0x77; 4000]);
    let mut b = [0u8; 32];
    let n = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: img.len() as u32,
        hdr_len: IMAGE_HDR_LEN,
    }
    .encode(&mut b)
    .unwrap();
    pc.ok(&mut relay, Cmd::XferBegin, &b[..n]);
    let mut lost = false;
    for (cmd, part) in [(Cmd::XferHdr, &img[..512]), (Cmd::XferData, &img[512..])] {
        for (i, c) in part.chunks(240).enumerate() {
            let mut p = vec![0u8; 16 + c.len()];
            let n = XferChunk {
                offset: (i * 240) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            if pc.node(&mut relay, NODE_I2C_ADDR, cmd, &p[..n]).0 == Status::Timeout {
                lost = true;
                break;
            }
        }
    }
    assert!(lost, "node went dark at the cut");
    let node = relay.i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR).unwrap();
    assert_eq!(node.run(), Run::Off);
    node.power_on(0);
    let h = HelloResp::parse(&pc.ok(&mut relay, Cmd::Hello, &hello())).unwrap();
    assert_eq!((h.active_version, h.trial), (Version::new(1, 0, 0), false));
}

#[test]
fn uart_node_behind_a_lossy_link() {
    let mut relay = relay_with_f401();
    let mut cfg = f401_config();
    cfg.max_payload = max_payload::UART as u16;
    let mut node = SimNode::new(cfg, *kp().pk, SimFlash::stm32f401xe(9), Layout::STM32F401XE);
    node.provision(&image(0, Version::new(2, 0, 0), &[1; 100]), 0)
        .unwrap();
    let mut link = SimLink::new(3);
    link.drop_p = 1.0;
    relay.set_uart(WATCH_UART_PORT, link, node);

    let mut pc = Pc { seq: 0 };
    let hello_over = |pc: &mut Pc, relay: &mut SimRelay| {
        let inner = pc.frame(Cmd::Hello, &hello());
        let mut p = vec![0u8; 9 + inner.len()];
        let n = Relay {
            link: link_on(WATCH_UART_PORT),
            addr: link::UART_ADDR,
            timeout_ms: Some(500),
            inhibit: false,
            inner: &inner,
        }
        .encode(&mut p)
        .unwrap();
        pc.ask(relay, Cmd::Relay, &p[..n])
    };
    assert_eq!(hello_over(&mut pc, &mut relay).0, Status::Timeout);
    let link = &mut relay.uart(WATCH_UART_PORT).unwrap().link;
    link.drop_p = 0.0;
    link.delay_ms = 20;
    let (s, body) = hello_over(&mut pc, &mut relay);
    assert_eq!(s, Status::Ok);
    let (st, h) = Pc::parse(&body, pc.seq - 1, Cmd::Hello);
    assert_eq!(st, Status::Ok);
    assert_eq!(
        HelloResp::parse(&h).unwrap().active_version,
        Version::new(2, 0, 0)
    );
}

/// The request payload `enc` writes.
#[allow(dead_code)]
fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 1100];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}

/// The bulk of an OK RELAY or I2C_RAW response; any other body as it is.
#[allow(dead_code)]
fn bulk_of(cmd: u8, status: dagon_proto::Status, body: &[u8]) -> &[u8] {
    match dagon_proto::Cmd::from_u8(cmd) {
        Some(c @ (dagon_proto::Cmd::Relay | dagon_proto::Cmd::I2cRaw))
            if status == dagon_proto::Status::Ok =>
        {
            dagon_proto::payload::parse_bulk_resp(c, body).unwrap()
        }
        _ => body,
    }
}
