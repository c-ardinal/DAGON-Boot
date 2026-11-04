//! Starting DAGON on an RP2040 node: the board definition ([`BoardDef`]),
//! opening the slot store and agent with the watchdog taken over, the
//! board's bring-up run and, if the app asks, its trial confirmed
//! ([`open`]), and, for a node whose DAGON link is USB, starting it all in
//! one call ([`start`]). The app hands over an [`App`] (self-check,
//! extensions, who confirms) and gets the node's events back.
//!
//! Flash size is check-only: the layout is fixed (`layout`, the topology's
//! `rp2040-4m` flash map) and needs [`MIN_FLASH_SIZE`] bytes; a board with
//! less fails to build, a larger one keeps the rest for the app.

use dagon_agent::{Agent, Config, SlotStore};
use dagon_events::{BoardBringUp, Published, Watchdog as Periods};
use dagon_format::header::{mcu_id, port_kind, target_tag};
pub use dagon_mcu_common::hooks::{always, App, AppEvents, Confirm, NodeChain, SelfCheck};
use dagon_mcu_common::hooks::{finish_start, node_chain, BoardExtension, Events};
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, SlotState, StatusResp};
use embassy_rp::peripherals::{FLASH, USB, WATCHDOG};
use embassy_rp::{Peri, Peripherals};
use static_cell::StaticCell;

use crate::flash::{RpFlash, REGIONS};
use crate::layout::MIN_FLASH_SIZE;
use crate::store::AppStore;
use crate::watchdog::{Guard, Wdt};

/// What DAGON takes from the chip on every RP2040 node.
pub struct DagonParts {
    pub flash: Peri<'static, FLASH>,
    pub watchdog: Peri<'static, WATCHDOG>,
    /// The DAGON link to the host or hub (USB CDC).
    pub usb: Peri<'static, USB>,
}

/// A board with an RP2040: its identity, flash size and how the chip's
/// peripherals split between DAGON and the app. A board package (or a
/// generated `wiring.rs` for a board without one) implements it.
pub trait BoardDef {
    /// The topology's board id.
    const BOARD_ID: &'static str;
    /// Flash size in bytes (check-only, see the module docs).
    const FLASH_SIZE: usize;
    /// GPIO of an LED the diagnostic panic handler may blink.
    const DIAG_LED: Option<u8>;
    /// That LED lights at a high level (else at low).
    const DIAG_LED_ACTIVE_HIGH: bool = true;
    /// Everything the app gets (pins and peripherals DAGON does not use).
    type UserIo;
    /// Splits the chip's peripherals.
    fn split(p: Peripherals) -> (DagonParts, Self::UserIo);
    /// The board's `BoardBringUp` port, which [`open`] runs before the
    /// links serve (`app.bring_up`). Default: none, so no bring-up runs and
    /// no `app.bring_up` is emitted. Called once.
    fn bring_up_port() -> Option<&'static mut dyn BoardBringUp> {
        None
    }
    /// The board package's extension, which the start puts in the node's
    /// chain after the publisher and before the app's: it observes every
    /// event and answers the decision points before the app. Default:
    /// none. Called once.
    fn extension() -> Option<BoardExtension> {
        None
    }
}

/// The agent of an RP2040 node; its chain holds the app's extensions and
/// CONFIRM check.
pub type AppAgent = Agent<AppStore, NodeChain>;

/// A handle on the one flash driver: the slot store holds one, the start
/// hands out another for the package's own reads (the topology copy), both
/// used by the serving task only. Flash operations run from RAM with XIP
/// off and interrupts masked by the driver, one 4 KiB sector (or page) per
/// operation (`ERASE_UNIT`, worst case `ERASE_UNIT_MAX_MS`); DAGON masks
/// nothing around them.
pub type SharedFlash = dagon_mcu_common::shared::SharedFlash<RpFlash>;

/// What [`open`] made.
pub(crate) struct Opened {
    pub(crate) agent: AppAgent,
    /// The flash chip's [`serial`].
    pub(crate) serial: &'static str,
    /// Another handle on the slot store's flash driver.
    pub(crate) flash: SharedFlash,
    /// The app's read access to the node's events.
    pub(crate) events: AppEvents,
}

const _: () = assert!(crate::layout::SECTOR == crate::ERASE_UNIT);

/// The guard task [`start`] gives DAGON's serving task; the app's tasks
/// take the others.
pub const DAGON_TASK: usize = 0;
/// The first guard task of the app's own tasks.
pub const APP_TASKS: usize = DAGON_TASK + 1;

/// Liveness levels for a guard with DAGON's serving task and `N - 1` app
/// tasks in thread mode.
pub const fn levels<const N: usize>() -> [crate::watchdog::Level; N] {
    crate::watchdog::levels::<N>(APP_TASKS)
}

/// The agent configuration of an RP2040 node reached over USB. `role` is
/// its topology role (`dagon_proto::payload::role`).
pub fn config(tag: &str, role: u8) -> Config {
    Config {
        pubkey: crate::PUBKEY,
        mcu_id: mcu_id::RP2040,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag(tag),
        role,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        trial_attempts: crate::TRIAL_ATTEMPTS,
    }
}

/// Whether the running image is on trial (not confirmed yet).
pub fn on_trial(st: &StatusResp) -> bool {
    st.slots.get(st.active as usize).map(|s| s.state) == Some(SlotState::Trying)
}

/// Opens the slot store, takes over the watchdog the boot core started
/// (fed once right away), builds the agent with the app's chain, runs the
/// board's bring-up and, with [`Confirm::Own`], confirms the app's trial
/// ([`finish_start`]); returns the agent with the flash chip's [`serial`],
/// a handle on the flash driver and the node's events. `task` is the guard
/// task that serves the agent; `tag` and `role` are the node's image
/// target tag and topology role; `app` holds the self-check, the app's
/// extensions and who confirms. Only the starts in this package hold the
/// agent. Call once.
pub(crate) fn open<B: BoardDef, const N: usize>(
    flash: Peri<'static, FLASH>,
    watchdog: Peri<'static, WATCHDOG>,
    guard: &'static Guard<N>,
    task: usize,
    tag: &str,
    role: u8,
    app: App,
) -> Opened {
    const {
        assert!(
            B::FLASH_SIZE >= MIN_FLASH_SIZE,
            "the DAGON RP2040 layout needs 4 MiB of flash"
        )
    };
    #[cfg(target_os = "none")]
    if let Some(gpio) = B::DIAG_LED {
        crate::arm::led::select(gpio, B::DIAG_LED_ACTIVE_HIGH);
    }
    use dagon_mcu_common::shared::{driver, FlashDriver};
    static FLASH_DRIVER: StaticCell<FlashDriver<RpFlash>> = StaticCell::new();
    let mut chip = RpFlash::new(flash);
    let serial = serial(&mut chip);
    let flash = SharedFlash::new(FLASH_DRIVER.init(driver(chip)), &REGIONS);
    let mut store = AppStore {
        flash,
        watch: guard,
        task,
    };
    let trial = on_trial(&store.status());
    guard.install(Wdt::new(watchdog), trial);
    guard.feed();
    static EVENTS: StaticCell<Events> = StaticCell::new();
    let periods = Periods::new(crate::layout::WDT_MS, crate::layout::WDT_MS);
    let now_us: fn() -> u64 = || embassy_time::Instant::now().as_micros();
    let events = EVENTS.init(Published::new());
    let (chain, events) = node_chain(
        events,
        B::extension(),
        app.exts,
        app.self_check,
        now_us,
        periods,
    );
    let mut agent = Agent::new(config(tag, role), store, chain);
    agent.watchdog_taken(crate::layout::WDT_MS, guard.trial_policy());
    let board = B::bring_up_port().map(|p| (B::BOARD_ID, p));
    guard.set_trial(finish_start(&mut agent, board, app.confirm));
    Opened {
        agent,
        serial,
        flash,
        events,
    }
}

static SERIAL: StaticCell<[u8; 16]> = StaticCell::new();

/// The flash chip's 64-bit unique ID in hex: a per-board USB serial
/// number. Call once.
fn serial(flash: &mut RpFlash) -> &'static str {
    let s = SERIAL.init(hex(&flash.unique_id()));
    core::str::from_utf8(s).unwrap_or("0")
}

fn hex(id: &[u8; 8]) -> [u8; 16] {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0u8; 16];
    for (i, b) in id.iter().enumerate() {
        out[2 * i] = DIGITS[(b >> 4) as usize];
        out[2 * i + 1] = DIGITS[(b & 0xF) as usize];
    }
    out
}

#[cfg(feature = "usb")]
pub use usb_node::*;

#[cfg(feature = "usb")]
mod usb_node {
    use dagon_proto::payload::role;
    use embassy_executor::Spawner;
    use embassy_rp::peripherals::USB;
    use embassy_rp::Peri;

    use super::*;
    use crate::usb::{self, serve_agent};
    use crate::watchdog::{feed_forever, Watch};

    /// Starts DAGON on a node whose DAGON link is USB: the USB device and
    /// the agent serving task (guard task [`DAGON_TASK`]; declare it
    /// `Preempting`) on DAGON's high-priority executor (`crate::exec`), the
    /// watchdog feeder on `spawner` (the app's executor). `tag` is the
    /// node's image target tag (also the USB product name); `app` holds
    /// the self-check, the app's extensions and who confirms ([`open`]).
    /// Returns the node's events. A 親機 uses `crate::relay::start`
    /// instead.
    pub fn start<B: BoardDef, const N: usize>(
        spawner: Spawner,
        parts: DagonParts,
        guard: &'static Guard<N>,
        tag: &'static str,
        app: App,
    ) -> AppEvents {
        let task = DAGON_TASK;
        let Opened {
            agent,
            serial,
            events,
            ..
        } = open::<B, N>(
            parts.flash,
            parts.watchdog,
            guard,
            task,
            tag,
            role::NODE,
            app,
        );
        let high = crate::exec::start();
        high.spawn(serve(parts.usb, tag, serial, agent, guard, task).expect("task pool"));
        spawner.spawn(feed(guard).expect("task pool"));
        events
    }

    /// Builds the USB device inside the high-priority executor (its types
    /// stay there), runs it and serves the agent.
    #[embassy_executor::task]
    async fn serve(
        usb: Peri<'static, USB>,
        product: &'static str,
        serial: &'static str,
        agent: AppAgent,
        watch: &'static dyn Watch,
        task: usize,
    ) -> ! {
        let (device, link) = usb::new(usb, product, serial);
        // SAFETY: awaited directly inside this embassy task, whose context
        // comes from the interrupt executor running it.
        let spawner = unsafe { Spawner::for_current_executor() }.await;
        spawner.spawn(usb::run(device).expect("task pool"));
        serve_agent(link, agent, watch, task).await
    }

    #[embassy_executor::task]
    async fn feed(watch: &'static dyn Watch) -> ! {
        feed_forever(watch).await
    }
}
