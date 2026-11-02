//! M5StopWatch demo app (ota_0 / ota_1).
//!
//! Tasks and priorities:
//!
//! | Task | Executor | Owns |
//! |---|---|---|
//! | DAGON link (`node::start`) | interrupt executor, priority 2 | UART0, USB Serial/JTAG, flash, RTC/super watchdogs, internal I2C |
//! | `heartbeat` (demo work) | thread mode (this `main`), below every interrupt | nothing yet |
//!
//! Every task registers in [`LIVENESS`] and announces a deadline before each
//! piece of work; the watchdog is fed only while none is overdue, so a stuck
//! demo task resets the watch instead of hanging it. DAGON traffic is served
//! at the higher priority and never waits for demo work.
#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use dagon_esp32s3_agent::liveness::{Lease, Liveness};
use dagon_esp32s3_agent::node::{self, NodeConfig, NodePeripherals};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::timer::timg::TimerGroup;

esp_bootloader_esp_idf::esp_app_desc!();

static LIVENESS: Liveness = Liveness::new();
/// Completed heartbeats: the demo's progress, checked on CONFIRM.
static BEATS: AtomicU32 = AtomicU32::new(0);

const BEAT_PERIOD: Duration = Duration::from_secs(1);
/// Longest a heartbeat's work may take before the watch is reset.
const BEAT_BUDGET_MS: u32 = 500;

/// CONFIRM only once the demo has shown progress (never in a `no-confirm`
/// build).
fn self_check() -> bool {
    !cfg!(feature = "no-confirm") && BEATS.load(Ordering::Relaxed) > 0
}

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    let mut p = esp_hal::init(esp_hal::Config::default());
    node::arm_watchdog(p.RTC_TIMER.reborrow());
    esp_rtos::start(TimerGroup::new(p.TIMG0).timer0, p.FROM_CPU_INTR0);
    let dagon = NodePeripherals {
        rtc_timer: p.RTC_TIMER,
        uart0: p.UART0,
        gpio43: p.GPIO43,
        gpio44: p.GPIO44,
        usb_device: p.USB_DEVICE,
        flash: p.FLASH,
        i2c0: p.I2C0,
        gpio47: p.GPIO47,
        gpio48: p.GPIO48,
    };
    node::start(
        dagon,
        p.FROM_CPU_INTR1,
        NodeConfig::app(self_check),
        &LIVENESS,
    );
    // Without a lease or a task the demo shows no progress: CONFIRM is
    // refused and the trial rolls back, which is the right outcome.
    if let Some(lease) = LIVENESS.register() {
        if let Ok(task) = heartbeat(lease) {
            spawner.spawn(task);
        }
    }
}

/// Placeholder demo work: counts heartbeats. Each beat is announced to the
/// watchdog with a budget; the wait in between is idle.
#[embassy_executor::task]
async fn heartbeat(lease: Lease) {
    loop {
        lease.busy(node::now_ms(), BEAT_BUDGET_MS);
        BEATS.fetch_add(1, Ordering::Relaxed);
        lease.idle(node::now_ms());
        Timer::after(BEAT_PERIOD).await;
    }
}
