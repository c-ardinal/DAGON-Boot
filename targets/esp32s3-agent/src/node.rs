//! The DAGON side of a running image: [`start`] takes the peripherals DAGON
//! owns and runs the link task on a high-priority interrupt executor.
//!
//! - The link task is the only owner of UART0 (rear G43/G44), USB
//!   Serial/JTAG, the flash, the RTC/super watchdogs and the internal I2C.
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
//! - UART0 and USB receivers drop a partial frame after 20 ms without a byte.

use core::cell::RefCell;

use dagon_agent::{Agent, Hooks, SlotStore, StoreError, IDLE_TIMEOUT_MS};
use dagon_esp32s3_port::{
    board,
    boot::EspBootSelect,
    flash::EspFlash,
    framer::FrameCutter,
    otadata::ENTRY_LEN,
    sys::{EspClock, EspReset, RtcWatchdog},
    TRIAL_WDT_MS,
};
use dagon_port::{BootSelectV, Reset, Watchdog};
use dagon_proto::payload::SlotState;
use embassy_executor::{SendSpawner, Spawner};
use embassy_futures::select::{select3, Either3};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_io_async::{Read, Write};
use esp_bootloader_esp_idf::partitions::FlashStorage;
#[cfg(feature = "pmic-download")]
use esp_hal::{
    delay::Delay,
    i2c::master::{Config as I2cConfig, Error as I2cError, I2c},
};
use esp_hal::{
    interrupt::Priority,
    peripherals::{
        FLASH, FROM_CPU_INTR1, GPIO43, GPIO44, GPIO47, GPIO48, I2C0, LPWR, RTC_TIMER, UART0,
        USB_DEVICE,
    },
    rtc_cntl::{Rtc, Swd},
    uart::Uart,
    usb::usb_serial_jtag::UsbSerialJtag,
    Async,
};
use esp_rtos::embassy::InterruptExecutor;
use static_cell::StaticCell;

use crate::liveness::{Lease, Liveness};
use crate::rom::{self, Method, Step};
use crate::store::{EspStore, Otadata};
use crate::trial::TrialWatchdog;

/// Priority of the link executor (the app's tasks run in thread mode, below).
pub const LINK_PRIORITY: Priority = Priority::Priority2;
/// The link task wakes at least this often to feed the watchdog.
const FEED_PERIOD: Duration = Duration::from_millis(500);
/// Time one request may take, a full slot erase included.
const REQUEST_BUDGET_MS: u32 = 120_000;
/// Give up on a USB reply after this long (host stopped reading).
const USB_SEND_TIMEOUT: Duration = Duration::from_millis(200);
/// Time the M5PM1 gets to reset the chip into download mode.
#[cfg(feature = "pmic-download")]
const M5PM1_WAIT_MS: u32 = 1000;

/// Everything DAGON owns on the watch.
pub struct NodePeripherals<'d> {
    pub rtc_timer: RTC_TIMER<'d>,
    pub uart0: UART0<'d>,
    /// Rear MUX_IO TX.
    pub gpio43: GPIO43<'d>,
    /// Rear MUX_IO RX.
    pub gpio44: GPIO44<'d>,
    pub usb_device: USB_DEVICE<'d>,
    pub flash: FLASH<'d>,
    /// Internal SYS I2C (M5IOE1 expander, M5PM1 power manager).
    pub i2c0: I2C0<'d>,
    pub gpio47: GPIO47<'d>,
    pub gpio48: GPIO48<'d>,
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

/// Starts the RTC WDT (4 s) right away. Call it first, right after
/// `esp_hal::init` (which stops every watchdog), so an image that stalls
/// before [`start`] still resets and, on trial, rolls back. The link task
/// takes the watchdog over later.
pub fn arm_watchdog(rtc_timer: RTC_TIMER<'_>) {
    RtcWatchdog::new(Rtc::new(rtc_timer).rwdt).start(TRIAL_WDT_MS);
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

/// What ENTER_ROM needs besides the RWDT: the super watchdog, and the
/// internal I2C kept after the rear pins are routed, for method 2
/// (`pmic-download`).
struct RomBoard {
    swd: Swd,
    #[cfg_attr(not(feature = "pmic-download"), allow(dead_code))]
    i2c0: I2C0<'static>,
    #[cfg_attr(not(feature = "pmic-download"), allow(dead_code))]
    sda: GPIO47<'static>,
    #[cfg_attr(not(feature = "pmic-download"), allow(dead_code))]
    scl: GPIO48<'static>,
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
        let rtc = Rtc::new(p.rtc_timer);
        let mut rwdt = RtcWatchdog::new(rtc.rwdt);
        rwdt.start(TRIAL_WDT_MS);

        let mut board = RomBoard {
            swd: rtc.swd,
            i2c0: p.i2c0,
            sda: p.gpio47,
            scl: p.gpio48,
        };
        // Without the expander the rear pins stay at their default (UART).
        let _ = board::route_rear_uart(
            board.i2c0.reborrow(),
            board.sda.reborrow(),
            board.scl.reborrow(),
        );
        let uart = board::rear_uart(p.uart0, p.gpio43, p.gpio44).into_async();
        let usb = UsbSerialJtag::new(p.usb_device).into_async();

        let storage = STORAGE.init(RefCell::new(FlashStorage::new(p.flash)));
        let boot = EspBootSelect::new(storage).ok()?;
        let wdt = TrialWatchdog::new(rwdt, EspClock).with_liveness(liveness);
        let mut store = EspStore::new(EspFlash::new(storage), boot, wdt);
        if trying(&mut store) {
            store.wdt.begin_trial();
        }
        let hooks = NodeHooks {
            self_check: cfg.self_check,
        };
        Some(Self {
            agent: Agent::new(crate::config(cfg.mode), store, hooks),
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
                    for &b in &ub[..n] {
                        if let Some(f) = uart_cut.push_at(b, now_ms()) {
                            last = Instant::now();
                            self.serve(f, Via::Uart).await;
                        }
                    }
                }
                // FIFO overflow or line error: the partial frame is lost.
                Either3::First(Err(_)) => uart_cut.clear(),
                Either3::Second(Ok(n)) => {
                    for &b in &vb[..n] {
                        if let Some(f) = usb_cut.push_at(b, now_ms()) {
                            last = Instant::now();
                            self.serve(f, Via::Usb).await;
                        }
                    }
                }
                Either3::Second(Err(_)) => usb_cut.clear(),
                Either3::Third(()) => {}
            }
        }
    }

    /// Answers one request; resets after the reply to REBOOT, switches to the
    /// ROM loader after the reply to ENTER_ROM.
    async fn serve(&mut self, frame: &[u8], via: Via) {
        let now = self.agent.store().wdt.now_ms();
        self.lease.busy(now, REQUEST_BUDGET_MS);
        self.answer(frame, via).await;
        let now = self.agent.store().wdt.now_ms();
        self.lease.idle(now);
    }

    async fn answer(&mut self, frame: &[u8], via: Via) {
        let Link {
            agent,
            uart,
            usb,
            board,
            ..
        } = self;
        let mut rom_reply = [0u8; rom::REPLY_LEN];
        let max = dagon_proto::max_payload::UART;
        if let Some((n, method)) = rom::enter_rom_request(frame, max, &mut rom_reply) {
            send(uart, usb, via, &rom_reply[..n]).await;
            if let Some(m) = method {
                enter_rom(m, board, agent.store().wdt.inner());
            }
            return;
        }
        #[cfg(feature = "conform")]
        {
            let mut out = [0u8; dagon_conform::node::RESPONSE_MAX];
            if let Some(n) = conform(agent, frame, &mut out) {
                send(uart, usb, via, &out[..n]).await;
                return;
            }
        }
        let mut refusal = [0u8; crate::REFUSAL_LEN];
        if let Some(n) = crate::refuse_set_pending_on_trial(agent.store(), frame, &mut refusal) {
            send(uart, usb, via, &refusal[..n]).await;
            return;
        }
        let Some(reply) = agent.handle(frame) else {
            return;
        };
        let reboot = reply.reboot;
        send(uart, usb, via, reply.frame).await;
        if reboot {
            agent.hooks().before_reboot();
            EspReset.reset();
        }
        let store = agent.store();
        if store.wdt.on_trial() && !trying(store) {
            store.wdt.end_trial();
        }
    }
}

/// Sends a reply and waits until it has left the node (the caller may reset
/// right after). A lost reply is recovered by the sender's retry (same seq).
async fn send(
    uart: &mut Uart<'static, Async>,
    usb: &mut UsbSerialJtag<'static, Async>,
    via: Via,
    frame: &[u8],
) {
    match via {
        Via::Uart => {
            let mut sent = 0;
            while sent < frame.len() {
                match uart.write_async(&frame[sent..]).await {
                    Ok(n) => sent += n,
                    Err(_) => return,
                }
            }
            let _ = uart.flush_async().await;
        }
        Via::Usb => {
            let _ = with_timeout(USB_SEND_TIMEOUT, async {
                usb.write_all(frame).await?;
                usb.flush().await
            })
            .await;
        }
    }
}

/// CONF_RUN in a conformance build. The scratch unit is the first erase unit
/// of the OTA slot that is neither running nor the fallback; without one the
/// request falls through to the agent (E_UNSUPPORTED).
#[cfg(feature = "conform")]
fn conform(agent: &mut Node, frame: &[u8], out: &mut [u8]) -> Option<usize> {
    use dagon_conform::node::{handle_frame, NodeSetup};
    use dagon_esp32s3_port::layout::{app_partition, REGIONS, SLOT_A, SLOT_B};
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
            Step::PmicDownload => pmic_download(board),
            Step::Reset => break,
        }
    }
    esp_hal::system::software_reset()
}

#[cfg(feature = "pmic-download")]
fn pmic_download(board: &mut RomBoard) {
    if m5pm1_download(board).is_ok() {
        Delay::new().delay_millis(M5PM1_WAIT_MS);
    }
}

/// Not reached: without `pmic-download` method 2 answers E_UNSUPPORTED.
#[cfg(not(feature = "pmic-download"))]
fn pmic_download(_: &mut RomBoard) {}

#[cfg(feature = "pmic-download")]
fn m5pm1_download(board: &mut RomBoard) -> Result<(), I2cError> {
    let mut i2c = I2c::new(board.i2c0.reborrow(), I2cConfig::default())
        .map_err(|_| I2cError::Timeout)?
        .with_sda(board.sda.reborrow())
        .with_scl(board.scl.reborrow());
    i2c.write(rom::M5PM1_ADDR, &[rom::M5PM1_SYS_CMD, rom::M5PM1_DOWNLOAD])
}
