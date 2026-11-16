//! What a write sends to make a node show something: only the progress of
//! the node being written, to that node, whether it is the one the PC is
//! connected to or one reached over a link; never a copy of the topology
//! and never a word to a node that is not being written. Run with an
//! RP2040, an STM32F401 and an ESP32-S3 as the connected node.

use std::cell::RefCell;
use std::rc::Rc;

use dagon_agent::Config;
use dagon_format::bundle::dest;
use dagon_format::header::{image_kind, target_tag};
use dagon_format::topology::{self as topo, TopoEnd, TopoLink, TopoNode, NONE};
use dagon_format::Version;
use dagon_host::{
    Host, HostClock, LocalNode, Outcome, RecoverAssets, RecoverPath, Target, Transport,
};
use dagon_port::RelayLinkDef;
use dagon_proto::payload::{phase, target, Relay, Show, XferBegin};
use dagon_proto::{flags, max_payload, Cmd, Frame};
use dagon_sign::image::{self, sign_image, ImageSpec};
use dagon_sign::release::{assemble, ReleaseItem};
use dagon_sim::{block_on, Layout, SimFlash, SimNode, SimRelay};
use dagon_topology::defs::{AddressingSpec, McuDef, Registry, RelayLinkSpec};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data).
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const OLD: Version = Version::new(1, 0, 0);
const NEW: Version = Version::new(1, 1, 0);
/// The linked node's place on the connected node's I2C port.
const ADDR: u8 = 0x42;
/// Topology indices: the connected node, then the linked one.
const CONNECTED: u8 = 0;
const LINKED: u8 = 1;
const LINKED_TAG: &str = "linked-f401";
/// The linked F401's I2C port (`i2c3`) in its MCU definition.
const F401_I2C3: u8 = 3;

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new(SEED))
}

/// One kind of node the PC is connected to: its MCU, how it boots, and the
/// I2C port of its MCU definition the linked node is on.
struct Connected {
    mcu: &'static str,
    port_kind: &'static str,
    i2c: &'static str,
}

const RP2040: Connected = Connected {
    mcu: "rp2040",
    port_kind: "self-boot",
    i2c: "i2c0",
};
const STM32F401: Connected = Connected {
    mcu: "stm32f401re",
    port_kind: "self-boot",
    i2c: "i2c1",
};
const ESP32S3: Connected = Connected {
    mcu: "esp32s3",
    port_kind: "vendor-stage2",
    i2c: "i2c0",
};

fn connected_tag(c: &Connected) -> String {
    format!("conn-{}", c.mcu)
}

/// The connected MCU's built-in definition, relaying over its I2C port.
fn definition(c: &Connected) -> McuDef {
    let mut def = Registry::builtin().mcu(c.mcu).unwrap().clone();
    def.relay_links = vec![RelayLinkSpec {
        id: 1,
        port: c.i2c.into(),
        addressing: AddressingSpec::SevenBit,
        max_payload: max_payload::I2C as u16,
    }];
    def
}

/// The canonical binary: the connected node and an F401 on its I2C port,
/// wired to its NRST and BOOT0 for the F401's ROM loader.
fn topology(c: &Connected, def: &McuDef) -> Vec<u8> {
    let node = |index, mcu: &str, port_kind: &str, tag: &str, order_rank| TopoNode {
        index,
        role: topo::role::NODE,
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        port_kind: image::port_kind_of(port_kind).unwrap(),
        max_attempts: 3,
        listen_window_ms: 0,
        map_id: topo::map_id::DEFAULT,
        recovery_tiers: [topo::tier::LOCAL, 0, 0],
        order_rank,
        target_tag: target_tag(tag),
    };
    let nodes = [
        node(CONNECTED, c.mcu, c.port_kind, &connected_tag(c), 1),
        node(LINKED, "stm32f401re", "self-boot", LINKED_TAG, 0),
    ];
    let links = [TopoLink {
        ends: [
            TopoEnd {
                node: CONNECTED,
                port: def.port(c.i2c).unwrap().index,
                addr: NONE,
            },
            TopoEnd {
                node: LINKED,
                port: F401_I2C3,
                addr: ADDR,
            },
        ],
        iface: topo::iface::I2C_BUS,
        control: topo::control::FULL,
        tx_pin: NONE,
        rx_pin: NONE,
        nrst_pin: 1,
        boot_pin: 2,
        baud: 400_000,
        rom_entry: topo::rom_entry::STM32_I2C_ROM,
        bus_id: 0,
        gpio_end: 0,
    }];
    let mut out = vec![0u8; topo::total_size(nodes.len(), links.len(), 0)];
    let n = topo::encode(&nodes, &links, &[], &mut out).unwrap();
    out.truncate(n);
    out
}

fn app(mcu: &str, port_kind: &str, tag: &str, slot: u8, v: Version) -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::APP);
    let spec = ImageSpec {
        image_kind: image_kind::APP,
        port_kind: image::port_kind_of(port_kind).unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::APP, slot).unwrap(),
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: target_tag(tag),
        proto_version: 1,
        capabilities: caps,
    };
    // Over USB too, the bytes take more than ten chunks.
    sign_image(&spec, &[slot ^ 0x5A; 16 * 1024], &key()).unwrap()
}

/// `node`'s app on [`NEW`] for each app slot its MCU definition gives (one
/// image bound to any slot where it gives one).
fn apps(node: u8, mcu: &str, port_kind: &str, tag: &str) -> Vec<(u8, Vec<u8>)> {
    (0..2)
        .filter(|&s| image::slot(mcu, image_kind::APP, s).is_some())
        .map(|s| (node, app(mcu, port_kind, tag, s, NEW)))
        .collect()
}

/// Both nodes on [`NEW`], the linked node first.
fn release(c: &Connected, bin: &[u8]) -> Vec<u8> {
    let mut images = apps(LINKED, "stm32f401re", "self-boot", LINKED_TAG);
    images.extend(apps(CONNECTED, c.mcu, c.port_kind, &connected_tag(c)));
    let items: Vec<_> = images
        .iter()
        .map(|(node, image)| ReleaseItem {
            image,
            node_index: *node,
            dest: dest::SUB_STORE,
        })
        .collect();
    assemble("progress", 1, &items, bin, &key()).unwrap().bundle
}

fn config(c: &Connected) -> (Config, SimFlash, Layout) {
    let (mut cfg, flash, layout) = match c.mcu {
        "rp2040" => (
            dagon_sim::hub_config(),
            SimFlash::rp2040_2m(1),
            Layout::RP2040_HUB,
        ),
        "stm32f401re" => (
            dagon_sim::f401_config(),
            SimFlash::stm32f401xe(1),
            Layout::STM32F401XE,
        ),
        _ => (
            dagon_sim::watch_config(),
            SimFlash::esp32s3_4m(1),
            Layout::ESP32S3_WATCH,
        ),
    };
    cfg.target_tag = target_tag(&connected_tag(c));
    cfg.max_payload = max_payload::USB as u16;
    (cfg, flash, layout)
}

/// The connected node and the linked F401, both on [`OLD`], the F401 wired
/// to NRST and BOOT0.
fn relay(c: &Connected, def: &McuDef) -> SimRelay {
    let (cfg, flash, layout) = config(c);
    let mut node = SimNode::new(cfg, *key().pk, flash, layout);
    node.provision(&app(c.mcu, c.port_kind, &connected_tag(c), 0, OLD), 0)
        .unwrap();
    let mut f401 = dagon_sim::f401_config();
    f401.target_tag = target_tag(LINKED_TAG);
    let mut linked = SimNode::new(
        f401,
        *key().pk,
        SimFlash::stm32f401xe(2),
        Layout::STM32F401XE,
    );
    linked.i2c_addr = Some(ADDR);
    linked
        .provision(&app("stm32f401re", "self-boot", LINKED_TAG, 0, OLD), 0)
        .unwrap();
    let mut r = SimRelay::with_definition(node, def);
    r.add_i2c(c.i2c, linked, true);
    r
}

/// Where a request went: the connected node, or a node over a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dest {
    Local,
    Linked(u8, u8),
}

/// One request as the node it reached reads it.
#[derive(Clone, Debug)]
struct Sent {
    to: Dest,
    cmd: u8,
    payload: Vec<u8>,
}

/// USB to the simulated connected node, recording every request.
#[derive(Clone)]
struct Usb {
    relay: Rc<RefCell<SimRelay>>,
    pending: Vec<u8>,
    sent: Rc<RefCell<Vec<Sent>>>,
}

impl Transport for Usb {
    type Error = ();
    fn max_payload(&self) -> usize {
        max_payload::USB
    }
    fn reconnect_kind(&self) -> dagon_host::Reconnect {
        dagon_host::Reconnect::Reopen
    }
    async fn write(&mut self, bytes: &[u8]) -> Result<(), ()> {
        if let Ok((f, _)) = Frame::decode(bytes, max_payload::USB) {
            if f.flags & flags::RESP == 0 {
                self.sent.borrow_mut().push(sent(&f));
            }
        }
        self.relay.borrow_mut().usb_write(bytes);
        Ok(())
    }
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        self.pending.extend(self.relay.borrow_mut().usb_read());
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
    async fn reopen(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl HostClock for Usb {
    fn now_ms(&self) -> u64 {
        self.relay.borrow().now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.relay.borrow_mut().advance_ms(ms as u64);
    }
}

fn sent(f: &Frame<'_>) -> Sent {
    if f.cmd == Cmd::Relay as u8 {
        let r = Relay::parse(f.payload).unwrap();
        let (inner, _) = Frame::decode(r.inner, max_payload::USB).unwrap();
        return Sent {
            to: Dest::Linked(r.link, r.addr),
            cmd: inner.cmd,
            payload: inner.payload.to_vec(),
        };
    }
    Sent {
        to: Dest::Local,
        cmd: f.cmd,
        payload: f.payload.to_vec(),
    }
}

/// The release, the topology and the simulated nodes behind one USB link.
struct World {
    c: &'static Connected,
    links: Vec<RelayLinkDef>,
    bin: Vec<u8>,
    bundle: Vec<u8>,
    usb: Usb,
}

impl World {
    fn new(c: &'static Connected) -> World {
        let def = definition(c);
        let bin = topology(c, &def);
        let bundle = release(c, &bin);
        let usb = Usb {
            relay: Rc::new(RefCell::new(relay(c, &def))),
            pending: Vec::new(),
            sent: Rc::default(),
        };
        World {
            c,
            links: def.relay_link_defs(),
            bin,
            bundle,
            usb,
        }
    }

    /// A host connected to node [`CONNECTED`], relaying over its table.
    fn host(&self) -> Host<Usb, Usb> {
        let mut h = Host::new(self.usb.clone(), self.usb.clone());
        h.local = Ok(LocalNode {
            index: CONNECTED,
            links: self.links.clone(),
        });
        h
    }

    /// Where requests to topology node `node` go.
    fn dest_of(&self, node: u8) -> Dest {
        match self.host().target(&self.bin, node).unwrap() {
            Target::Local => Dest::Local,
            Target::Linked { link, addr, .. } => Dest::Linked(link, addr),
        }
    }

    /// The progress number node `node`'s own app reads.
    fn told(&self, node: u8) -> Option<u8> {
        let mut r = self.usb.relay.borrow_mut();
        if node == CONNECTED {
            r.node.app_snapshot().percent
        } else {
            r.i2c_node(self.c.i2c, ADDR).unwrap().app_snapshot().percent
        }
    }

    fn sent(&self) -> Vec<Sent> {
        self.usb.sent.borrow().clone()
    }
}

fn topology_transfers(sent: &[Sent]) -> usize {
    sent.iter()
        .filter(|s| s.cmd == Cmd::XferBegin as u8)
        .filter(|s| {
            #[allow(deprecated)]
            let reserved = target::TOPOLOGY;
            XferBegin::parse(&s.payload).unwrap().target == reserved
        })
        .count()
}

fn shows(sent: &[Sent]) -> impl Iterator<Item = (usize, &Sent)> {
    sent.iter()
        .enumerate()
        .filter(|(_, s)| s.cmd == Cmd::Show as u8)
}

/// Apply writes both nodes; the node being written is told its number at
/// every step of its bytes, and every SHOW goes to that node only. No
/// topology transfer goes anywhere.
fn apply_tells_only_the_node_being_written(c: &'static Connected) {
    let w = World::new(c);
    let mut h = w.host();
    // At each step the node's app holds the number the previous step told.
    let mut told: [Option<u8>; 2] = [None, None];
    let mut steps = [0u32; 2];
    let out = block_on(h.apply(&w.bundle, &w.bin, &key().pk, None, &mut |e| {
        let n = usize::from(e.node);
        if e.phase == phase::TRANSFER {
            if let Some(was) = told[n] {
                assert_eq!(w.told(e.node), Some(was), "{}: {e:?}", c.mcu);
                steps[n] += 1;
            }
            told[n] = Some(e.shown);
        }
    }))
    .unwrap_or_else(|e| panic!("{}: {e:?}", c.mcu));
    for r in &out {
        assert!(
            matches!(r.outcome, Outcome::Done(v) if v == NEW),
            "{}: node {} {:?}",
            c.mcu,
            r.node,
            r.outcome
        );
    }
    assert_eq!(out.len(), 2, "{}", c.mcu);
    assert!(steps.iter().all(|&s| s >= 5), "{}: {steps:?}", c.mcu);

    let sent = w.sent();
    assert_eq!(topology_transfers(&sent), 0, "{}", c.mcu);
    let xfer = [
        Cmd::XferBegin as u8,
        Cmd::XferData as u8,
        Cmd::XferEnd as u8,
    ];
    let mut count = 0;
    for (i, s) in shows(&sent) {
        count += 1;
        let show = Show::parse(&s.payload).unwrap();
        assert_eq!(s.to, w.dest_of(show.node), "{}: SHOW {i}", c.mcu);
        // The node being written: the next transfer request goes to it.
        let next = sent[i..].iter().find(|n| xfer.contains(&n.cmd));
        assert_eq!(next.map(|n| n.to), Some(s.to), "{}: SHOW {i}", c.mcu);
    }
    assert!(count > 0, "{}", c.mcu);
}

/// Stage0's bytes for the F401's ROM path (the simulator runs no code).
const STAGE0: [u8; 0x400] = [0x5A; 0x400];

fn f401_bootcore() -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::BOOTCORE);
    let mcu = "stm32f401re";
    let spec = ImageSpec {
        image_kind: image_kind::BOOTCORE,
        port_kind: image::port_kind_of("self-boot").unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::BOOTCORE, 0).unwrap(),
        version: OLD,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: target_tag(LINKED_TAG),
        proto_version: 1,
        capabilities: caps,
    };
    sign_image(&spec, &[0xB0; 2000], &key()).unwrap()
}

/// The linked F401, erased, comes back through its ROM loader. No agent
/// runs on it: its progress goes to the host's callback only, and nothing
/// is sent to make any node show it.
fn a_rom_write_reports_to_the_callback_only(c: &'static Connected) {
    let w = World::new(c);
    {
        let mut r = w.usb.relay.borrow_mut();
        let now = r.now_ms();
        let node = r.i2c_node(c.i2c, ADDR).unwrap();
        node.flash().erase_all().unwrap();
        node.reset(now);
    }
    let roms = dagon_host::rom::rom_maps(Registry::builtin());
    let boot = f401_bootcore();
    let assets = RecoverAssets {
        release: &w.bundle,
        topology: &w.bin,
        pubkey: &key().pk,
        bootcore: Some(&boot),
        stage0: Some(&STAGE0),
        roms: &roms,
        rom_confirmed: true,
    };
    let mut rom_events = 0;
    let (path, v) = block_on(w.host().recover(LINKED, &assets, &mut |e| {
        rom_events += usize::from(e.phase == phase::ROM);
    }))
    .unwrap_or_else(|e| panic!("{}: {e:?}", c.mcu));
    assert!(matches!(path, RecoverPath::Rom(_)), "{}: {path:?}", c.mcu);
    assert_eq!(v, NEW, "{}", c.mcu);
    assert!(rom_events > 1, "{}: {rom_events}", c.mcu);
    let sent = w.sent();
    assert_eq!(shows(&sent).count(), 0, "{}", c.mcu);
    assert_eq!(topology_transfers(&sent), 0, "{}", c.mcu);
}

#[test]
fn apply_with_an_rp2040_connected_tells_only_the_node_being_written() {
    apply_tells_only_the_node_being_written(&RP2040);
}

#[test]
fn apply_with_an_stm32f401_connected_tells_only_the_node_being_written() {
    apply_tells_only_the_node_being_written(&STM32F401);
}

#[test]
fn apply_with_an_esp32s3_connected_tells_only_the_node_being_written() {
    apply_tells_only_the_node_being_written(&ESP32S3);
}

#[test]
fn a_rom_write_with_an_rp2040_connected_reports_to_the_callback_only() {
    a_rom_write_reports_to_the_callback_only(&RP2040);
}

#[test]
fn a_rom_write_with_an_stm32f401_connected_reports_to_the_callback_only() {
    a_rom_write_reports_to_the_callback_only(&STM32F401);
}

#[test]
fn a_rom_write_with_an_esp32s3_connected_reports_to_the_callback_only() {
    a_rom_write_reports_to_the_callback_only(&ESP32S3);
}
