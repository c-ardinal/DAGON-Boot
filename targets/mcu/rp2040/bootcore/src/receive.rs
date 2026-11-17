//! Update and recovery mode over USB: the update session (dagon-agent with
//! the boot core writer, an app chain without app extensions) served by
//! the loop every MCU package runs, on the USB device the app presents
//! (same IDs and serial, so the host finds the node again). One task on a
//! thread-mode executor serves; the USB device runs in its own task.
//!
//! The watchdog is fed by the serving loop each time it comes round (at
//! least every [`TICK`]), at every request, every erased sector and every
//! step of a signature check; a loop that stops coming round resets the
//! node.

use dagon_agent::bootcore::{Entry, IdleExit};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, AppChain, SlotStore};
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog as Periods};
use dagon_format::SigVerifier;
use dagon_mcu_common::guard::Watch;
use dagon_mcu_common::link::{Kind, Link, LinkError, Reconnect};
use dagon_mcu_common::serve::{serve as serve_loop, Exit, ServeHost};
use dagon_mcu_common::shared::{driver, FlashDriver, SharedFlash};
use dagon_mcu_rp2040::flash::{RpFlash, REGIONS};
use dagon_mcu_rp2040::layout::watchdog_ms;
use dagon_mcu_rp2040::node;
use dagon_mcu_rp2040::store::AppStore;
use dagon_mcu_rp2040::usb::{self, CdcLink};
use dagon_mcu_rp2040::{ERASE_REGIONS, TARGET_TAG};
use dagon_proto::payload::role;
use embassy_executor::{Executor, Spawner};
use embassy_futures::select::{select, Either};
use embassy_rp::peripherals::USB;
use embassy_rp::Peri;
use embassy_time::{Duration, Instant, Timer};
use static_cell::StaticCell;

use crate::{feed, reset, Verifier, VERIFIER};

/// How long a read waits before the loop comes round: well inside the
/// watchdog's period.
const TICK: Duration = Duration::from_secs(1);

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type Session = Agent<AppStore, Chain, Verifier, BootCore>;

/// Serves the update session in `entry`'s mode on USB until a REBOOT's
/// reply has left or, entered by STAY or the hand-over, no request came
/// for 60 s; then resets.
pub fn serve(usb: Peri<'static, USB>, mut flash: RpFlash, entry: Entry) -> ! {
    static EXECUTOR: StaticCell<Executor> = StaticCell::new();
    let serial = node::serial(&mut flash);
    EXECUTOR.init(Executor::new()).run(|spawner| {
        spawner.spawn(session(usb, flash, serial, entry).expect("task pool"));
    })
}

#[embassy_executor::task]
async fn session(usb: Peri<'static, USB>, flash: RpFlash, serial: &'static str, entry: Entry) {
    static CHIP: StaticCell<FlashDriver<RpFlash>> = StaticCell::new();
    static EVENTS: StaticCell<Published<(), 4>> = StaticCell::new();
    let (device, link) = usb::new(usb, TARGET_TAG, serial);
    // SAFETY: awaited directly inside this embassy task, whose context
    // comes from the executor running it.
    let spawner = unsafe { Spawner::for_current_executor() }.await;
    spawner.spawn(usb::run(device).expect("task pool"));
    let store = AppStore::new(
        SharedFlash::new(CHIP.init(driver(flash)), &REGIONS),
        &CORE,
        0,
        &ERASE_REGIONS,
    );
    let now: fn() -> u64 = || Instant::now().as_micros();
    let periods = Periods::new(watchdog_ms(true), watchdog_ms(false));
    let chain = compose(
        Layer::App,
        EVENTS.init(Published::new()),
        Nop,
        Nop,
        Bounds::new(now, periods),
    );
    let cfg = dagon_agent::Config {
        mode: entry.mode(),
        ..node::config(TARGET_TAG, role::NODE)
    };
    let mut agent: Session = Agent::for_writer(cfg, VERIFIER, store, chain);
    let mut link = IdleExit::new(Ticked(link), || Instant::now().as_millis(), entry);
    loop {
        match serve_loop(&mut link, &mut agent, &mut CoreHost).await {
            Exit::Closed if !link.expired() => continue,
            _ => reset(),
        }
    }
}

/// The USB link with reads that give up after [`TICK`], so the loop
/// comes round to feed the watchdog and to count the idle time.
struct Ticked(CdcLink);

impl Link for Ticked {
    fn max_payload(&self) -> usize {
        self.0.max_payload()
    }
    fn reconnect(&self) -> Reconnect {
        self.0.reconnect()
    }
    fn kind(&self) -> Kind {
        self.0.kind()
    }
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        match select(self.0.read_frame(), Timer::after(TICK)).await {
            Either::First(r) => r,
            Either::Second(()) => Err(LinkError::Timeout),
        }
    }
    async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
        self.0.write_frame(frame).await
    }
    async fn flush(&mut self) -> Result<(), LinkError> {
        self.0.flush().await
    }
}

/// The boot core's watchdog as the slot store and the serving loop see
/// it: one task, fed directly (the boot core runs nothing else).
struct Core;

static CORE: Core = Core;

impl Watch for Core {
    fn busy(&self, _: usize) {}
    fn busy_for(&self, _: usize, _: u64) {}
    fn idle(&self, _: usize) {}
    fn feed(&self) {
        feed();
    }
    fn set_trial(&self, _: bool) {}
    fn reboot(&self) -> ! {
        reset()
    }
}

struct CoreHost;

impl<S: SlotStore, X: AppChain, V: SigVerifier> ServeHost<S, X, V, BootCore> for CoreHost {
    fn now_ms(&self) -> u64 {
        Instant::now().as_millis()
    }
    /// The loop came round.
    fn idle(&mut self) {
        feed();
    }
    fn busy(&mut self) {}
    fn feed(&mut self) {
        feed();
    }
    async fn wait_ms(&mut self, ms: u32) {
        Timer::after_millis(ms.into()).await;
    }
}
