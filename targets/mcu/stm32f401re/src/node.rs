//! Starting DAGON on an STM32F401RE node: the board definition
//! ([`BoardDef`]), the slot store, taking over the watchdog, and the
//! serving loop of the DAGON link (an I2C target with the mailbox). A node
//! that needs nothing special calls [`start`]; the pieces are public for an
//! app that lays out its own executors.
//!
//! Flash size is check-only: the layout is fixed (`layout`, the topology's
//! `f401-default` flash map) and needs [`MIN_FLASH_SIZE`] bytes.

use dagon_agent::{Agent, Config, DefaultHooks, Hooks, SlotStore, IDLE_TIMEOUT_MS};
use dagon_core::log::LogError;
use dagon_format::header::{port_kind, target_tag, ImageHeader, HEADER_SIZE};
use dagon_format::Version;
use dagon_proto::payload::{mode, role};
use dagon_proto::{frame_len, max_payload};
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_stm32::i2c::{I2c, MultiMaster, SlaveCommandKind};
use embassy_stm32::peripherals::{FLASH, IWDG};
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::{mode::Async, pac, Peri, Peripherals};
use embassy_time::{Duration, Instant, Timer};

use crate::flash::F401Flash;
use crate::layout::{self, MIN_FLASH_SIZE};
use crate::mailbox::Mailbox;
use crate::store::KindSStore;
use crate::watchdog::{Guard, Iwdg, Watch};

/// The DAGON link: an I2C target (listening at the node's address).
pub type I2cTarget = I2c<'static, Async, MultiMaster>;
pub type Store = KindSStore<F401Flash<'static>>;
pub type AppAgent = Agent<Store, DefaultHooks>;

/// Slot this build is linked for (`slot-b`).
pub const ACTIVE: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };

/// How often the serving task wakes to drop an abandoned transfer.
const TICK: Duration = Duration::from_millis(500);
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
}

/// Opens the slot store over the state log.
pub fn open_store(flash: Peri<'static, FLASH>) -> Result<Store, LogError> {
    KindSStore::load(F401Flash::new(flash), &layout::LOG, layout::SLOTS)
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
    }
}

/// Starts DAGON on the executor of `spawner`: opens the store, takes over
/// the watchdog, opens the link and spawns the serving task (guard task
/// `task`) and a feeder. Fails if the state log is unreadable (nothing is
/// started then; the watchdog resets the chip).
pub fn start<B: BoardDef, const N: usize>(
    spawner: Spawner,
    parts: DagonParts<B::Link>,
    guard: &'static Guard<N>,
    task: usize,
    tag: &str,
) -> Result<(), LogError> {
    const { assert!(B::FLASH_SIZE >= MIN_FLASH_SIZE, "the DAGON F401 layout needs 512 KiB of flash") };
    let store = open_store(parts.flash)?;
    let period = watchdog_period_ms(on_trial(&store));
    install_watchdog(parts.iwdg, guard, period, &store);
    let (target, regs) = B::open_link(parts.link);
    let agent = Agent::new(config(tag), store, DefaultHooks);
    spawner.spawn(serve(agent, target, regs, guard, task).expect("task pool"));
    spawner.spawn(feed(guard).expect("task pool"));
    Ok(())
}

#[embassy_executor::task]
async fn feed(watch: &'static dyn Watch) -> ! {
    loop {
        watch.feed();
        Timer::after(FEED_EVERY).await;
    }
}

/// The mailbox loop. Requests are handled synchronously with the own
/// address NACKed, so a hub poll during an erase or write never meets a
/// stretched clock (instruction fetch from flash stalls while it is busy).
/// Run it on an executor that preempts the app's (guard task `task` at
/// preempting level) when the app has long-running work.
#[embassy_executor::task]
pub async fn serve(
    mut agent: AppAgent,
    mut target: I2cTarget,
    regs: pac::i2c::I2c,
    watch: &'static dyn Watch,
    task: usize,
) -> ! {
    let mut mailbox = Mailbox::default();
    let mut rx = [0u8; frame_len(max_payload::I2C)];
    let mut last_request: Option<Instant> = None;
    // ACK (true) or NACK (false) the own address and data.
    let acknowledge = |on: bool| regs.cr1().modify(|w| w.set_ack(on));
    loop {
        watch.idle(task);
        let event = select(target.listen(), Timer::after(TICK)).await;
        watch.busy(task);
        let cmd = match event {
            Either::First(Ok(cmd)) => cmd,
            Either::First(Err(e)) => {
                warn!("i2c: {}", e);
                continue;
            }
            Either::Second(()) => {
                let idle_ms = IDLE_TIMEOUT_MS as u64;
                if last_request.is_some_and(|t| t.elapsed().as_millis() >= idle_ms) {
                    agent.abort();
                    last_request = None;
                }
                continue;
            }
        };
        match cmd.kind {
            SlaveCommandKind::Write => {
                let n = match target.respond_to_write(&mut rx).await {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("i2c write: {}", e);
                        continue;
                    }
                };
                // Zero bytes: an address probe.
                if n == 0 {
                    continue;
                }
                last_request = Some(Instant::now());
                mailbox.request();
                // An erase starts with a full watchdog period.
                watch.feed();
                acknowledge(false);
                let t0 = Instant::now();
                if !conform::serve(&mut agent, &rx[..n], &mut mailbox, watch, task) {
                    let reply = agent.handle(&rx[..n]);
                    let reboot = reply.as_ref().is_some_and(|r| r.reboot);
                    mailbox.respond(reply.map(|r| r.frame), reboot);
                }
                acknowledge(true);
                watch.set_trial(on_trial(agent.store()));
                let ms = t0.elapsed().as_millis();
                if ms >= SLOW_REQUEST_MS {
                    // Byte 5 of a frame is the command.
                    info!("cmd {=u8:#04x}: {} ms", rx[5], ms);
                }
            }
            SlaveCommandKind::Read => {
                let sent = match target.respond_to_read(mailbox.read_data()).await {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("i2c read: {}", e);
                        continue;
                    }
                };
                if mailbox.read_done(sent) {
                    agent.hooks().before_reboot();
                    cortex_m::peripheral::SCB::sys_reset();
                }
            }
        }
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

    /// Answers a CONF_RUN request; false for any other frame.
    pub fn serve(
        agent: &mut AppAgent,
        request: &[u8],
        mailbox: &mut Mailbox,
        watch: &dyn Watch,
        task: usize,
    ) -> bool {
        let state = agent.store().log_state();
        let setup = NodeSetup {
            declared: &layout::FLASH_REGIONS,
            scratch: crate::scratch_addr(&state, ACTIVE, &layout::SLOTS),
        };
        let mut out = [0u8; RESPONSE_MAX];
        let flash = agent.store().flash();
        let max = max_payload::I2C;
        // Long tests feed through this hook and count as progress.
        let mut feed = || watch.progress(task);
        match handle_frame(request, max, flash, &setup, &mut out, &mut feed) {
            Some(n) => {
                mailbox.respond(Some(&out[..n]), false);
                true
            }
            None => false,
        }
    }
}

#[cfg(not(feature = "conform"))]
mod conform {
    use super::*;

    pub const WDT_MS: u32 = 0;

    pub fn serve(_: &mut AppAgent, _: &[u8], _: &mut Mailbox, _: &dyn Watch, _: usize) -> bool {
        false
    }
}
