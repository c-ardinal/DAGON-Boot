//! The DAGON side of a running image: [`start`] takes the peripherals DAGON
//! owns and runs the link task on a high-priority interrupt executor.
//!
//! - The link task is the only owner of the link UART (on the board's pins),
//!   USB Serial/JTAG, the flash, the RTC/super watchdogs and whatever the
//!   board's `BoardDef` implementation holds (e.g. an internal I2C bus). It runs
//!   `BoardDef::bring_up` once before serving.
//!   It waits on both links with interrupt-driven async reads (no polling),
//!   serves each request (CONF_RUN in a conformance build, ENTER_ROM, the
//!   agent) and replies on the link the request came in on. Agent access is
//!   serialized here.
//! - It runs at [`LINK_PRIORITY`], above the thread-mode executor where the
//!   app's own tasks run, so the app's work never delays DAGON traffic.
//! - Watchdog: the RTC WDT (4 s from start) is not fed at all while on
//!   trial, so an image that is not confirmed in time is reset and rolled
//!   back. Once confirmed it is fed only while every task registered in the
//!   [`Liveness`] registry is within its deadline; the link task registers
//!   too and announces a budget for every request.
//! - UART and USB receivers drop a partial frame after 20 ms without a byte
//!   (inclusive). Bytes of one read share its completion time; after a
//!   synchronous section of 20 ms or more any partial frame is dropped, since
//!   the FIFO may have collected bytes with gaps the read cannot see. Residual
//!   limit: a whole frame that arrived entirely during such a section is
//!   accepted even if its own bytes had gaps.
//! - Liveness: the link lease holds the CPU only around synchronous work
//!   (decide the reply, flash, CONFIRM check), not across awaits.
//! - ENTER_ROM: the ROM switch happens only after the OK has left the node;
//!   otherwise the node stays in the app ([`ROM_REPLY_LOST`] counts it).

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::{
    board::{BoardDef, PlainBoard},
    boot::EspBootSelect,
    flash::EspFlash,
    framer::{FrameCutter, INTER_BYTE_TIMEOUT_MS},
    otadata::ENTRY_LEN,
    sys::{EspClock, EspReset, RtcWatchdog},
    TRIAL_WDT_MS,
};
use dagon_agent::{Agent, Hooks, SlotStore, StoreError, IDLE_TIMEOUT_MS};
use dagon_port::{BootSelectV, Reset, Watchdog};
use dagon_proto::payload::SlotState;
use embassy_executor::{SendSpawner, Spawner};
use embassy_futures::select::{select3, Either3};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_io_async::{Read, Write};
use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::{
    interrupt::Priority,
    peripherals::{FLASH, FROM_CPU_INTR1, LPWR, RTC_TIMER, USB_DEVICE},
    rtc_cntl::{Rtc, Swd},
    uart::Uart,
    usb::usb_serial_jtag::UsbSerialJtag,
    Async, Blocking,
};
use esp_rtos::embassy::InterruptExecutor;
use static_cell::StaticCell;

use crate::liveness::{Lease, Liveness};
use crate::rom::{self, Method, Step};
use crate::serve::{self, Next, Then};
use crate::store::{EspStore, Otadata};
use crate::trial::TrialWatchdog;
use crate::Version;

/// Priority of the link executor (the app's tasks run in thread mode, below).
pub const LINK_PRIORITY: Priority = Priority::Priority2;
/// The link task wakes at least this often to feed the watchdog.
const FEED_PERIOD: Duration = Duration::from_millis(500);
/// Time one request may take, a full slot erase included.
const REQUEST_BUDGET_MS: u32 = 120_000;
/// Give up on a reply after this long (USB: host stopped reading; the largest
/// UART frame takes about 90 ms).
const SEND_TIMEOUT: Duration = Duration::from_millis(500);

/// Everything DAGON owns on the chip, handed over by the board crate.
pub struct NodePeripherals<'d> {
    pub rtc_timer: RTC_TIMER<'d>,
    /// The link UART on the board's pins, configured with
    /// `link::uart_config()` (115200 8N1).
    pub uart: Uart<'d, Blocking>,
    pub usb_device: USB_DEVICE<'d>,
    pub flash: FLASH<'d>,
    /// The board's default node tag (`DAGON_TARGET_TAG` overrides it).
    pub target_tag: &'static str,
    /// Board extras (bring-up, optional PMIC ROM entry).
    pub board: &'d mut dyn BoardDef,
}

/// How the node identifies itself and decides CONFIRM.
#[derive(Clone, Copy)]
pub struct NodeConfig {
    /// HELLO mode: `payload::mode::APP` (ota_0/ota_1) or `AGENT` (factory).
    pub mode: u8,
    /// Self-check on CONFIRM while on trial; `false` keeps the trial.
    pub self_check: fn() -> bool,
}

impl NodeConfig {
    /// App image (ota_0/ota_1): HELLO mode app.
    pub const fn app(self_check: fn() -> bool) -> Self {
        Self {
            mode: dagon_proto::payload::mode::APP,
            self_check,
        }
    }

    /// Factory recovery agent: HELLO mode agent, confirms without a check.
    pub const fn factory() -> Self {
        Self {
            mode: dagon_proto::payload::mode::AGENT,
            self_check: || true,
        }
    }
}

/// Proof that the RTC WDT runs; [`start`] requires it.
pub struct WatchdogArmed(());

/// Starts the RTC WDT (4 s) right away. Call it first, right after
/// `esp_hal::init` (which stops every watchdog), so an image that stalls
/// before [`start`] still resets and, on trial, rolls back. This is the only
/// start: the link task never restarts it and feeds it only once it knows
/// the image is confirmed.
pub fn arm_watchdog(rtc_timer: RTC_TIMER<'_>) -> WatchdogArmed {
    RtcWatchdog::new(Rtc::new(rtc_timer).rwdt).start(TRIAL_WDT_MS);
    WatchdogArmed(())
}

/// Version of the running image from the startup STATUS (its header in the
/// dagon partition); `None` before the link task has read it, or when the
/// running slot has no header.
pub fn running_version() -> Option<Version> {
    VERSION_KNOWN
        .load(Ordering::Acquire)
        .then(|| Version::from_raw(RUNNING_VERSION.load(Ordering::Acquire)))
}

/// The board reported its display released (`BoardDef::bring_up`); the app's display
/// task may start talking to the panel (after its own settling time).
pub fn display_ready() -> bool {
    DISPLAY_READY.load(Ordering::Acquire)
}

static RUNNING_VERSION: AtomicU32 = AtomicU32::new(0);
static VERSION_KNOWN: AtomicBool = AtomicBool::new(false);
static DISPLAY_READY: AtomicBool = AtomicBool::new(false);

fn publish_startup(store: &mut Store, display_released: bool) {
    let st = store.status();
    let has_header = st
        .slots
        .get(st.active as usize)
        .is_some_and(|s| s.digest16 != [0; 16]);
    if has_header {
        let v = st.slots[st.active as usize].version;
        RUNNING_VERSION.store(v.raw(), Ordering::Release);
        VERSION_KNOWN.store(true, Ordering::Release);
    }
    DISPLAY_READY.store(display_released, Ordering::Release);
}

/// The board definition of a board without extras (`board::PlainBoard`), for
/// `NodePeripherals::board` when the app wires the link itself. Call once;
/// a second call resets the chip (there is only one link task).
pub fn plain_board() -> &'static mut PlainBoard {
    static PLAIN: StaticCell<PlainBoard> = StaticCell::new();
    PLAIN
        .try_init(PlainBoard)
        .unwrap_or_else(|| esp_hal::system::software_reset())
}

/// Milliseconds since boot, wrapping: the time base of [`Liveness`] deadlines
/// (the one the watchdog checks them against).
pub fn now_ms() -> u32 {
    use dagon_port::Clock;
    (EspClock.now_us() / 1000) as u32
}

/// Starts the DAGON side: the link task on a high-priority interrupt
/// executor driven by software interrupt 1 (`esp_rtos::start` takes 0).
/// Call once, after `esp_rtos::start`. If the flashed partition table does
/// not match, the task stops and the watchdog resets the chip.
pub fn start(
    _armed: WatchdogArmed,
    p: NodePeripherals<'static>,
    link_irq: FROM_CPU_INTR1<'static>,
    cfg: NodeConfig,
    liveness: &'static Liveness,
) {
    static EXECUTOR: StaticCell<InterruptExecutor<1>> = StaticCell::new();
    let spawner: SendSpawner = EXECUTOR
        .init(InterruptExecutor::new(link_irq))
        .start(LINK_PRIORITY);
    spawner.spawn(bootstrap(p, cfg, liveness).expect("link bootstrap spawned once"));
}

/// Hands the link task its peripherals on the link executor itself: the
/// link task holds non-`Send` state, so it is spawned from there.
#[embassy_executor::task]
async fn bootstrap(p: NodePeripherals<'static>, cfg: NodeConfig, liveness: &'static Liveness) {
    // SAFETY: awaited inside an embassy task, so the context comes from the
    // embassy executor (the documented way to spawn on an InterruptExecutor).
    let spawner = unsafe { Spawner::for_current_executor() }.await;
    spawner.spawn(link_task(p, cfg, liveness).expect("link task spawned once"));
}

#[embassy_executor::task]
async fn link_task(p: NodePeripherals<'static>, cfg: NodeConfig, liveness: &'static Liveness) {
    if let Some(link) = Link::new(p, cfg, liveness) {
        link.run().await;
    }
    // No link: the RTC WDT, already running, resets the chip.
}

type Store = EspStore<
    EspFlash<'static, 'static>,
    EspBootSelect<'static, 'static>,
    TrialWatchdog<RtcWatchdog, EspClock>,
>;
type Node = Agent<Store, NodeHooks>;

impl Otadata for EspBootSelect<'_, '_> {
    fn running(&self) -> u8 {
        self.current_slot()
    }
    fn entries(&self) -> Result<[[u8; ENTRY_LEN]; 2], StoreError> {
        EspBootSelect::entries(self).map_err(|_| StoreError::Flash)
    }
    fn set_next(&mut self, slot: u8) -> Result<(), StoreError> {
        BootSelectV::set_next(self, slot).map_err(|_| StoreError::Flash)
    }
    fn confirm(&mut self) -> Result<(), StoreError> {
        EspBootSelect::confirm(self).map_err(|_| StoreError::Flash)
    }
}

pub struct NodeHooks {
    self_check: fn() -> bool,
}

impl Hooks for NodeHooks {
    fn self_check(&mut self) -> bool {
        (self.self_check)()
    }
}

/// What ENTER_ROM needs besides the RWDT: the super watchdog and the board
/// (method 2).
struct RomBoard {
    swd: Swd,
    board: &'static mut dyn BoardDef,
}

#[derive(Clone, Copy)]
enum Via {
    Uart,
    Usb,
}

struct Link {
    agent: Node,
    uart: Uart<'static, Async>,
    usb: UsbSerialJtag<'static, Async>,
    board: RomBoard,
    lease: Lease,
}

fn trying(store: &mut Store) -> bool {
    let st = store.status();
    st.slots
        .get(st.active as usize)
        .is_some_and(|s| s.state == SlotState::Trying)
}

impl Link {
    fn new(
        p: NodePeripherals<'static>,
        cfg: NodeConfig,
        liveness: &'static Liveness,
    ) -> Option<Self> {
        static STORAGE: StaticCell<RefCell<FlashStorage<'static>>> = StaticCell::new();
        // Armed by `arm_watchdog`; not restarted here (a start feeds it).
        let rtc = Rtc::new(p.rtc_timer);
        let rwdt = RtcWatchdog::new(rtc.rwdt);

        // Board bring-up, once, before serving anything.
        let bring_up = p.board.bring_up();
        let board = RomBoard {
            swd: rtc.swd,
            board: p.board,
        };
        let uart = p.uart.into_async();
        let usb = UsbSerialJtag::new(p.usb_device).into_async();

        let storage = STORAGE.init(RefCell::new(FlashStorage::new(p.flash)));
        let boot = EspBootSelect::new(storage).ok()?;
        let wdt = TrialWatchdog::new(rwdt, EspClock).with_liveness(liveness);
        let mut store = EspStore::new(EspFlash::new(storage), boot, wdt);
        if trying(&mut store) {
            store.wdt.begin_trial();
        } else {
            store.wdt.end_trial();
        }
        publish_startup(&mut store, bring_up.display_ready);
        let hooks = NodeHooks {
            self_check: cfg.self_check,
        };
        Some(Self {
            agent: Agent::new(
                crate::config(cfg.mode, crate::target_tag(p.target_tag)),
                store,
                hooks,
            ),
            uart,
            usb,
            board,
            lease: liveness.register_preempting()?,
        })
    }

    async fn run(mut self) {
        let (mut uart_cut, mut usb_cut) = (FrameCutter::new(), FrameCutter::new());
        let (mut ub, mut vb) = ([0u8; 64], [0u8; 64]);
        let idle = Duration::from_millis(IDLE_TIMEOUT_MS.into());
        let mut last = Instant::now();
        loop {
            self.agent.store().wdt.feed();
            if last.elapsed() >= idle {
                self.agent.abort();
                last = Instant::now();
            }
            let ev = select3(
                self.uart.read_async(&mut ub),
                self.usb.read(&mut vb),
                Timer::after(FEED_PERIOD),
            )
            .await;
            match ev {
                Either3::First(Ok(n)) => {
                    let t = now_ms();
                    for &b in &ub[..n] {
                        if let Some(f) = uart_cut.push_at(b, t) {
                            last = Instant::now();
                            if self.serve(f, Via::Uart).await >= INTER_BYTE_TIMEOUT_MS {
                                // Bytes that came in meanwhile wait in the FIFO
                                // and read as one batch with one time: no
                                // partial frame may run across that section.
                                uart_cut.drop_partial();
                                usb_cut.drop_partial();
                            }
                        }
                    }
                }
                // FIFO overflow or line error: the partial frame is lost.
                Either3::First(Err(_)) => uart_cut.clear(),
                Either3::Second(Ok(n)) => {
                    let t = now_ms();
                    for &b in &vb[..n] {
                        if let Some(f) = usb_cut.push_at(b, t) {
                            last = Instant::now();
                            if self.serve(f, Via::Usb).await >= INTER_BYTE_TIMEOUT_MS {
                                // Bytes that came in meanwhile wait in the FIFO
                                // and read as one batch with one time: no
                                // partial frame may run across that section.
                                uart_cut.drop_partial();
                                usb_cut.drop_partial();
                            }
                        }
                    }
                }
                Either3::Second(Err(_)) => usb_cut.clear(),
                Either3::Third(()) => {}
            }
        }
    }

    /// Answers one request; resets after the reply to REBOOT, switches to the
    /// ROM loader after the reply to ENTER_ROM if that reply left the node.
    /// Returns the longest synchronous section (ms) it ran.
    async fn serve(&mut self, frame: &[u8], via: Via) -> u32 {
        let start = now_ms();
        self.lease.busy(start, REQUEST_BUDGET_MS);
        self.lease.hold_cpu(start);
        let mut out = [0u8; REPLY_MAX];
        let decided = self.decide(frame, &mut out);
        let mut held = self.lease.release_cpu(now_ms());
        if let Some((n, then)) = decided {
            let sent = send(&mut self.uart, &mut self.usb, via, &out[..n]).await;
            held = held.max(self.after_reply(serve::next(then, sent)));
        }
        self.lease.idle();
        held
    }

    /// The synchronous part: the reply to send and what follows it.
    fn decide(&mut self, frame: &[u8], out: &mut [u8; REPLY_MAX]) -> Option<(usize, Then)> {
        let max = dagon_proto::max_payload::UART;
        let mut rom_reply = [0u8; rom::REPLY_LEN];
        let pmic = self.board.board.pmic_download_supported();
        if let Some((n, method)) = rom::enter_rom_request(frame, max, pmic, &mut rom_reply) {
            out[..n].copy_from_slice(&rom_reply[..n]);
            return Some((n, method.map_or(Then::Nothing, Then::Rom)));
        }
        let agent = &mut self.agent;
        #[cfg(feature = "conform")]
        if let Some(n) = conform(agent, frame, &mut out[..]) {
            return Some((n, Then::Nothing));
        }
        let mut refusal = [0u8; crate::REFUSAL_LEN];
        if let Some(n) = crate::refuse_set_pending_on_trial(agent.store(), frame, &mut refusal) {
            out[..n].copy_from_slice(&refusal[..n]);
            return Some((n, Then::Nothing));
        }
        let reply = agent.handle(frame)?;
        let n = reply.frame.len();
        out.get_mut(..n)?.copy_from_slice(reply.frame);
        let then = if reply.reboot {
            Then::Reboot
        } else {
            Then::CheckTrial
        };
        Some((n, then))
    }

    /// Runs the step after a reply; returns how long it kept the CPU (ms).
    fn after_reply(&mut self, next: Next) -> u32 {
        match next {
            Next::Stay => 0,
            Next::EnterRom(m) => enter_rom(m, &mut self.board, self.agent.store().wdt.inner()),
            Next::RomReplyLost => {
                ROM_REPLY_LOST.fetch_add(1, Ordering::Relaxed);
                0
            }
            Next::Reset => {
                self.agent.hooks().before_reboot();
                EspReset.reset()
            }
            Next::CheckTrial => {
                self.lease.hold_cpu(now_ms());
                let store = self.agent.store();
                if store.wdt.on_trial() && !trying(store) {
                    store.wdt.end_trial();
                }
                self.lease.release_cpu(now_ms())
            }
        }
    }
}

/// ENTER_ROM requests not acted on because their OK did not leave the node.
pub static ROM_REPLY_LOST: AtomicU32 = AtomicU32::new(0);

/// Room for the largest reply frame (agent STATUS 84 B, CONF_RUN 82 B).
const REPLY_MAX: usize = 128;

/// Sends a reply and waits until it has left the node (UART: transmitter
/// idle; USB: written and flushed), within [`SEND_TIMEOUT`]. `false` when it
/// did not. A lost reply is recovered by the sender's retry (same seq).
async fn send(
    uart: &mut Uart<'static, Async>,
    usb: &mut UsbSerialJtag<'static, Async>,
    via: Via,
    frame: &[u8],
) -> bool {
    let done = match via {
        Via::Uart => {
            with_timeout(SEND_TIMEOUT, async {
                let mut sent = 0;
                while sent < frame.len() {
                    sent += uart.write_async(&frame[sent..]).await.map_err(|_| ())?;
                }
                uart.flush_async().await.map_err(|_| ())
            })
            .await
        }
        Via::Usb => {
            with_timeout(SEND_TIMEOUT, async {
                usb.write_all(frame).await.map_err(|_| ())?;
                usb.flush().await.map_err(|_| ())
            })
            .await
        }
    };
    matches!(done, Ok(Ok(())))
}

/// CONF_RUN in a conformance build. The scratch unit is the first erase unit
/// of the OTA slot that is neither running nor the fallback; without one the
/// request falls through to the agent (E_UNSUPPORTED).
#[cfg(feature = "conform")]
fn conform(agent: &mut Node, frame: &[u8], out: &mut [u8]) -> Option<usize> {
    use crate::layout::{app_partition, REGIONS, SLOT_A, SLOT_B};
    use dagon_conform::node::{handle_frame, NodeSetup};
    use dagon_proto::{Cmd, Frame};

    let max = dagon_proto::max_payload::UART;
    if Frame::decode(frame, max).ok()?.0.cmd != Cmd::ConfRun as u8 {
        return None;
    }
    let store = agent.store();
    let st = store.status();
    let idle = [SLOT_A, SLOT_B]
        .into_iter()
        .find(|&s| s != st.active && s != st.confirmed)?;
    let setup = NodeSetup {
        declared: &REGIONS,
        scratch: app_partition(idle)?.offset,
    };
    let (flash, wdt) = (&mut store.flash, &mut store.wdt);
    handle_frame(frame, max, flash, &setup, out, &mut || wdt.feed())
}

/// Runs [`rom::steps`] after the reply has been sent: watchdogs off, the
/// method, then a software reset (into the ROM loader if the method took,
/// otherwise back into the agent).
fn enter_rom(method: Method, board: &mut RomBoard, rwdt: &mut RtcWatchdog) -> ! {
    for step in rom::steps(method) {
        match step {
            Step::StopWatchdogs => {
                rwdt.stop();
                // `Swd::disable` turns the super watchdog's auto-feed on.
                board.swd.disable();
            }
            Step::ForceDownloadBoot => {
                LPWR::regs()
                    .option1()
                    .modify(|_, w| w.force_download_boot().set_bit());
            }
            Step::PmicDownload => board.board.pmic_download(),
            Step::Reset => break,
        }
    }
    esp_hal::system::software_reset()
}
