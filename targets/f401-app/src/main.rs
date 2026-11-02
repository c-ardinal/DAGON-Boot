//! NUCLEO-F401RE demo app with DAGON: an I2C target at [`I2C_ADDR`] on I2C3
//! (PA8 SCL, PB4 SDA) serving dagon-agent through the mailbox, and its own
//! version on the 3-digit 7-segment display (`1.1.` for 1.1.x). Built for
//! slot A or B (`slot-b`); the boot core picks which one runs.
//!
//! Watchdog: fed every [`FEED_PERIOD`] once confirmed. On trial it is fed
//! for [`TRIAL_FEED_MS`] only, so an image that never gets CONFIRM resets and
//! the boot core counts the attempt (rolling back after the last one).
//!
//! `confirm`: confirms its own trial at start-up (bench use without a hub).
#![no_std]
#![no_main]

use cortex_m::peripheral::SCB;
use dagon_agent::{Agent, Config, DefaultHooks, Hooks, SlotStore, IDLE_TIMEOUT_MS};
use dagon_format::header::{port_kind, ImageHeader, HEADER_SIZE};
use dagon_proto::payload::{mode, role};
use dagon_proto::{frame_len, max_payload};
use defmt::{error, info, warn, Display2Format};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_stm32::gpio::{AnyPin, Level, Output, Speed};
use embassy_stm32::i2c::{self, I2c, SlaveAddrConfig, SlaveCommandKind};
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::{bind_interrupts, dma, pac, peripherals, Peri};
use embassy_time::{Duration, Instant, Timer};
use f401_board as board;
use f401_node::mailbox::Mailbox;
use f401_node::{seg, KindSStore};
use panic_probe as _;

const SLOT: u8 = if cfg!(feature = "slot-b") { 1 } else { 0 };
const SELF_CONFIRM: bool = cfg!(feature = "confirm");

/// 7-bit I2C target address (the STM32 ROM loader keeps 0x39).
const I2C_ADDR: u8 = 0x42;
const FEED_PERIOD: Duration = Duration::from_millis(500);
/// How long a trial image keeps the watchdog fed while waiting for CONFIRM.
const TRIAL_FEED_MS: u64 = 30_000;
/// How long "TEST KEY" (`tSt`) shows before the version.
const TEST_KEY_SPLASH: Duration = Duration::from_millis(1500);
/// On time of one digit of the multiplexed display.
const DIGIT_PERIOD: Duration = Duration::from_millis(2);
/// Requests taking longer are timed on RTT (sizes the hub's wait windows).
const SLOW_REQUEST_MS: u64 = 20;

// 7-segment expansion board wiring. The board's part number and pin map are
// not confirmed yet: direct drive, segments a..g + dp and three digit
// selects on Arduino header pins clear of I2C3, I2C2 (D6/D3), LD2 and the
// ST-LINK UART. Adjust here (and in `display_pins`) once the board is known.
const SEG_ON: Level = Level::High;
const SEG_OFF: Level = Level::Low;
const DIGIT_ON: Level = Level::Low;
const DIGIT_OFF: Level = Level::High;

bind_interrupts!(struct Irqs {
    I2C3_EV => i2c::EventInterruptHandler<peripherals::I2C3>;
    I2C3_ER => i2c::ErrorInterruptHandler<peripherals::I2C3>;
    DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
});

type Store = KindSStore<board::BoardFlash<'static>>;
type Wdt = IndependentWatchdog<'static, peripherals::IWDG>;

fn config() -> Config {
    Config {
        pubkey: board::PUBKEY,
        mcu_id: board::MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: board::TARGET_TAG,
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(board::clocks());
    let mut led = Output::new(p.PA5, Level::Low, Speed::Low);
    let o = |pin: Peri<'static, AnyPin>| Output::new(pin, SEG_OFF, Speed::Low);
    let segments = [
        o(p.PA10.into()), // a  D2
        o(p.PB5.into()),  // b  D4
        o(p.PA9.into()),  // c  D8
        o(p.PC7.into()),  // d  D9
        o(p.PB6.into()),  // e  D10
        o(p.PA7.into()),  // f  D11
        o(p.PA6.into()),  // g  D12
        o(p.PA0.into()),  // dp A0
    ];
    let d = |pin: Peri<'static, AnyPin>| Output::new(pin, DIGIT_OFF, Speed::Low);
    let digits = [d(p.PA1.into()), d(p.PA4.into()), d(p.PB0.into())];

    let mut store =
        match KindSStore::load(board::BoardFlash::new(p.FLASH), &board::LOG, board::SLOTS) {
            Ok(s) => s,
            Err(e) => {
                error!("state log unreadable: {}", defmt::Debug2Format(&e));
                spawner.spawn(display(segments, digits, seg::DASHES, seg::DASHES).unwrap());
                loop {
                    led.toggle();
                    Timer::after_millis(100).await;
                }
            }
        };
    let own = own_header(&mut store);
    let version = own
        .and_then(|h| ImageHeader::parse(&h).ok())
        .map(|h| h.version)
        .unwrap_or_default();
    let splash = if board::TEST_KEY {
        info!("TEST KEY: built with the public test verifying key");
        seg::TEST_KEY
    } else {
        seg::version(version)
    };
    spawner.spawn(display(segments, digits, splash, seg::version(version)).unwrap());

    let s = store.log_state();
    let on_trial = s.on_trial() && s.active() == SLOT;
    info!(
        "app {} slot {} on_trial={} attempts={} last_reason={}",
        Display2Format(&version),
        SLOT,
        on_trial,
        s.attempts,
        s.last_reason
    );
    if on_trial && SELF_CONFIRM {
        match own.map(|h| store.confirm(SLOT, &h)) {
            Some(Ok(())) => info!("confirmed slot {}", SLOT),
            _ => error!("self-confirm failed"),
        }
    }

    let wdt_ms = if cfg!(feature = "conform") {
        conform::WDT_MS
    } else if on_trial {
        board::TRIAL_WDT_MS
    } else {
        board::CONFIRMED_WDT_MS
    };
    let mut wdt = IndependentWatchdog::new(p.IWDG, wdt_ms * 1000);
    wdt.unleash();

    let mut i2c_cfg = i2c::Config::default();
    i2c_cfg.sda_pullup = true;
    i2c_cfg.scl_pullup = true;
    let mut target = I2c::new(p.I2C3, p.PA8, p.PB4, p.DMA1_CH4, p.DMA1_CH2, Irqs, i2c_cfg)
        .into_slave_multimaster(SlaveAddrConfig::basic(I2C_ADDR));
    let mut agent = Agent::new(config(), store, DefaultHooks);
    serve(&mut agent, &mut target, &mut wdt, &mut led).await
}

fn own_header(store: &mut Store) -> Option<[u8; HEADER_SIZE]> {
    let mut h = [0u8; HEADER_SIZE];
    store.read_header(SLOT, &mut h).ok().map(|_| h)
}

/// The mailbox loop. Requests are handled synchronously with the own address
/// NACKed, so a hub poll during an erase or write never meets a stretched
/// clock (instruction fetch from flash stalls while it is busy).
async fn serve(
    agent: &mut Agent<Store, DefaultHooks>,
    target: &mut I2c<'static, embassy_stm32::mode::Async, i2c::MultiMaster>,
    wdt: &mut Wdt,
    led: &mut Output<'static>,
) -> ! {
    let started = Instant::now();
    let mut mailbox = Mailbox::default();
    let mut rx = [0u8; frame_len(max_payload::I2C)];
    let mut last_request: Option<Instant> = None;
    let mut next_tick = started;
    // Feeds unless this is a trial past its window; also called right before
    // each request so an erase starts with a full watchdog period.
    let feed = |agent: &mut Agent<Store, DefaultHooks>,
                wdt: &mut IndependentWatchdog<'static, peripherals::IWDG>| {
        let s = agent.store().log_state();
        let trial = s.on_trial() && s.active() == SLOT;
        if !trial || started.elapsed().as_millis() < TRIAL_FEED_MS {
            wdt.pet();
        }
    };
    loop {
        // Ticks on time even while the hub keeps the bus busy.
        if Instant::now() >= next_tick {
            next_tick = Instant::now() + FEED_PERIOD;
            feed(agent, wdt);
            if last_request.is_some_and(|t| t.elapsed().as_millis() >= IDLE_TIMEOUT_MS as u64) {
                agent.abort();
                last_request = None;
            }
            led.toggle();
        }
        let cmd = match select(target.listen(), Timer::at(next_tick)).await {
            Either::First(Ok(cmd)) => cmd,
            Either::First(Err(e)) => {
                warn!("i2c: {}", e);
                continue;
            }
            Either::Second(()) => continue,
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
                feed(agent, wdt);
                acknowledge(false);
                let t0 = Instant::now();
                if !conform::serve(agent, &rx[..n], wdt, &mut mailbox) {
                    let reply = agent.handle(&rx[..n]);
                    let reboot = reply.as_ref().is_some_and(|r| r.reboot);
                    mailbox.respond(reply.map(|r| r.frame), reboot);
                }
                acknowledge(true);
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
                    SCB::sys_reset();
                }
            }
        }
    }
}

/// ACK (true) or NACK (false) the own address and data on I2C3.
fn acknowledge(on: bool) {
    pac::I2C3.cr1().modify(|w| w.set_ack(on));
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
        let codes = if started.elapsed() < TEST_KEY_SPLASH {
            first
        } else {
            then
        };
        for (i, code) in codes.iter().enumerate() {
            for (bit, s) in segments.iter_mut().enumerate() {
                s.set_level(if code & (1 << bit) != 0 {
                    SEG_ON
                } else {
                    SEG_OFF
                });
            }
            digits[i].set_level(DIGIT_ON);
            Timer::after(DIGIT_PERIOD).await;
            digits[i].set_level(DIGIT_OFF);
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
        agent: &mut Agent<Store, DefaultHooks>,
        request: &[u8],
        wdt: &mut Wdt,
        mailbox: &mut Mailbox,
    ) -> bool {
        let state = agent.store().log_state();
        let setup = NodeSetup {
            declared: &board::FLASH_REGIONS,
            scratch: f401_node::scratch_addr(&state, SLOT, &board::SLOTS),
        };
        let mut out = [0u8; RESPONSE_MAX];
        let flash = agent.store().flash();
        let max = max_payload::I2C;
        match handle_frame(request, max, flash, &setup, &mut out, &mut || wdt.pet()) {
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

    pub fn serve(
        _: &mut Agent<Store, DefaultHooks>,
        _: &[u8],
        _: &mut Wdt,
        _: &mut Mailbox,
    ) -> bool {
        false
    }
}
