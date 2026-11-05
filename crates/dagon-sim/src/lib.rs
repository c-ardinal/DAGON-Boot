//! DAGON simulator: deterministic stand-ins for the hardware so the host
//! tools, the browser UI and tests have a partner without boards.
//!
//! - [`SimFlash`]: geometry, operation log, power-cut points.
//! - [`SimLink`]: frame loss, bit flips, delay, silent peer.
//! - [`SimRom`]: the STM32 I2C ROM loader (ACK/NACK/BUSY, 1000 ms frame
//!   timeout, at most 256 bytes per read or write).
//! - [`SimNode`]: dagon-agent over a [`SimStore`], with a boot decision at
//!   reset, an I2C mailbox and selectable trial-app behaviour.
//! - [`SimHub`]: answers the PC (directly or as a USB byte stream), relays to
//!   nodes on I2C and UART, drives NRST/BOOT0 and raw ROM transactions.
//! - [`TimedHub`]: a [`SimHub`] whose answers take the links' and flash's
//!   real time (one model for `dagon demo` and the browser demo).
//!
//! Randomness comes only from [`SimRng`] seeds, so a seed replays a run.
#![forbid(unsafe_code)]

pub mod flash;
pub mod hub;
pub mod link;
pub mod node;
mod ports;
pub mod rng;
pub mod rom;
pub mod store;
pub mod timing;

pub use flash::{CutPoint, FlashOp, OpKind, SimFlash};
pub use hub::{FrameCutter, Screen, SimBus, SimHub};
pub use link::SimLink;
pub use node::{Run, SimNode, TrialApp};
pub use rng::SimRng;
pub use rom::SimRom;
pub use store::{Layout, SimStore};
pub use timing::{Medium, TimedHub};

// The simulator declares no machine of its own: its nodes run the agent's.
dagon_events::state_machine! {}

use dagon_agent::Config;
use dagon_format::header::{mcu_id, port_kind, target_tag};
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};

/// Default I2C address of the first node.
pub const NODE_I2C_ADDR: u8 = 0x42;

/// Hub on RP2040, answering on USB.
pub fn hub_config(pubkey: [u8; 32]) -> Config {
    Config {
        pubkey,
        mcu_id: mcu_id::RP2040,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag("hub-rp2040-lcd"),
        role: role::HUB,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        trial_attempts: store::trial_attempts("rp2040"),
    }
}

/// STM32F401 node on I2C.
pub fn f401_config(pubkey: [u8; 32]) -> Config {
    Config {
        pubkey,
        mcu_id: mcu_id::STM32F401XE,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag("seg-f401-7seg"),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: store::trial_attempts("stm32f401re"),
    }
}

/// Hub with an empty flash, plus an empty F401 at [`NODE_I2C_ADDR`] wired to
/// NRST/BOOT0. Provision images with [`SimNode::provision`]; a node with
/// nothing to run stays silent (NACK).
pub fn demo_hub(pubkey: [u8; 32], seed: u64) -> SimHub {
    let hub = SimNode::new(
        hub_config(pubkey),
        SimFlash::rp2040_2m(seed),
        Layout::RP2040_HUB,
    );
    let mut f401 = SimNode::new(
        f401_config(pubkey),
        SimFlash::stm32f401xe(seed ^ 1),
        Layout::STM32F401XE,
    );
    f401.i2c_addr = Some(NODE_I2C_ADDR);
    let mut h = SimHub::new(hub);
    h.add_i2c(f401, true);
    h
}

/// ESP32-S3 watch on the PIO UART link (kind V header fields; the slot
/// handling is simulated like kind S).
pub fn watch_config(pubkey: [u8; 32]) -> Config {
    Config {
        pubkey,
        mcu_id: mcu_id::ESP32S3,
        port_kind: port_kind::VENDOR_STAGE2,
        target_tag: target_tag("watch-s3"),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::UART as u16,
        trial_attempts: store::trial_attempts("esp32s3"),
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
}

/// The sample, every node confirmed on 1.0.0: hub (RP2040, USB), seg
/// (F401 on I2C at [`NODE_I2C_ADDR`], NRST/BOOT0 wired) and watch
/// (ESP32-S3 behind a clean [`SimLink`] on the PIO UART, factory slot also
/// holding 1.0.0). Keys: [`sample::PUBKEY`].
pub fn sample_hub(seed: u64) -> SimHub {
    let pk = sample::PUBKEY;
    let mut h = demo_hub(pk, seed);
    h.node
        .provision(sample::HUB_1_0_0, 0)
        .expect("hub fixture fits");
    h.i2c_node(NODE_I2C_ADDR)
        .expect("seg")
        .provision(sample::SEG_1_0_0, 0)
        .expect("seg fixture fits");
    let mut watch = SimNode::new(
        watch_config(pk),
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
    h.set_uart(SimLink::new(seed ^ 3), watch);
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
