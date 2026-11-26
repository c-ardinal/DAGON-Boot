//! UART_RAW through the simulated relaying node, to the ROM serial loader
//! of the sample's watch (ESP32-S3 on the PIO UART link).

use dagon_proto::payload::{self, link, rom_method, EnterRom, HelloResp, Relay, UartRaw};
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use dagon_romlink::esp;
use dagon_sim::*;

fn uart() -> u8 {
    store::relay_link_id("rp2040", WATCH_UART_PORT)
}

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

    /// The status and, of an OK answer with a bulk, the bulk.
    fn parse(resp: &[u8], seq: u8, cmd: Cmd) -> (Status, Vec<u8>) {
        let (f, _) = Frame::decode(resp, max_payload::USB).unwrap();
        assert_eq!((f.flags, f.seq, f.cmd), (flags::RESP, seq, cmd as u8));
        let (s, body) = split_status(f.payload).unwrap();
        let body = match (s, cmd.spec().resp.bulk) {
            (Status::Ok, true) => payload::parse_bulk_resp(cmd, body).unwrap(),
            _ => body,
        };
        (s, body.to_vec())
    }

    fn ask(&mut self, relay: &mut SimRelay, cmd: Cmd, payload: &[u8]) -> (Status, Vec<u8>) {
        let req = self.frame(cmd, payload);
        let resp = relay.request(&req).expect("the relaying node answers");
        Pc::parse(&resp, self.seq, cmd)
    }

    fn raw(&mut self, relay: &mut SimRelay, raw: UartRaw<'_>) -> (Status, Vec<u8>) {
        let mut p = vec![0u8; UartRaw::OVERHEAD_MAX + raw.data.len()];
        let n = raw.encode(&mut p).unwrap();
        self.ask(relay, Cmd::UartRaw, &p[..n])
    }

    /// RELAY to the watch: the relay's status and the watch's answer.
    fn watch(
        &mut self,
        relay: &mut SimRelay,
        cmd: Cmd,
        payload: &[u8],
    ) -> (Status, Option<(Status, Vec<u8>)>) {
        let inner = self.frame(cmd, payload);
        let seq = self.seq;
        let mut p = vec![0u8; Relay::OVERHEAD_MAX + inner.len()];
        let n = Relay {
            link: uart(),
            addr: link::UART_ADDR,
            timeout_ms: Some(200),
            inner: &inner,
        }
        .encode(&mut p)
        .unwrap();
        let (s, body) = self.ask(relay, Cmd::Relay, &p[..n]);
        (s, (s == Status::Ok).then(|| Pc::parse(&body, seq, cmd)))
    }

    fn hello(&mut self, relay: &mut SimRelay) -> Option<HelloResp> {
        let mut p = [0u8; 16];
        let n = payload::HelloReq {
            proto_ver: 2,
            max_payload: max_payload::UART as u16,
        }
        .encode(&mut p)
        .unwrap();
        let (_, inner) = self.watch(relay, Cmd::Hello, &p[..n]);
        inner.map(|(s, body)| {
            assert_eq!(s, Status::Ok);
            HelloResp::parse(&body).unwrap()
        })
    }

    fn enter_rom(&mut self, relay: &mut SimRelay, method: u8) -> Option<Status> {
        let mut p = [0u8; 4];
        let n = EnterRom { method }.encode(&mut p).unwrap();
        self.watch(relay, Cmd::EnterRom, &p[..n]).1.map(|(s, _)| s)
    }
}

fn sync() -> Vec<u8> {
    let mut payload = [0x55u8; 36];
    payload[..4].copy_from_slice(&[0x07, 0x07, 0x12, 0x20]);
    let mut out = [0u8; 96];
    let n = esp::request(&mut out, esp::cmd::SYNC, 0, &payload, &[]);
    out[..n].to_vec()
}

fn ask(data: &[u8], read_max: u16, wait_ms: u16) -> UartRaw<'_> {
    UartRaw {
        link: uart(),
        read_max,
        wait_ms,
        data,
    }
}

#[test]
fn the_loader_answers_over_uart_raw_once_the_node_entered_it() {
    let mut relay = sample_hub(7);
    let mut pc = Pc { seq: 0 };
    assert!(pc.hello(&mut relay).is_some());

    // The app holds the line: raw bytes get no answer, after the whole wait.
    let t0 = relay.now_ms();
    let (st, read) = pc.raw(&mut relay, ask(&sync(), 256, 120));
    assert_eq!((st, read.len()), (Status::Ok, 0));
    assert!(relay.now_ms() - t0 >= 120);
    assert!(pc.hello(&mut relay).is_some(), "the app still answers");

    // A method its definition does not list leaves it where it is.
    assert_ne!(pc.enter_rom(&mut relay, rom_method::PMIC), Some(Status::Ok));
    assert!(!relay.uart(WATCH_UART_PORT).unwrap().node.in_download());

    assert_eq!(
        pc.enter_rom(&mut relay, rom_method::FORCE_DOWNLOAD_BOOT),
        Some(Status::Ok)
    );
    assert!(relay.uart(WATCH_UART_PORT).unwrap().node.in_download());
    assert!(pc.hello(&mut relay).is_none(), "no agent in the loader");

    // What the loader printed, then its SYNC answers, in one read.
    let (st, read) = pc.raw(&mut relay, ask(&sync(), 512, 100));
    assert_eq!(st, Status::Ok);
    let text = String::from_utf8_lossy(&read);
    assert!(text.starts_with("ESP-ROM:"), "{text}");
    let answer = [0xC0, 0x01, esp::cmd::SYNC];
    let answers = read.windows(3).filter(|w| *w == answer).count();
    assert_eq!(answers, esp_rom::SYNC_ANSWERS);

    // What a read leaves behind is lost: the next read starts clean.
    let (_, first) = pc.raw(&mut relay, ask(&sync(), 20, 100));
    assert_eq!(first.len(), 20);
    let (st, rest) = pc.raw(&mut relay, ask(&[], 512, 30));
    assert_eq!((st, rest.len()), (Status::Ok, 0));
}

#[test]
fn uart_raw_is_refused_before_a_port_is_used() {
    let mut relay = sample_hub(8);
    let mut pc = Pc { seq: 0 };
    let limit = relay
        .links()
        .iter()
        .find(|l| l.id == uart())
        .unwrap()
        .buffer_len();
    let ops = relay.bus.ops;
    let i2c = store::relay_link_id("rp2040", NODE_I2C_PORT);
    let none = (1..=u8::MAX)
        .find(|id| relay.links().iter().all(|l| l.id != *id))
        .unwrap();
    let long = vec![0u8; limit + 1];
    for (what, raw) in [
        (
            "an I2C link",
            UartRaw {
                link: i2c,
                ..ask(&[], 1, 10)
            },
        ),
        (
            "no link of the node",
            UartRaw {
                link: none,
                ..ask(&[], 1, 10)
            },
        ),
        ("a read over the buffer", ask(&[], limit as u16 + 1, 10)),
        ("data over the buffer", ask(&long, 0, 10)),
    ] {
        assert_eq!(pc.raw(&mut relay, raw).0, Status::Arg, "{what}");
        assert_eq!(relay.bus.ops, ops, "{what}");
    }
    // At the buffer the port is used.
    assert_eq!(pc.raw(&mut relay, ask(&[], limit as u16, 10)).0, Status::Ok);
    assert_eq!(relay.bus.ops, ops + 1);

    // A UART link nothing is wired to.
    let other = store::relay_link_id("rp2040", "pio-uart1");
    let raw = UartRaw {
        link: other,
        ..ask(&[], 1, 10)
    };
    assert_eq!(pc.raw(&mut relay, raw).0, Status::Unsupported);
}
