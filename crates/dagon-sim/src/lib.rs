//! DAGON simulator: deterministic stand-ins for the hardware so the host
//! tools, the browser UI and tests have a partner without boards.
//!
//! - [`SimFlash`]: geometry, operation log, power-cut points.
//! - [`SimLink`]: frame loss, bit flips, delay, silent peer.
//! - [`SimRom`]: the STM32 I2C ROM loader (ACK/NACK/BUSY, 1000 ms frame
//!   timeout, at most 256 bytes per read or write).
//! - [`SimNode`]: dagon-agent over a [`SimStore`], with a boot decision at
//!   reset, an I2C mailbox and selectable trial-app behaviour.
//! - [`SimRelay`]: answers the PC (directly or as a USB byte stream), relays
//!   over every link of its relay table (one bus per I2C port, one stream
//!   per UART port, routed by link id), drives NRST/BOOT0 and raw ROM
//!   transactions.
//! - [`TimedRelay`]: a [`SimRelay`] whose answers take the links' and
//!   flash's real time (one model for `dagon demo` and the browser demo).
//! - [`world`]: the wiring of any topology, from the definitions given.
//! - [`listen`]: when the hub's STAY reaches a node's boot core on each
//!   link kind, against the listen window its MCU definition opens.
//!
//! Randomness comes only from [`SimRng`] seeds, so a seed replays a run.
#![forbid(unsafe_code)]

pub mod flash;
pub mod link;
pub mod listen;
mod mcu;
pub mod node;
mod ports;
pub mod relay;
pub mod rng;
pub mod rom;
pub mod store;
pub mod timing;
pub mod world;

pub use flash::{CutPoint, FlashOp, OpKind, SimFlash};
pub use link::SimLink;
pub use node::{Run, SimNode, TrialApp};
pub use relay::{FrameCutter, SimBus, SimPort, SimRelay, SimStream};
pub use rng::SimRng;
pub use rom::SimRom;
pub use store::{Layout, SimStore};
pub use timing::{Medium, TimedRelay};

// The simulator declares no machine of its own: its nodes run the agent's.
dagon_events::state_machine! {}

use dagon_agent::Config;
use dagon_format::header::target_tag;
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};

/// Default I2C address of the first node.
pub const NODE_I2C_ADDR: u8 = 0x42;
/// The sample's wiring: the RP2040 port its I2C node is on, and the one
/// its UART node (the watch) is on.
pub const NODE_I2C_PORT: &str = "i2c0";
pub const WATCH_UART_PORT: &str = "pio-uart";

/// The sample topology the simulated wiring follows. The nodes' tags come
/// from it, so the simulator names no node of its own.
pub const SAMPLE_TOPOLOGY: &str = include_str!("../../../topology/sample.yaml");

/// The `target_tag` the sample topology gives node `id`.
fn sample_tag(id: &str) -> [u8; 16] {
    let t = dagon_topology::parse_str(SAMPLE_TOPOLOGY).expect("the sample topology parses");
    let node = t
        .nodes
        .iter()
        .find(|n| n.id == id)
        .expect("the sample has the node");
    target_tag(&node.tag)
}

/// What built-in MCU definition `mcu` gives a node's configuration: its
/// identity, port kind and boot attempts; the rest is a node on the sample.
fn of_mcu(mcu: &str) -> Config {
    let m = mcu::simulated_mcu(mcu);
    Config {
        mcu_id: m.mcu_id(),
        port_kind: m.port.wire(),
        target_tag: [0; 16],
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: store::trial_attempts(mcu),
    }
}

/// Hub on RP2040, answering on USB.
pub fn hub_config() -> Config {
    Config {
        target_tag: sample_tag("hub"),
        role: role::HUB,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        ..of_mcu("rp2040")
    }
}

/// STM32F401 node on I2C.
pub fn f401_config() -> Config {
    Config {
        target_tag: sample_tag("seg"),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        ..of_mcu("stm32f401re")
    }
}

/// The sample's hub with an empty flash, plus an empty F401 at
/// [`NODE_I2C_ADDR`] on [`NODE_I2C_PORT`], wired to NRST/BOOT0. Provision
/// each node's boot core with [`SimNode::provision_core`] (stage0 checks
/// its signature under `pubkey`), then its app with [`SimNode::provision`];
/// a node with nothing to run stays silent (NACK).
pub fn demo_hub(pubkey: [u8; 32], seed: u64) -> SimRelay {
    let hub = SimNode::new(
        hub_config(),
        pubkey,
        SimFlash::rp2040_4m(seed),
        Layout::of("rp2040"),
    );
    let mut f401 = SimNode::new(
        f401_config(),
        pubkey,
        SimFlash::stm32f401xe(seed ^ 1),
        Layout::of("stm32f401re"),
    );
    f401.i2c_addr = Some(NODE_I2C_ADDR);
    let mut h = SimRelay::new(hub);
    h.add_i2c(NODE_I2C_PORT, f401, true);
    h
}

/// ESP32-S3 watch on the PIO UART link (kind V header fields; the slot
/// handling is simulated like kind S).
pub fn watch_config() -> Config {
    Config {
        target_tag: sample_tag("watch"),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::UART as u16,
        ..of_mcu("esp32s3")
    }
}

/// Pre-signed sample images and release (RFC 8032 section 7.1 TEST 1
/// key, public test data), so nothing is signed at run time. Regenerated
/// and checked by the `sample_fixtures` test.
pub mod sample {
    /// The TEST 1 public key the fixtures verify under.
    pub const PUBKEY: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];
    /// Hub (RP2040) APP 1.0.0 for slot A.
    pub const HUB_1_0_0: &[u8] = include_bytes!("../fixtures/sample-hub-1.0.0.img");
    /// Seg (F401) APP 1.0.0 for slot A.
    pub const SEG_1_0_0: &[u8] = include_bytes!("../fixtures/sample-seg-1.0.0.img");
    /// Watch (ESP32-S3) APP 1.0.0, runs from any slot.
    pub const WATCH_1_0_0: &[u8] = include_bytes!("../fixtures/sample-watch-1.0.0.img");
    /// Release 1.1.0 (.dgb) for the sample topology: seg A/B, watch,
    /// hub A/B, plus the topology.
    pub const RELEASE_1_1_0: &[u8] = include_bytes!("../fixtures/sample-1.1.0.dgb");
    /// Hub (RP2040) boot core 1.0.0 for boot core slot A, the first
    /// install's ([`crate::SimStore::first_core`]).
    pub const HUB_CORE_1_0_0: &[u8] = include_bytes!("../fixtures/sample-hub-core-1.0.0.img");
    /// Seg (F401) boot core 1.0.0 for boot core slot A, likewise.
    pub const SEG_CORE_1_0_0: &[u8] = include_bytes!("../fixtures/sample-seg-core-1.0.0.img");
    /// App sizes about what the sample apps weigh, so a simulated transfer
    /// takes its real time: the images of `dagon demo`, and the full-size
    /// sample release (`fixtures/sample-full-1.1.0.dgb`) the browser demo writes.
    pub const HUB_APP_BYTES: usize = 128 * 1024;
    pub const SEG_APP_BYTES: usize = 48 * 1024;
    pub const WATCH_APP_BYTES: usize = 48 * 1024;
}

/// The sample, every node confirmed on 1.0.0: hub (RP2040, USB), seg
/// (F401 at [`NODE_I2C_ADDR`] on [`NODE_I2C_PORT`], NRST/BOOT0 wired) and
/// watch (ESP32-S3 behind a clean [`SimLink`] on [`WATCH_UART_PORT`],
/// factory slot also holding 1.0.0). Keys: [`sample::PUBKEY`].
pub fn sample_hub(seed: u64) -> SimRelay {
    let pk = sample::PUBKEY;
    let mut h = demo_hub(pk, seed);
    h.node
        .store()
        .provision_core(sample::HUB_CORE_1_0_0)
        .expect("hub core fixture fits");
    h.node
        .provision(sample::HUB_1_0_0, 0)
        .expect("hub fixture fits");
    let seg = h.i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR).expect("seg");
    seg.store()
        .provision_core(sample::SEG_CORE_1_0_0)
        .expect("seg core fixture fits");
    seg.provision(sample::SEG_1_0_0, 0)
        .expect("seg fixture fits");
    let mut watch = SimNode::new(
        watch_config(),
        pk,
        SimFlash::esp32s3_4m(seed ^ 2),
        Layout::ESP32S3_WATCH,
    );
    watch
        .store()
        .provision_factory(sample::WATCH_1_0_0)
        .expect("factory fits");
    watch
        .provision(sample::WATCH_1_0_0, 0)
        .expect("watch fixture fits");
    h.set_uart(WATCH_UART_PORT, SimLink::new(seed ^ 3), watch);
    h
}

/// Runs a future that never waits (every simulator port completes at once).
pub fn block_on<F: core::future::Future>(f: F) -> F::Output {
    let mut f = core::pin::pin!(f);
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    loop {
        if let core::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}
