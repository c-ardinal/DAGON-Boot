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

pub use flash::{CutPoint, FlashOp, OpKind, SimFlash};
pub use hub::{FrameCutter, Screen, SimBus, SimHub};
pub use link::SimLink;
pub use node::{Run, SimNode, TrialApp};
pub use rng::SimRng;
pub use rom::SimRom;
pub use store::{Layout, SimStore};

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
        target_tag: target_tag("hub-rp2040"),
        role: role::HUB,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
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
