//! The relaying node takes its ports and its relay table from the
//! definitions it is given, a project's override included, both to route a
//! RELAY and to attach the simulated media; every RELAY outcome
//! (answered, `Arg`, `Unsupported`, `Timeout`) and I2C_RAW's port follow
//! from them.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use dagon_proto::payload::{boot0, nrst, HelloReq, HelloResp, I2cRaw, Relay};
use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
use dagon_sim::rom::{cmd, ACK};
use dagon_sim::*;
use dagon_topology::defs::{McuDef, Registry, Source};

const ADDR: u8 = NODE_I2C_ADDR;

/// A project's dagon.toml that moves `i2c0` off its built-in index, adds a
/// third I2C port, gives both their own relay ids, and lists a relay link on
/// the USB port, a kind the simulator has no relay medium for.
const OVERRIDE: &str = r#"format = 1

[[mcu_override.rp2040.link_port]]
name = "i2c0"
index = 9

[[mcu_override.rp2040.link_port]]
name = "i2c2"
index = 12
kind = "i2c"

[[mcu_override.rp2040.relay_link]]
port = "i2c0"
id = 7

[[mcu_override.rp2040.relay_link]]
port = "i2c2"
id = 9
addressing = "seven-bit"
max_payload = 256

[[mcu_override.rp2040.relay_link]]
port = "usb"
id = 20
addressing = { fixed = 0xFF }
max_payload = 1024
"#;

/// A project's dagon.toml that moves the STM32F401's ROM loader to an
/// address no built-in definition uses.
const ROM_OVERRIDE: &str = "format = 1\n\n[mcu_override.stm32f401re.rom_i2c]\naddr = 0x3B\n";

/// MCU `mcu`'s definition with the project's dagon.toml `text` merged in,
/// as a project loads it.
fn loaded(text: &str, mcu: &str) -> McuDef {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir: PathBuf =
        std::env::temp_dir().join(format!("dagon-sim-override-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    std::fs::write(&toml, text).unwrap();
    let reg = Registry::load(&[Source::Project(&toml)]);
    std::fs::remove_dir_all(&dir).unwrap();
    reg.expect("the override loads").mcu(mcu).unwrap().clone()
}

/// The RP2040 definition with [`OVERRIDE`] merged in.
fn overridden() -> McuDef {
    loaded(OVERRIDE, "rp2040")
}

/// The relay id `def` gives its port `port`.
fn id_on(def: &McuDef, port: &str) -> u8 {
    def.relay_links.iter().find(|r| r.port == port).unwrap().id
}

fn relaying_node() -> SimNode {
    SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_4m(1),
        Layout::of("rp2040"),
    )
}

/// An F401 at [`ADDR`] whose HELLO answers with `max` as its payload size.
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

/// The relaying node's own answer to `req`: status and body.
fn ask(r: &mut SimRelay, req: &[u8]) -> (Status, Vec<u8>) {
    let resp = r.request(req).expect("answers");
    let (f, _) = Frame::decode(&resp, max_payload::USB).unwrap();
    let (s, body) = split_status(f.payload).unwrap();
    let body = bulk_of(f.cmd, s, body);
    (s, body.to_vec())
}

/// RELAY a HELLO over `link` to `addr`: the relay status, and the
/// `max_payload` of the node that answered.
fn hello_over(r: &mut SimRelay, link: u8, addr: u8) -> (Status, Option<u16>) {
    let mut h = [0u8; 6];
    let n = HelloReq {
        proto_ver: 1,
        max_payload: max_payload::I2C as u16,
    }
    .encode(&mut h)
    .unwrap();
    let inner = frame(Cmd::Hello, &h[..n]);
    let mut p = vec![0u8; Relay::OVERHEAD_MAX + inner.len()];
    let n = Relay {
        link,
        addr,
        timeout_ms: Some(300),
        inner: &inner,
    }
    .encode(&mut p)
    .unwrap();
    let (s, body) = ask(r, &frame(Cmd::Relay, &p[..n]));
    let max = (s == Status::Ok).then(|| {
        let (inner, _) = Frame::decode(&body, max_payload::USB).unwrap();
        let (st, h) = split_status(inner.payload).unwrap();
        assert_eq!(st, Status::Ok);
        HelloResp::parse(h).unwrap().max_payload
    });
    (s, max)
}

#[test]
fn a_project_override_moves_and_adds_ports_and_the_relay_follows_it() {
    let def = overridden();
    assert_ne!(def.port("i2c0").unwrap().index, 1, "off the built-in index");
    let mut r = SimRelay::with_definition(relaying_node(), &def);
    r.add_i2c("i2c0", f401(2, 200), false);
    r.add_i2c("i2c2", f401(3, 100), false);
    assert_eq!(
        hello_over(&mut r, id_on(&def, "i2c0"), ADDR),
        (Status::Ok, Some(200))
    );
    assert_eq!(
        hello_over(&mut r, id_on(&def, "i2c2"), ADDR),
        (Status::Ok, Some(100))
    );
}

#[test]
fn a_listed_link_of_a_kind_with_no_simulated_medium_is_unsupported() {
    let def = overridden();
    let mut r = SimRelay::with_definition(relaying_node(), &def);
    assert_eq!(
        hello_over(&mut r, id_on(&def, "usb"), 0xFF),
        (Status::Unsupported, None)
    );
}

/// I2C_RAW goes over the relay link of the wired node's port (its buffer is
/// the limit), not the first I2C port.
#[test]
fn i2c_raw_reaches_the_wired_node_on_its_own_port() {
    let mut r = SimRelay::with_definition(relaying_node(), &overridden());
    r.add_i2c("i2c0", f401(2, 200), false);
    r.add_i2c("i2c2", f401(3, 100), true);
    let (s, _) = ask(
        &mut r,
        &frame(
            Cmd::SubCtl,
            &wire(|b| {
                dagon_proto::payload::SubCtl {
                    nrst: nrst::PULSE,
                    boot0: boot0::HIGH,
                    target: None,
                }
                .encode(b)
            }),
        ),
    );
    assert_eq!(s, Status::Ok);
    let mut p = [0u8; 32];
    let n = I2cRaw {
        target: None,
        read_len: 1,
        data: &[cmd::GET, 0xFF],
    }
    .encode(&mut p)
    .unwrap();
    assert_eq!(
        ask(&mut r, &frame(Cmd::I2cRaw, &p[..n])),
        (Status::Ok, vec![ACK]),
        "the ROM loader of the node on i2c2 answers"
    );
}

/// A node whose MCU definition moves its ROM loader is reached there: the
/// loader answers at the definition's address, and the relay's recovery,
/// built from the wired node's definition, sends I2C_RAW to it.
#[test]
fn i2c_raw_reaches_the_rom_loader_at_its_definitions_address() {
    let def = loaded(ROM_OVERRIDE, "stm32f401re");
    let at = def.rom_i2c.as_ref().map(|r| r.addr);
    assert_eq!(at, Some(0x3B));
    let mut node = f401(3, 100);
    node.rom_addr = at;
    let mut r = SimRelay::new(relaying_node());
    r.add_i2c(NODE_I2C_PORT, node, true);
    assert_eq!(
        r.rom_addr, at,
        "the relay's recovery takes the wired node's"
    );
    let (s, _) = ask(
        &mut r,
        &frame(
            Cmd::SubCtl,
            &wire(|b| {
                dagon_proto::payload::SubCtl {
                    nrst: nrst::PULSE,
                    boot0: boot0::HIGH,
                    target: None,
                }
                .encode(b)
            }),
        ),
    );
    assert_eq!(s, Status::Ok);
    let port = r.port(NODE_I2C_PORT, dagon_port::LinkKind::I2c);
    assert_eq!(
        r.bus.scan(port),
        vec![0x3B],
        "the loader answers there only"
    );
    let mut p = [0u8; 32];
    let n = I2cRaw {
        target: None,
        read_len: 1,
        data: &[cmd::GET, 0xFF],
    }
    .encode(&mut p)
    .unwrap();
    assert_eq!(
        ask(&mut r, &frame(Cmd::I2cRaw, &p[..n])),
        (Status::Ok, vec![ACK])
    );
}

/// A wired node on a port the node does not relay over has no recovery
/// link: I2C_RAW is unsupported there, as on the relay.
#[test]
fn i2c_raw_without_a_recovery_link_is_unsupported() {
    let def = loaded(
        "format = 1\n\n[[mcu_override.rp2040.relay_link]]\nport = \"i2c1\"\ndagon-remove = true\n",
        "rp2040",
    );
    let mut r = SimRelay::with_definition(relaying_node(), &def);
    r.add_i2c("i2c1", f401(3, 100), true);
    let (s, _) = ask(
        &mut r,
        &frame(
            Cmd::SubCtl,
            &wire(|b| {
                dagon_proto::payload::SubCtl {
                    nrst: nrst::PULSE,
                    boot0: boot0::HIGH,
                    target: None,
                }
                .encode(b)
            }),
        ),
    );
    assert_eq!(s, Status::Ok);
    let mut p = [0u8; 32];
    let n = I2cRaw {
        target: None,
        read_len: 1,
        data: &[cmd::GET, 0xFF],
    }
    .encode(&mut p)
    .unwrap();
    assert_eq!(
        ask(&mut r, &frame(Cmd::I2cRaw, &p[..n])).0,
        Status::Unsupported
    );
}

#[test]
#[should_panic(expected = "the definition of stm32f401re given to a node of rp2040")]
fn a_definition_of_another_mcu_is_refused() {
    let other = Registry::builtin().mcu("stm32f401re").unwrap();
    SimRelay::with_definition(relaying_node(), other);
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
