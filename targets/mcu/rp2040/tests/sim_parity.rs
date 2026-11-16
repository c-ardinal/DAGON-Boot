//! The relay command argument rules against dagon-sim's relaying node with no node
//! answering: a request the rules reject must get the same status from the
//! sim; one they accept reaches the bus (and times out there).

use std::sync::atomic::{AtomicU8, Ordering};

use dagon_mcu_rp2040::{RELAY_LINKS, RELAY_LINK_I2C0 as I2C, RELAY_LINK_PIO_UART as UART};
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

fn relay(h: &mut SimRelay, l: u8, addr: u8, inner_len: usize) -> Status {
    let inner = vec![0u8; inner_len];
    let r = Relay {
        link: l,
        addr,
        timeout_ms: 20,
        inner: &inner,
    };
    let mut p = vec![0u8; 4 + inner_len];
    let n = r.encode(&mut p).unwrap();
    ask(h, Cmd::Relay, &p[..n])
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
        let want = if cmd::relay_fits(&RELAY_LINKS, l, a, len) {
            Status::Timeout
        } else {
            Status::Arg
        };
        assert_eq!(
            relay(&mut h, l, a, len),
            want,
            "link {l} addr {a:#x} len {len}"
        );
    }
    // Short timeout: an absent I2C node answers within about the timeout.
    let t0 = h.now_ms();
    assert_eq!(relay(&mut h, I2C, 0x55, 12), Status::Timeout);
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
        let mut p = [0u8; 8];
        let n = r.encode(&mut p).unwrap();
        p[..n].to_vec()
    };
    let subctl = |n: u8, b: u8| [n, b];
    let rule = |p: &[u8], high| cmd::i2c_raw(p, high).err();

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
    let long = raw(cmd::RAW_READ_MAX as u16 + 1);
    assert_eq!(rule(&long, true), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::I2cRaw, &long), Status::Arg);
    assert_eq!(rule(&[1, 0], true), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::I2cRaw, &[1, 0]), Status::Arg);
    // Accepted by the rules: the sim's ROM loader answers.
    assert_eq!(rule(&raw(1), true), None);
    // Out-of-range SUBCTL values are refused by the shared parser.
    assert_eq!(SubCtl::parse(&[3, 0]), Err(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::SubCtl, &[3, 0]), Status::Arg);
    // SHOW, which the relaying node's own agent answers: a text over 32
    // bytes is E_ARG in both.
    let mut show = [0u8; 7 + 33];
    show[1] = 1;
    assert_eq!(Show::parse(&show).err(), Some(Status::Arg));
    assert_eq!(ask(&mut h, Cmd::Show, &show), Status::Arg);
    assert_eq!(ask(&mut h, Cmd::Show, &show[..7]), Status::Ok);
}
