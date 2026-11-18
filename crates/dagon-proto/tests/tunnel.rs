//! The tunnel a relay offers over each of its links: the largest inner
//! frame it forwards, one hop and nested, and the inner frames it refuses
//! before forwarding anything.

use dagon_proto::payload::Relay;
use dagon_proto::{finish, frame_len, max_payload, Cmd, Frame, HEADER_LEN, PROTO_VERSION};

const SIZES: [usize; 6] = [
    0,
    8,
    64,
    max_payload::I2C,
    max_payload::UART,
    max_payload::USB,
];

/// A request frame of `len` bytes in all (command 0x7F, opaque payload).
fn frame(len: usize) -> Vec<u8> {
    let mut f = vec![0x5Au8; len];
    finish(&mut f, 0, 1, 0x7F, len - frame_len(0)).unwrap();
    f
}

/// The links of the samples: the sizes are checked, nothing is relayed.
const FIRST_LINK: u8 = 1;
const SECOND_LINK: u8 = 2;

fn relay(inner: &[u8]) -> Relay<'_> {
    Relay {
        link: FIRST_LINK,
        addr: 0x42,
        timeout_ms: Some(300),
        inhibit: false,
        inner,
    }
}

#[test]
fn one_hop_is_the_smallest_of_the_link_frame_the_request_and_the_response() {
    for ingress in SIZES {
        for egress in SIZES {
            let want = frame_len(egress)
                .min(ingress.saturating_sub(Relay::OVERHEAD_MAX))
                .min(ingress.saturating_sub(Relay::RESP_OVERHEAD));
            assert_eq!(
                Relay::tunnel_mtu(ingress, egress),
                want,
                "{ingress} {egress}"
            );
        }
    }
    // USB uplink, I2C link: one I2C frame.
    assert_eq!(
        Relay::tunnel_mtu(max_payload::USB, max_payload::I2C),
        frame_len(max_payload::I2C)
    );
    // I2C uplink, UART link: what a RELAY on the I2C link carries.
    assert_eq!(
        Relay::tunnel_mtu(max_payload::I2C, max_payload::UART),
        max_payload::I2C - Relay::OVERHEAD_MAX
    );
    // An OK response wraps the node's frame in its status and delimiter.
    assert_eq!(Relay::RESP_OVERHEAD, 2);
}

#[test]
fn the_tunnel_mtu_frame_fits_its_relay_request_and_response() {
    for ingress in [64, max_payload::I2C, max_payload::UART, max_payload::USB] {
        for egress in [64, max_payload::I2C, max_payload::UART] {
            let mtu = Relay::tunnel_mtu(ingress, egress);
            let inner = frame(mtu);
            let mut p = vec![0u8; ingress + 64];
            let n = relay(&inner).encode(&mut p).unwrap();
            assert!(n <= ingress, "request {n} > {ingress}");
            assert!(Relay::RESP_OVERHEAD + mtu <= ingress, "response");
            assert!(mtu <= frame_len(egress));
        }
    }
}

/// Two relays deep: the second relay's ingress is what the first tunnel
/// carries as payload, so the formula applies hop by hop.
#[test]
fn two_hops_apply_the_formula_to_the_first_tunnels_payload() {
    let hops = |up: usize, links: &[usize]| {
        links.iter().fold(up + frame_len(0), |mtu, &egress| {
            Relay::tunnel_mtu(mtu - frame_len(0), egress)
        })
    };
    // USB -> UART -> I2C: the I2C frame.
    let first = Relay::tunnel_mtu(max_payload::USB, max_payload::UART);
    assert_eq!(first, frame_len(max_payload::UART));
    let second = Relay::tunnel_mtu(first - frame_len(0), max_payload::I2C);
    assert_eq!(second, frame_len(max_payload::I2C));
    assert_eq!(
        hops(max_payload::USB, &[max_payload::UART, max_payload::I2C]),
        second
    );
    // USB -> I2C -> UART: the I2C payload less a RELAY's overhead.
    let first = Relay::tunnel_mtu(max_payload::USB, max_payload::I2C);
    let second = Relay::tunnel_mtu(first - frame_len(0), max_payload::UART);
    assert_eq!(second, max_payload::I2C - Relay::OVERHEAD_MAX);
    assert_eq!(
        hops(max_payload::USB, &[max_payload::I2C, max_payload::UART]),
        second
    );

    // A frame of the second tunnel's size, wrapped once per hop, fits each
    // link on the way.
    let inner = frame(second);
    let mut p = vec![0u8; max_payload::USB];
    let n = Relay {
        link: SECOND_LINK,
        ..relay(&inner)
    }
    .encode(&mut p)
    .unwrap();
    let mut mid = vec![0u8; frame_len(n)];
    mid[HEADER_LEN..HEADER_LEN + n].copy_from_slice(&p[..n]);
    let m = finish(&mut mid, 0, 2, Cmd::Relay as u8, n).unwrap();
    assert!(m <= first, "{m} > {first}");
    let n = relay(&mid[..m]).encode(&mut p).unwrap();
    assert!(n <= max_payload::USB);
}

#[test]
fn a_relay_admits_one_whole_frame_of_this_wire_within_the_tunnel() {
    let (ingress, egress) = (max_payload::I2C, max_payload::UART);
    let mtu = Relay::tunnel_mtu(ingress, egress);
    let ok = frame(mtu);
    assert!(Frame::decode(&ok, egress).is_ok());
    assert!(relay(&ok).admits(ingress, egress));
    assert!(relay(&frame(16)).admits(ingress, egress));

    // Over the tunnel, though within the link's frame.
    let over = frame(mtu + 1);
    assert!(over.len() <= frame_len(egress));
    assert!(!relay(&over).admits(ingress, egress));

    let bad = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut f = frame(32);
        edit(&mut f);
        relay(&f).admits(ingress, egress)
    };
    assert!(!bad(&|f| f[0] = 0), "start");
    assert!(!bad(&|f| f[2] = PROTO_VERSION + 1), "version");
    assert!(!bad(&|f| f[12] ^= 1), "CRC");
    assert!(!bad(&|f| f.push(0)), "a byte after the frame");
    assert!(
        !bad(&|f| {
            f.pop();
        }),
        "a frame cut short"
    );
    assert!(!bad(&|f| f.truncate(4)), "less than a header");
    assert!(!relay(&[]).admits(ingress, egress), "nothing");
}
