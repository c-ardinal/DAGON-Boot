//! Starting DAGON on an STM32F401RE node: the board definition
//! ([`BoardDef`]), the slot store, taking over the watchdog, and the
//! serving loop of the DAGON link (an I2C target with the mailbox). A node
//! calls [`start`] with its [`App`] (self-check, extensions, who confirms)
//! and gets its published events back. The agent is the start's and its
//! serving task's only: no public function hands it out.
//!
//! The flash has one driver: the slot store holds one [`SharedFlash`]
//! handle on it, the serving loop another (a conformance build runs its
//! flash tests through it); both are used by the serving task only. No
//! flash operation runs with interrupts masked (the execution model of
//! docs/design/lifecycle-events.md): an erase or program stalls instruction
//! fetch for at most `ERASE_UNIT_MAX_MS` (the definition's 128 KiB sector
//! erase maximum; `ERASE_UNIT_MAX_MS_KNOWN` says whether it is checked
//! against the datasheet), below the confirmed IWDG period; every request starts
//! with a gated feed, and the own I2C address is NACKed while a request
//! runs, so the parent retries instead of meeting a stretched clock.
//!
//! Flash size is check-only: the layout is fixed (`layout`, the topology's
//! `f401-default` flash map) and needs [`MIN_FLASH_SIZE`] bytes.

use dagon_agent::{Agent, Config, SlotStore};
pub use dagon_core::log::LogError;
use dagon_events::{BoardBringUp, Published, Watchdog as Periods};
use dagon_format::header::{port_kind, target_tag, ImageHeader, HEADER_SIZE};
use dagon_format::Version;
pub use dagon_mcu_common::hooks::{always, App, AppEvents, Confirm, NodeChain, SelfCheck};
use dagon_mcu_common::hooks::{finish_start, node_chain, BoardExtension, Events, UserExtensions};
use dagon_mcu_common::link::LinkError;
use dagon_mcu_common::serve::{serve as dagon_serve, Exit, ServeHost};
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_stm32::i2c::{I2c, MultiMaster};
use embassy_stm32::peripherals::{FLASH, IWDG};
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::{mode::Async, pac, Peri, Peripherals};
use embassy_time::{Duration, Instant, Timer};
use static_cell::StaticCell;

use crate::flash::F401Flash;
use crate::layout::{self, MIN_FLASH_SIZE};
use crate::links;
use crate::store::KindSStore;
use crate::watchdog::{Guard, Iwdg, Watch};

/// The DAGON link: an I2C target (listening at the node's address).
pub type I2cTarget = I2c<'static, Async, MultiMaster>;
pub type Store = KindSStore<SharedFlash>;
/// The agent of a node on this MCU; its chain holds the app's extensions
/// and CONFIRM check.
pub type AppAgent = Agent<Store, NodeChain>;

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

/// Slot this build is linked for (`slot-b`).
pub const ACTIVE: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };

/// Requests taking longer are timed on RTT (sizes the hub's wait windows).
const SLOW_REQUEST_MS: u64 = 20;
/// Feeding interval of [`start`]'s feeder.
pub const FEED_EVERY: Duration = Duration::from_millis(500);

/// What DAGON takes from the chip on every STM32F401RE node; `L` is the
/// board's DAGON link before it is opened ([`BoardDef::open_link`]).
pub struct DagonParts<L> {
    pub flash: Peri<'static, FLASH>,
    pub iwdg: Peri<'static, IWDG>,
    pub link: L,
}

/// A board with an STM32F401RE: its identity, flash size and how the
/// chip's peripherals split between DAGON and the app. A board package (or
/// a generated `wiring.rs` for a board without one) implements it.
pub trait BoardDef {
    /// The topology's board id.
    const BOARD_ID: &'static str;
    /// Flash size in bytes (check-only, see the module docs).
    const FLASH_SIZE: usize;
    /// The DAGON link's peripherals and pins.
    type Link;
    /// Everything the app gets (pins and peripherals DAGON does not use).
    type UserIo;
    /// Splits the chip's peripherals.
    fn split(p: Peripherals) -> (DagonParts<Self::Link>, Self::UserIo);
    /// Opens the DAGON link: the I2C target at the node's address, and its
    /// registers (the address is NACKed while a request runs).
    fn open_link(link: Self::Link) -> (I2cTarget, pac::i2c::I2c);
    /// The board's `BoardBringUp` port, which [`start`] runs before the
    /// link serves (`app.bring_up`). Default: none, so no bring-up runs and
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

/// A handle on the one flash driver (see the module docs).
pub type SharedFlash = dagon_mcu_common::shared::SharedFlash<F401Flash<'static>>;

// One slot is one erase unit: a store erase is one flash operation, whose
// worst case the watchdog period covers (checked on the definition).
const _: () = assert!(layout::SLOTS.slot_len <= crate::ERASE_UNIT);

/// Opens the slot store over the state log; returns it with the serving
/// loop's handle on the flash. Call once.
pub(crate) fn open_store(flash: Peri<'static, FLASH>) -> Result<(Store, SharedFlash), LogError> {
    use dagon_mcu_common::shared::{driver, FlashDriver};
    static FLASH_DRIVER: StaticCell<FlashDriver<F401Flash<'static>>> = StaticCell::new();
    let driver = FLASH_DRIVER.init(driver(F401Flash::new(flash)));
    let flash = SharedFlash::new(driver, &layout::FLASH_REGIONS);
    let store = KindSStore::load(flash, &layout::LOG, layout::SLOTS)?;
    Ok((store, flash))
}

/// The running image's own header.
pub fn own_header(store: &mut Store) -> Option<[u8; HEADER_SIZE]> {
    let mut h = [0u8; HEADER_SIZE];
    store.read_header(ACTIVE, &mut h).ok().map(|_| h)
}

/// The running image's version (default if its header is unreadable).
pub fn own_version(store: &mut Store) -> Version {
    own_header(store)
        .and_then(|h| ImageHeader::parse(&h).ok())
        .map(|h| h.version)
        .unwrap_or_default()
}

/// Whether the running image is on trial.
pub fn on_trial(store: &Store) -> bool {
    let s = store.log_state();
    s.on_trial() && s.active() == ACTIVE
}

/// The IWDG period the app keeps: the boot core's for this boot (trial or
/// confirmed), or the conformance build's.
pub fn watchdog_period_ms(on_trial: bool) -> u32 {
    if cfg!(feature = "conform") {
        conform::WDT_MS
    } else if on_trial {
        layout::TRIAL_WDT_MS
    } else {
        layout::CONFIRMED_WDT_MS
    }
}

/// Takes over the IWDG with `period_ms` and hands it to `guard`, which
/// learns whether the image runs on trial.
pub fn install_watchdog<const N: usize>(
    iwdg: Peri<'static, IWDG>,
    guard: &Guard<N>,
    period_ms: u32,
    store: &Store,
) {
    let mut wdt = IndependentWatchdog::new(iwdg, period_ms * 1000);
    wdt.unleash();
    guard.install(Iwdg(wdt), on_trial(store));
}

/// The agent configuration of a node on this MCU reached over I2C. `tag`
/// is the node's image target tag.
pub fn config(tag: &str) -> Config {
    Config {
        pubkey: crate::PUBKEY,
        mcu_id: layout::MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag(tag),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: crate::TRIAL_ATTEMPTS,
    }
}

/// The agent over `store`, with the app layer's chain: the published
/// events, the board's extension `board`, the app's extensions `exts` and
/// `self_check` as the CONFIRM decision; returns it with the app's read
/// access to the events. Only [`start`] holds the agent. Call
/// once, after [`install_watchdog`] gave `guard` the watchdog
/// (`app.watchdog_taken` names its period and the guard's trial policy).
fn agent<const N: usize>(
    tag: &str,
    store: Store,
    board: Option<BoardExtension>,
    exts: Option<UserExtensions>,
    self_check: fn() -> bool,
    guard: &Guard<N>,
) -> (AppAgent, AppEvents) {
    static EVENTS: StaticCell<Events> = StaticCell::new();
    let periods = Periods::new(layout::TRIAL_WDT_MS, layout::CONFIRMED_WDT_MS);
    let now_us: fn() -> u64 = || Instant::now().as_micros();
    let events = EVENTS.init(Published::new());
    let (chain, reader) = node_chain(events, board, exts, self_check, now_us, periods);
    let on_trial = on_trial(&store);
    let mut agent = Agent::new(config(tag), store, chain);
    agent.watchdog_taken(watchdog_period_ms(on_trial), guard.trial_policy());
    (agent, reader)
}

/// Starts DAGON: opens the store, takes over the watchdog, opens the link,
/// spawns the serving task (guard task [`DAGON_TASK`]; declare it
/// `Preempting`) on DAGON's high-priority executor (`crate::exec`) and the
/// feeder on `spawner` (the app's executor). Before the link serves, the
/// board's bring-up runs and, with [`Confirm::Own`], the app's trial is
/// confirmed ([`finish_start`]). `tag` is the node's image target tag;
/// `app` holds the self-check, the app's extensions and who confirms.
/// Returns the app's read access to the node's events. Fails if the state
/// log is unreadable (nothing is started then; the watchdog resets the
/// chip).
pub fn start<B: BoardDef, const N: usize>(
    spawner: Spawner,
    parts: DagonParts<B::Link>,
    guard: &'static Guard<N>,
    tag: &str,
    app: App,
    uplink: links::Kind,
) -> Result<AppEvents, LogError> {
    // The only uplink this MCU offers (`require_uplink` checks it at
    // compile time): the board's I2C target.
    assert!(uplink == links::Kind::I2c, "uplink: only i2c on this MCU");
    let task = DAGON_TASK;
    const {
        assert!(
            B::FLASH_SIZE >= MIN_FLASH_SIZE,
            "the DAGON F401 layout needs 512 KiB of flash"
        )
    };
    let (store, flash) = open_store(parts.flash)?;
    let period = watchdog_period_ms(on_trial(&store));
    install_watchdog(parts.iwdg, guard, period, &store);
    let (mut agent, events) = agent(tag, store, B::extension(), app.exts, app.self_check, guard);
    let board = B::bring_up_port().map(|p| (B::BOARD_ID, p));
    guard.set_trial(finish_start(&mut agent, board, app.confirm));
    let (target, regs) = B::open_link(parts.link);
    let link = links::I2c::new(target, regs);
    let high = crate::exec::start();
    high.spawn(serve(agent, flash, link, guard, task).expect("task pool"));
    spawner.spawn(feed(guard).expect("task pool"));
    Ok(events)
}

#[embassy_executor::task]
async fn feed(watch: &'static dyn Watch) -> ! {
    loop {
        watch.feed();
        Timer::after(FEED_EVERY).await;
    }
}

/// The serving loop over the I2C target link (the generic loop of
/// `dagon_mcu_common::serve`). Requests are handled synchronously with the
/// own address NACKed, so a hub poll during an erase or write never meets
/// a stretched clock (instruction fetch from flash stalls while it is
/// busy). Run it on an executor that preempts the app's (guard task
/// `task` at preempting level) when the app has long-running work. A
/// reboot waits the delay `allow_reboot_now` asked for, idle under the
/// guard, then resets.
#[embassy_executor::task]
async fn serve(
    mut agent: AppAgent,
    flash: SharedFlash,
    mut link: links::I2c,
    watch: &'static dyn Watch,
    task: usize,
) -> ! {
    let mut host = Host {
        watch,
        task,
        flash,
        started: Instant::now(),
        out: [0; conform::OUT_MAX],
    };
    loop {
        if dagon_serve(&mut link, &mut agent, &mut host).await == Exit::Reboot {
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}

/// The serving loop's view of this node.
struct Host {
    watch: &'static dyn Watch,
    task: usize,
    flash: SharedFlash,
    started: Instant,
    out: [u8; conform::OUT_MAX],
}

impl ServeHost<Store, NodeChain> for Host {
    fn now_ms(&self) -> u64 {
        Instant::now().as_millis()
    }
    fn idle(&mut self) {
        self.watch.idle(self.task);
    }
    fn busy(&mut self) {
        self.watch.busy(self.task);
    }
    fn feed(&mut self) {
        self.watch.feed();
    }
    fn on_request(&mut self) {
        self.started = Instant::now();
    }
    fn intercept(&mut self, _: &mut AppAgent, request: &[u8]) -> Option<&[u8]> {
        let n = conform::serve(
            &mut self.flash,
            request,
            &mut self.out,
            self.watch,
            self.task,
        )?;
        Some(&self.out[..n])
    }
    fn after(&mut self, agent: &mut AppAgent, cmd: u8) {
        self.watch.set_trial(agent.images().on_trial());
        let ms = self.started.elapsed().as_millis();
        if ms >= SLOW_REQUEST_MS {
            info!("cmd {=u8:#04x}: {} ms", cmd, ms);
        }
    }
    fn on_error(&mut self, e: LinkError) {
        warn!("i2c: {}", defmt::Debug2Format(&e));
    }
    async fn wait_ms(&mut self, ms: u32) {
        Timer::after_millis(ms.into()).await;
    }
}

/// Conformance build (`conform`): CONF_RUN runs dagon-conform's node-side
/// tests in front of the agent.
#[cfg(feature = "conform")]
mod conform {
    use super::*;
    use dagon_conform::node::{handle_frame, NodeSetup, RESPONSE_MAX};

    /// IWDG timeout for this build. The HAL assumes a 32 kHz LSI; the F401's
    /// LSI may run at up to 47 kHz, which shortens this to about 8.2 s, still
    /// above the 4 s a 128 KiB sector erase may take between two feeds.
    pub const WDT_MS: u32 = 12_000;
    pub const OUT_MAX: usize = RESPONSE_MAX;

    /// Answers a CONF_RUN request into `out`; `None` for any other frame.
    pub fn serve(
        flash: &mut SharedFlash,
        request: &[u8],
        out: &mut [u8; OUT_MAX],
        watch: &dyn Watch,
        task: usize,
    ) -> Option<usize> {
        let setup = NodeSetup {
            declared: &layout::FLASH_REGIONS,
            scratch: crate::conform_scratch(),
        };
        let max = max_payload::I2C;
        // Long tests feed through this hook and count as progress.
        let mut feed = || watch.progress(task);
        handle_frame(request, max, flash, &setup, out, &mut feed)
    }
}

#[cfg(not(feature = "conform"))]
mod conform {
    use super::*;

    pub const WDT_MS: u32 = 0;
    pub const OUT_MAX: usize = 0;

    pub fn serve(
        _: &mut SharedFlash,
        _: &[u8],
        _: &mut [u8; OUT_MAX],
        _: &dyn Watch,
        _: usize,
    ) -> Option<usize> {
        None
    }
}
