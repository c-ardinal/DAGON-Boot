//! The 親機 (hub) relay: the PC link over USB CDC, relaying to sub nodes
//! over I2C0 (DAGON mailbox) and a PIO UART, SUBCTL / I2C_RAW for the ROM
//! recovery path of an STM32 sub node, HELLO polling of the sub nodes, and
//! the 親機's own agent (its updates). A display, if the board has one,
//! reads [`state`].
//!
//! Tasks: the PC task and one task per present link run on DAGON's
//! high-priority executor ([`crate::exec`]), as guard tasks [`task::PC`],
//! [`task::I2C`], [`task::UART`] (declare them `Preempting`); the app's own
//! tasks take guard tasks from [`TASKS`] on, in thread mode
//! (`Background`). [`start`] also spawns the watchdog feeder on the app's
//! executor.

pub use crate::nodes;
mod pc;
mod ports;
pub mod state;

pub use nodes::{named, SubNode};

use dagon_proto::payload::role;
use embassy_executor::Spawner;
use embassy_rp::peripherals::{I2C0, PIO0};

use crate::links::{I2cLink, PioStream, RpI2c};
use crate::node::{self, App, AppEvents, BoardDef, DagonParts};
use crate::sub::SubLines;
use crate::uart::UartLink;
use crate::watchdog::{feed_forever, now_ms, Guard, Level, Watch};

/// The relay's guard tasks.
pub mod task {
    pub const PC: usize = 0;
    pub const I2C: usize = 1;
    pub const UART: usize = 2;
}

/// Guard tasks the relay uses; the app's tasks are numbered from here.
pub const TASKS: usize = 3;

/// Liveness levels for a guard with the relay and `N - TASKS` app tasks in
/// thread mode.
pub const fn levels<const N: usize>() -> [Level; N] {
    crate::watchdog::levels::<N>(TASKS)
}

/// The links the board wires to sub nodes (any may be absent; requests for
/// an absent one answer E_UNSUPPORTED).
pub struct Links {
    pub i2c: Option<I2cLink<RpI2c<'static, I2C0>>>,
    pub uart: Option<PioStream<'static, PIO0, 0, 1>>,
    /// NRST / BOOT0 of an STM32 sub node (SUBCTL, I2C_RAW).
    pub sub: Option<SubLines<'static>>,
}

/// Who the 親機 is.
pub struct RelayConfig {
    /// Image target tag of this 親機 (as in the topology).
    pub tag: &'static str,
    /// USB product name shown on the host.
    pub name: &'static str,
    /// Sub nodes to poll while no topology display copy is stored.
    pub fallback: &'static [SubNode],
    /// The 親機 app's self-check, extensions and who confirms.
    pub app: App,
}

/// Starts the relay: opens the store and the agent (taking over the
/// watchdog, running the board's bring-up, see `node::open`), spawns the
/// PC and link tasks on DAGON's high-priority executor and the watchdog
/// feeder on `spawner` (the app's executor). Returns the 親機's events.
pub fn start<B: BoardDef, const N: usize>(
    spawner: Spawner,
    parts: DagonParts,
    links: Links,
    guard: &'static Guard<N>,
    cfg: RelayConfig,
) -> AppEvents {
    const { assert!(N >= TASKS, "the relay needs guard tasks 0..3") };
    let node::Opened {
        agent,
        serial,
        flash,
        events,
    } = node::open::<B, N>(
        parts.flash,
        parts.watchdog,
        guard,
        task::PC,
        cfg.tag,
        role::HUB,
        cfg.app,
    );
    let watch: &'static dyn Watch = guard;
    let pc = pc::Pc {
        agent,
        flash,
        sub: links.sub,
        has_i2c: links.i2c.is_some(),
        has_uart: links.uart.is_some(),
        fallback: cfg.fallback,
        watch,
        resp: [0; dagon_proto::MAX_FRAME],
    };
    let high = crate::exec::start();
    high.spawn(pc::task(parts.usb, cfg.name, serial, pc).expect("task pool"));
    if let Some(i2c) = links.i2c {
        high.spawn(ports::i2c_task(i2c, watch).expect("task pool"));
    }
    if let Some(stream) = links.uart {
        let uart = UartLink::new(stream, now_ms as fn() -> u64);
        high.spawn(ports::uart_task(uart, watch).expect("task pool"));
    }
    spawner.spawn(feed(watch).expect("task pool"));
    events
}

#[embassy_executor::task]
async fn feed(watch: &'static dyn Watch) -> ! {
    feed_forever(watch).await
}
