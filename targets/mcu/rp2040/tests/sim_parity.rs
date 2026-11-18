//! The relay command argument rules against dagon-sim's relaying node with no node
//! answering: a request the rules reject must get the same status from the
//! sim; one they accept reaches the bus (and times out there).

use std::sync::atomic::{AtomicU8, Ordering};

use dagon_mcu_rp2040::{
    RELAY_LINKS, RELAY_LINK_BUFFERS, RELAY_LINK_I2C0 as I2C, RELAY_LINK_PIO_UART as UART,
};
use dagon_port::timeout::link_limits;
use dagon_proto::payload::link::UART_ADDR;
use dagon_proto::payload::{boot0, nrst, I2cRaw, Relay, Show, SubCtl};
use dagon_proto::{finish, frame_len, max_payload, split_status, Cmd, Frame, Status};
use dagon_relay::cmd;
use dagon_sim::{demo_hub, SimRelay};

fn ask(h: &mut SimRelay, c: Cmd, payload: &[u8]) -> Status {
    // A new seq each time: a node's agent answers a repeated one with its
    // stored reply.
    static SEQ: AtomicU8 = AtomicU8::new(1);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut buf = vec![0u8; frame_len(payload.len())];
    buf[8..8 + payload.len()].copy_from_slice(payload);
    finish(&mut buf, 0, seq, c as u8, payload.len()).unwrap();
    let resp = h.request(&buf).expect("the relaying node answers");
    let (f, _) = Frame::decode(&resp, max_payload::USB).unwrap();
    split_status(f.payload).unwrap().0
}

/// A whole request frame of `len` bytes (command 0x7F, opaque payload).
fn frame(len: usize) -> Vec<u8> {
    let mut f = vec![0u8; len];
    finish(&mut f, 0, 1, 0x7F, len - frame_len(0)).unwrap();
    f
}

/// RELAY of `inner` as the sim answers it, and as the relay's rules
/// predict it from the sim's USB uplink: E_ARG when they refuse it, else
/// E_TIMEOUT (no node answers).
fn relay(h: &mut SimRelay, l: u8, addr: u8, inner: &[u8]) -> (Status, Status) {
    let r = Relay {
        link: l,
        addr,
        timeout_ms: Some(20),
        inhibit: false,
        inner,
    };
    let want = if cmd::relay_fits(&RELAY_LINKS, max_payload::USB, &r) {
        Status::Timeout
    } else {
        Status::Arg
    };
    let mut p = vec![0u8; 16 + inner.len()];
    let n = r.encode(&mut p).unwrap();
    (ask(h, Cmd::Relay, &p[..n]), want)
}

/// The generated `RELAY_LINKS` are the entries the definitions loader reads
/// from the same `[[relay_link]]`, and each port's id constant is its entry's.
#[test]
fn generated_relay_links_match_the_definition() {
    assert_eq!(
        RELAY_LINKS.to_vec(),
        dagon_sim::store::relay_links("rp2040")
    );
    assert_eq!(I2C, dagon_sim::store::relay_link_id("rp2040", "i2c0"));
    assert_eq!(UART, dagon_sim::store::relay_link_id("rp2040", "pio-uart"));
}

/// Each generated link buffer is one frame of the link's max_payload, and
/// holds the largest frame of the link's kind wherever the kind has limits
/// (the frames its driver receives): the size a port of the link takes.
#[test]
fn every_link_buffer_holds_its_links_frames() {
    assert_eq!(RELAY_LINK_BUFFERS.len(), RELAY_LINKS.len());
    for (link, &n) in RELAY_LINKS.iter().zip(&RELAY_LINK_BUFFERS) {
        assert_eq!(n, frame_len(link.max_payload.into()), "link {}", link.id);
        if let Some(l) = link_limits(link.kind) {
            assert!(n >= frame_len(l.max_payload), "link {}", link.id);
        }
        assert!(dagon_relay::fits_link(link, n), "link {}", link.id);
        assert!(!dagon_relay::fits_link(link, n - 1), "link {}", link.id);
    }
}

#[test]
fn relay_rules_match_the_sim() {
    let mut h = demo_hub([0; 32], 1);
    let i2c_max = frame_len(max_payload::I2C);
    let uart_max = frame_len(max_payload::UART);
    let cases = [
        (I2C, 0x42, 12),
        (I2C, 0x10, i2c_max),
        (I2C, 0x42, i2c_max + 1),
        (I2C, 0x80, 12),
        (UART, UART_ADDR, uart_max),
        (UART, UART_ADDR, uart_max + 1),
        (UART, 0x42, 12),
        (9, 0x42, 12),
    ];
    for (l, a, len) in cases {
        let (got, want) = relay(&mut h, l, a, &frame(len));
        assert_eq!(got, want, "link {l} addr {a:#x} len {len}");
    }
    // An inner frame that is not one whole frame: refused by both.
    let mut bad_crc = frame(16);
    bad_crc[12] ^= 1;
    let mut trailing = frame(16);
    trailing.push(0);
    for inner in [bad_crc, trailing, vec![0; 16]] {
        assert_eq!(relay(&mut h, I2C, 0x42, &inner), (Status::Arg, Status::Arg));
    }
    // Short timeout: an absent I2C node answers within about the timeout.
    let t0 = h.now_ms();
    assert_eq!(relay(&mut h, I2C, 0x55, &frame(12)).0, Status::Timeout);
    assert!(h.now_ms() - t0 <= 20 + 2 + 2, "{} ms", h.now_ms() - t0);
}

#[test]
fn i2c_raw_and_subctl_rules_match_the_sim() {
    let mut h = demo_hub([0; 32], 2);
    let raw = |read_len: u16| {
        let r = I2cRaw {
            read_len,
            data: &[0x00, 0xFF],
        };
        let mut p = [0u8; 32];
        let n = r.encode(&mut p).unwrap();
        p[..n].to_vec()
    };
    let subctl = |nrst: u8, boot0: u8| wire(|b| SubCtl { nrst, boot0 }.encode(b));
    // The ROM loader answers on the I2C link: its buffer is the limit.
    let i2c = RELAY_LINKS.iter().position(|l| l.id == I2C).unwrap();
    let (link, limit) = (RELAY_LINKS[i2c], RELAY_LINK_BUFFERS[i2c]);
    let rule = |p: &[u8], high| {
        let checked = I2cRaw::parse(p).and_then(|raw| cmd::i2c_raw(&raw, high, &link));
        checked.err()
    };

    // BOOT0 low.
    assert_eq!(
        ask(&mut h, Cmd::SubCtl, &subctl(nrst::RELEASE, boot0::LOW)),
        Status::Ok
    );
    assert_eq!(rule(&raw(1), false), Some(Status::State));
    assert_eq!(ask(&mut h, Cmd::I2cRaw, &raw(1)), Status::State);
    // BOOT0 high.
    assert_eq!(
        ask(&mut h, Cmd::SubCtl, &subctl(nrst::PULSE, boot0::HIGH)),
        Status::Ok
    );
    let long = raw(limit as u16 + 1);
    assert_eq!(rule(&long, true), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::I2cRaw, &long), Status::Arg);
    let mut long_write = vec![0u8; 16 + limit + 1];
    let n = I2cRaw {
        read_len: 0,
        data: &vec![0xFF; limit + 1],
    }
    .encode(&mut long_write)
    .unwrap();
    assert_eq!(rule(&long_write[..n], true), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::I2cRaw, &long_write[..n]), Status::Arg);
    // At the limit: accepted by both.
    assert_eq!(rule(&raw(limit as u16), true), None);
    assert_ne!(ask(&mut h, Cmd::I2cRaw, &raw(limit as u16)), Status::Arg);
    assert_eq!(rule(&[1, 0], true), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::I2cRaw, &[1, 0]), Status::Arg);
    // Accepted by the rules: the sim's ROM loader answers.
    assert_eq!(rule(&raw(1), true), None);
    // Out-of-range SUBCTL values are refused by the shared parser.
    assert_eq!(SubCtl::parse(&[3, 0]), Err(Status::Arg));
    assert_eq!(
        ask(
            &mut h,
            Cmd::SubCtl,
            &wire(|b| dagon_proto::payload::SubCtl { nrst: 3, boot0: 0 }.encode(b))
        ),
        Status::Arg
    );
    // SHOW, which the relaying node's own agent answers: a text over 32
    // bytes is E_ARG in both.
    // Core: the phase; field 4 (key 0x08): a text of 33 bytes.
    let mut show = vec![1, 0x08, 33];
    show.extend([b'x'; 33]);
    assert_eq!(Show::parse(&show).err(), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::Show, &show), Status::Arg);
    assert_eq!(ask(&mut h, Cmd::Show, &show[..1]), Status::Ok);
}

/// The request payload `enc` writes.
#[allow(dead_code)]
fn wire(enc: impl FnOnce(&mut [u8]) -> Result<usize, dagon_proto::FrameError>) -> Vec<u8> {
    let mut b = vec![0u8; 1100];
    let n = enc(&mut b).unwrap();
    b.truncate(n);
    b
}
