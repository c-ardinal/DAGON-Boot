//! NUCLEO-F401RE test app for the boot core. Built for slot A or B
//! (`slot-b`), it reports the state log over RTT and blinks LD2: fast
//! (100 ms) while on trial, slow (500 ms) once confirmed.
//!
//! With `confirm` it confirms its own trial at start-up (stand-in for the
//! hub's CONFIRM) and feeds the watchdog. Without it, it never confirms and
//! never feeds the watchdog while on trial, so the boot core rolls it back.
#![no_std]
#![no_main]

use dagon_core::log::Log;
use dagon_format::header::{ImageHeader, HEADER_SIZE};
use dagon_format::statelog::{kind, reason};
use dagon_port::Flash;
use defmt::{error, info, Display2Format};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_time::{Duration, Timer};
use f401_board as board;
use panic_probe as _;

const SLOT: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };
const CONFIRMS: bool = cfg!(feature = "confirm");
const BLINK_TRIAL: Duration = Duration::from_millis(100);
const BLINK_CONFIRMED: Duration = Duration::from_millis(500);

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    let mut led = Output::new(p.PA5, Level::Low, Speed::Low);
    let mut flash = board::BoardFlash::new(p.FLASH);

    let mut hdr = [0u8; HEADER_SIZE];
    let version = flash
        .read(board::SLOTS.slots[SLOT as usize], &mut hdr)
        .ok()
        .and_then(|_| ImageHeader::parse(&hdr).ok())
        .map(|h| h.version)
        .unwrap_or_default();

    let mut log = match Log::load(&mut flash, &board::LOG) {
        Ok(log) => log,
        Err(e) => {
            error!("state log unreadable: {}", defmt::Debug2Format(&e));
            loop {
                Timer::after(BLINK_TRIAL).await;
            }
        }
    };
    let s = log.state;
    let on_trial = s.on_trial() && s.active() == SLOT;
    if board::TEST_KEY {
        info!("TEST KEY: built with the public test verifying key");
    }
    info!(
        "app {} slot {} confirm-build={} on_trial={} attempts={} confirmed_slot={} last_reason={}",
        Display2Format(&version),
        SLOT,
        CONFIRMS,
        on_trial,
        s.attempts,
        s.confirmed.map(|c| c.slot),
        s.last_reason
    );

    let mut confirmed = !on_trial;
    if on_trial && CONFIRMS {
        let pending = s.pending.expect("on trial implies a pending slot");
        match log.append(&mut flash, kind::CONFIRMED, pending, reason::NONE) {
            Ok(()) => {
                info!("confirmed slot {}", SLOT);
                confirmed = true;
            }
            Err(e) => error!("confirm failed: {}", defmt::Debug2Format(&e)),
        }
    }

    // Unconfirmed: leave the boot core's trial watchdog running unfed.
    let mut wdt = confirmed.then(|| {
        let mut w = IndependentWatchdog::new(p.IWDG, board::CONFIRMED_WDT_MS * 1000);
        w.pet();
        w
    });
    let blink = if confirmed {
        BLINK_CONFIRMED
    } else {
        BLINK_TRIAL
    };
    loop {
        led.toggle();
        if let Some(w) = wdt.as_mut() {
            w.pet();
        }
        Timer::after(blink).await;
    }
}
