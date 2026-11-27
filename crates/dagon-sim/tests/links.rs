//! The relaying node relays over every link its table lists: one simulated
//! bus per I2C port and one stream per UART port, routed by link id. The
//! relay table here is this test's own, in the RP2040 definition.

use dagon_port::{Addressing, LinkKind, RelayLinkDef};
use dagon_proto::payload::{HelloReq, HelloResp, Relay};
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::timing::{exchange_ms, Medium};
use dagon_sim::*;
use dagon_topology::defs::{AddressingSpec, McuDef, Registry, RelayLinkSpec};

const ADDR: u8 = NODE_I2C_ADDR;
const STREAM_ADDR: u8 = 0xFF;

/// The index of port `name` in the RP2040 definition.
fn index(name: &str) -> u8 {
    dagon_topology::defs::Registry::builtin()
        .mcu("rp2040")
        .and_then(|m| m.port(name))
        .unwrap_or_else(|| panic!("rp2040 has no port {name}"))
        .index
}

fn i2c(id: u8, port: &str) -> RelayLinkDef {
    RelayLinkDef {
        id,
        port: index(port),
        kind: LinkKind::I2c,
        addressing: Addressing::SevenBit,
        max_payload: max_payload::I2C as u16,
    }
}

fn uart(id: u8, port: &str) -> RelayLinkDef {
    RelayLinkDef {
        id,
        port: index(port),
        kind: LinkKind::Uart,
        addressing: Addressing::Fixed(STREAM_ADDR),
        max_payload: max_payload::UART as u16,
    }
}

/// The RP2040 definition relaying over two I2C ports and two UART ports,
/// with ids of this test's choosing.
fn definition() -> McuDef {
    let spec = |id, port: &str, addressing, max: usize| RelayLinkSpec {
        id,
        port: port.into(),
        addressing,
        max_payload: max as u16,
    };
    let (seven, fixed) = (AddressingSpec::SevenBit, AddressingSpec::Fixed(STREAM_ADDR));
    let mut def = Registry::builtin().mcu("rp2040").unwrap().clone();
    def.relay_links = vec![
        spec(7, "i2c0", seven, max_payload::I2C),
        spec(9, "i2c1", seven, max_payload::I2C),
        spec(12, "uart0", fixed, max_payload::UART),
        spec(14, "pio-uart", fixed, max_payload::UART),
    ];
    def
}

fn relaying_node() -> SimNode {
    SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_4m(1),
        Layout::of("rp2040"),
    )
}

/// An F401 at [`ADDR`] whose HELLO answers with `max` as its payload size,
/// so the test can tell which node answered.
fn f401(seed: u64, max: u16) -> SimNode {
    let mut cfg = f401_config();
    cfg.max_payload = max;
    let mut n = SimNode::new(
        cfg,
        sample::PUBKEY,
        SimFlash::stm32f401xe(seed),
        Layout::of("stm32f401re"),
    );
    n.i2c_addr = Some(ADDR);
    n.store().provision_core(sample::SEG_CORE_1_0_0).unwrap();
    n.provision(sample::SEG_1_0_0, 0).expect("seg fixture fits");
    n
}

fn watch(seed: u64, max: u16) -> SimNode {
    let mut cfg = watch_config();
    cfg.max_payload = max;
    let mut n = SimNode::new(
        cfg,
        sample::PUBKEY,
        SimFlash::esp32s3(seed),
        Layout::of("esp32s3"),
    );
    n.provision(sample::WATCH_1_0_0, 0)
        .expect("watch fixture fits");
    n
}

fn frame(cmd: Cmd, payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; dagon_proto::frame_len(payload.len())];
    let n = Frame {
        flags: 0,
        seq: 1,
        cmd: cmd as u8,
        payload,
    }
    .encode(&mut b)
    .unwrap();
    b.truncate(n);
    b
}

fn relay_frame(link: u8, addr: u8, inner: &[u8]) -> Vec<u8> {
    let mut p = vec![0u8; Relay::OVERHEAD_MAX + inner.len()];
    let n = Relay {
        link,
        addr,
        timeout_ms: Some(300),
        inner,
    }
    .encode(&mut p)
    .unwrap();
    frame(Cmd::Relay, &p[..n])
}

fn hello() -> Vec<u8> {
    let mut b = [0u8; 6];
    let n = HelloReq {
        proto_ver: 1,
        max_payload: max_payload::I2C as u16,
    }
    .encode(&mut b)
    .unwrap();
    frame(Cmd::Hello, &b[..n])
}

/// RELAY a HELLO over `link` to `addr`: the relay status, and the
/// `max_payload` of the node that answered.
fn hello_over(r: &mut SimRelay, link: u8, addr: u8) -> (Status, Option<u16>) {
    let resp = r
        .request(&relay_frame(link, addr, &hello()))
        .expect("answers");
    let (f, _) = Frame::decode(&resp, max_payload::USB).unwrap();
    let (s, body) = split_status(f.payload).unwrap();
    let body = bulk_of(f.cmd, s, body);
    let max = (s == Status::Ok).then(|| {
        let (inner, _) = Frame::decode(body, max_payload::USB).unwrap();
        let (st, h) = split_status(inner.payload).unwrap();
        assert_eq!(st, Status::Ok);
        HelloResp::parse(h).unwrap().max_payload
    });
    (s, max)
}

#[test]
fn two_nodes_at_one_address_on_two_ports_each_answer_on_their_link() {
    let def = definition();
    let links = def.relay_link_defs();
    let mut r = SimRelay::with_definition(relaying_node(), &def);
    r.add_i2c("i2c0", f401(2, 200), false);
    r.add_i2c("i2c1", f401(3, 100), false);
    r.set_uart("uart0", SimLink::new(4), watch(5, 300));
    r.set_uart("pio-uart", SimLink::new(6), watch(7, 500));

    assert_eq!(
        hello_over(&mut r, links[0].id, ADDR),
        (Status::Ok, Some(200))
    );
    assert_eq!(
        hello_over(&mut r, links[1].id, ADDR),
        (Status::Ok, Some(100))
    );
    assert_eq!(
        hello_over(&mut r, links[2].id, STREAM_ADDR),
        (Status::Ok, Some(300))
    );
    assert_eq!(
        hello_over(&mut r, links[3].id, STREAM_ADDR),
        (Status::Ok, Some(500))
    );
}

#[test]
fn an_id_the_table_does_not_list_is_an_argument_error() {
    let def = definition();
    let links = def.relay_link_defs();
    let mut r = SimRelay::with_definition(relaying_node(), &def);
    r.add_i2c("i2c0", f401(2, 200), false);
    let unknown = (1..=254u8)
        .find(|id| links.iter().all(|l| l.id != *id))
        .unwrap();
    assert_eq!(hello_over(&mut r, unknown, ADDR), (Status::Arg, None));
    // A listed id whose port has nothing wired times out instead.
    assert_eq!(
        hello_over(&mut r, links[1].id, ADDR),
        (Status::Timeout, None)
    );
}

#[test]
fn a_link_takes_the_time_of_its_ports_kind() {
    let data = frame(Cmd::XferData, &[0; 1000]);
    // The same ids on swapped kinds: the time follows the kind, not the id.
    let a = [i2c(1, "i2c0"), uart(2, "pio-uart")];
    let b = [uart(1, "pio-uart"), i2c(2, "i2c0")];
    let t = |links: &[RelayLinkDef], id| {
        exchange_ms(
            &relay_frame(id, ADDR, &data),
            Medium::UsbCdc,
            links,
            |_, _, _| 0,
        )
    };
    assert!(t(&a, 1) < t(&a, 2));
    assert_eq!(t(&a, 1), t(&b, 2));
    assert_eq!(t(&a, 2), t(&b, 1));
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
