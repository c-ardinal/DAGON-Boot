//! NUCLEO-F401RE demo app with DAGON: an I2C target at
//! [`board::I2C_ADDR`] on I2C3 (PA8 SCL, PB4 SDA) serving dagon-agent
//! through the mailbox, and its own version on the 3-digit 7-segment
//! display (`1.1.` for 1.1.x). Built for slot A or B (`slot-b`); the boot
//! core picks which one runs. DAGON comes from the MCU package
//! (dagon-mcu-stm32f401re) and the board package (dagon-board-nucleo-f401re).
//!
//! Tasks (each peripheral owned by one task):
//! - DAGON's interrupt executor (`exec`, on the unused USART6 interrupt;
//!   preempts thread mode): mailbox serving (`node::serve`), owning I2C3 (target) and the
//!   agent with the flash, so the hub is answered whatever the display does;
//! - thread executor: display multiplexing (segment and digit pins) and the
//!   watchdog task (IWDG, LED heartbeat).
//!
//! Watchdog: fed by its task only while no task is past the deadline of its
//! current work, and never while on trial: an image that does not get
//! CONFIRM resets when the 4 s trial watchdog runs out and the boot core
//! counts the attempt (rolling back after the third).
//!
//! `confirm`: confirms its own trial at start-up (bench use without a hub).
#![no_std]
#![no_main]

use board::{Board, BoardDef};
use dagon_agent::{Agent, SlotStore};
use dagon_board_nucleo_f401re as board;
use dagon_board_nucleo_f401re::seg;
use dagon_mcu_stm32f401re::node::{self, ACTIVE};
use dagon_mcu_stm32f401re::watchdog::{now_ms, Guard, Level, Trial};
use defmt::{error, info, Display2Format};
use defmt_rtt as _;
use embassy_executor::Executor;
use embassy_stm32::gpio::{Level as Pin, Output, Speed};
use embassy_time::{Duration, Instant, Timer};
use panic_probe as _;
use static_cell::StaticCell;

const SELF_CONFIRM: bool = cfg!(feature = "confirm");

/// How long "TEST KEY" (`tSt`) shows before the version.
const TEST_KEY_SPLASH: Duration = Duration::from_millis(1500);
/// On time of one digit of the multiplexed display.
const DIGIT_PERIOD: Duration = Duration::from_millis(2);

/// Watchdog guard tasks: mailbox serving (preempting), display.
mod task {
    pub const SERVE: usize = 0;
    pub const DISPLAY: usize = 1;
}

static GUARD: Guard<2> = Guard::new(
    [Level::Preempting, Level::Background],
    Trial::NeverFeed,
    now_ms,
);

static EXECUTOR_LOW: StaticCell<Executor> = StaticCell::new();

#[cortex_m_rt::entry]
fn main() -> ! {
    let (dagon, io) = Board::split(embassy_stm32::init(dagon_mcu_stm32f401re::clocks()));
    let led = Output::new(io.led, Pin::Low, Speed::Low);
    let (segments, digits) = io.display.outputs();
    let low = EXECUTOR_LOW.init(Executor::new());

    let mut store = match node::open_store(dagon.flash) {
        Ok(s) => s,
        Err(e) => {
            error!("state log unreadable: {}", defmt::Debug2Format(&e));
            low.run(|s| {
                s.spawn(display(segments, digits, seg::DASHES, seg::DASHES).unwrap());
                s.spawn(blink(led).unwrap());
            })
        }
    };
    let own = node::own_header(&mut store);
    let version = node::own_version(&mut store);
    let splash = if dagon_mcu_stm32f401re::TEST_KEY {
        info!("TEST KEY: built with the public test verifying key");
        seg::TEST_KEY
    } else {
        seg::version(version)
    };

    let s = store.log_state();
    let on_trial = node::on_trial(&store);
    info!(
        "app {} slot {} on_trial={} attempts={} last_reason={}",
        Display2Format(&version),
        ACTIVE,
        on_trial,
        s.attempts,
        s.last_reason
    );
    if on_trial && SELF_CONFIRM {
        match own.map(|h| store.confirm(ACTIVE, &h)) {
            Some(Ok(())) => info!("confirmed slot {}", ACTIVE),
            _ => error!("self-confirm failed"),
        }
    }

    let period = node::watchdog_period_ms(on_trial);
    node::install_watchdog(dagon.iwdg, &GUARD, period, &store);
    let (target, regs) = Board::open_link(dagon.link);
    let agent = Agent::new(
        node::config(dagon_mcu_stm32f401re::target_tag!()),
        store,
        node::SelfCheck(node::always),
    );

    let high = dagon_mcu_stm32f401re::exec::start();
    let link = dagon_mcu_stm32f401re::links::I2c::new(target, regs);
    high.spawn(node::serve(agent, link, &GUARD, task::SERVE).unwrap());

    low.run(|s| {
        s.spawn(display(segments, digits, splash, seg::version(version)).unwrap());
        s.spawn(watchdog(led).unwrap());
    })
}

/// Blinks the LED fast forever (no usable state log).
#[embassy_executor::task]
async fn blink(mut led: Output<'static>) -> ! {
    loop {
        led.toggle();
        Timer::after_millis(100).await;
    }
}

/// Feeds while every task is alive; the LED blinks as a heartbeat.
#[embassy_executor::task]
async fn watchdog(mut led: Output<'static>) -> ! {
    loop {
        GUARD.feed();
        led.toggle();
        Timer::after(node::FEED_EVERY).await;
    }
}

/// Multiplexes the display: `first` for the splash time, then `then`.
#[embassy_executor::task]
async fn display(
    mut segments: [Output<'static>; 8],
    mut digits: [Output<'static>; seg::DIGITS],
    first: [u8; seg::DIGITS],
    then: [u8; seg::DIGITS],
) {
    let started = Instant::now();
    loop {
        GUARD.busy(task::DISPLAY);
        let codes = if started.elapsed() < TEST_KEY_SPLASH {
            first
        } else {
            then
        };
        for (i, code) in codes.iter().enumerate() {
            for (bit, s) in segments.iter_mut().enumerate() {
                s.set_level(if code & (1 << bit) != 0 {
                    board::SEG_ON
                } else {
                    board::SEG_OFF
                });
            }
            digits[i].set_level(board::DIGIT_ON);
            Timer::after(DIGIT_PERIOD).await;
            digits[i].set_level(board::DIGIT_OFF);
        }
    }
}
