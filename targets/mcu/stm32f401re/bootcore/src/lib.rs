//! STM32F401RE boot core: sets its clock up as the definitions choose for
//! the node (the MCU's `[[bootcore.clock]]` setup for its board's
//! `[clock]`), arms the watchdog with the trial period (fed at every step
//! of a signature check), reads
//! the state log, picks the APP slot (trial of the pending slot, rollback
//! after the attempts run out, otherwise the confirmed slot), sets the
//! watchdog to the confirmed period when the slot is not on trial, and
//! jumps. With no startable slot it shows the board's halt pattern on its
//! boot LED and stays here.
//!
//! Before the log decides, the boot core opens its I2C target link (the
//! board's DAGON bus, `link.rs` from build.rs) and enters update mode when
//! the app handed over (an UPDATE_STAY in the log, consumed now) or a STAY
//! arrives within the listen window of the node's MCU definition. In
//! update mode the shared session (`dagon_agent::Agent` as the boot core's
//! writer, with the same ticking verifier) answers the host until REBOOT
//! or 60 s without a request, then the node restarts; nothing outside
//! update mode erases or writes an image. A node without the link opens
//! no window.
//!
//! Images are verified with `dagon_mcu_stm32f401re::PUBKEY` (`DAGON_PUBKEY`
//! at build time; the public test key only with `DAGON_ALLOW_TEST_KEY=1`).
//!
//! A node's boot core is the boot crate `dagon gen` writes into
//! `boot/<id>/` ([`boot!`]): it runs the extensions its
//! `boot_wiring.rs` lists (each declared trusted in dagon.toml) on every
//! boot event, each call timed against its `max_run_us`, and takes the
//! board's `[boot] led` and `halt` and the node's tag from that folder
//! (`DAGON_BOOT_CRATE`, `DAGON_TARGET_TAG`); the build fails without
//! them. It accepts only images tagged with its node's tag.
//!
//! Boot core A/B (`dagon_core::bootcore::settle_log` over the shared state
//! log): started by stage0 as the pending core, it confirms itself only
//! after its own check (the decision starts an app, so that app's header
//! verified under this core's key); otherwise it resets, so stage0 counts the start
//! and falls back once the starts run out. Started while another core is
//! pending, it records the rollback. Built for boot core slot A, or B with
//! `slot-b`.
#![no_std]
// The build that measures the boot decision stops after it: the code that
// starts the app is unused there.
#![cfg_attr(feature = "decide-time", allow(unreachable_code, unused_variables, unused_mut))]

use core::panic::PanicInfo;

use cortex_m::peripheral::{DWT, NVIC, SCB};
use dagon_agent::update_mode::{listen, serve, Stay};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, Config};
use dagon_boot::BootRun;
use dagon_core::boot::{BootConfig, Decision};
use dagon_core::bootcore::{settle_log, Cell, MustReset, Settled, CLEARED};
use dagon_core::log::Log;
use dagon_ed25519::Ed25519Small;
use dagon_events::{
    compose, compose_boot, BootExtension, Bounds, Clock, Extension, Layer, Nop, Published, Watchdog,
};
use dagon_format::header::port_kind;
#[cfg(any(feature = "listen-time", feature = "decide-time"))]
use dagon_mcu_stm32f401re::boot_time;
use dagon_mcu_stm32f401re::boot_link::I2cPort;
use dagon_mcu_stm32f401re::clock::Setup;
use dagon_mcu_stm32f401re::flash::F401Flash;
use dagon_mcu_stm32f401re::layout as board;
use dagon_mcu_stm32f401re::tag::checked_tag;
use dagon_mcu_stm32f401re::{iwdg, trial, KindSStore, ERASE_REGIONS, LINKED_SLOT, TRIAL_ATTEMPTS};
use dagon_mcu_stm32f401re::{PUBKEY, TARGET_TAG};
use dagon_port::BootSelectS;
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};
#[allow(unused_imports)]
use embassy_stm32::gpio::{Level, Output, Speed};
use static_cell::StaticCell;

#[doc(hidden)]
pub use cortex_m_rt::entry as __entry;

/// The boot core's `main` for the node whose generated `boot_wiring.rs`
/// is the module `$wiring`: its extension chain and their time limits,
/// and the node's I2C target address from the topology (`LINK_ADDR`).
#[macro_export]
macro_rules! boot {
    ($wiring:ident) => {
        #[$crate::__entry]
        fn main() -> ! {
            $crate::run(
                $wiring::BOOT_CHAIN,
                $wiring::BOOT_EXTENSIONS,
                $wiring::LINK_ADDR,
            )
        }
    };
}

/// Wakes the core from the link's wait (`boot_link`); nothing to do.
#[cortex_m_rt::exception]
fn SysTick() {}

/// The longest the listen window and update mode sleep before they feed
/// the watchdog again: a quarter of the period it runs with.
const FEED_EVERY_MS: u32 = board::TRIAL_WDT_MS / 4;

/// The board's `[boot]` values (build.rs, from the node's boot folder).
#[allow(dead_code)]
mod board_def {
    include!(concat!(env!("OUT_DIR"), "/board.rs"));
}
/// The clock setup the definitions choose for the node (build.rs).
#[allow(dead_code)]
mod clock_def {
    include!(concat!(env!("OUT_DIR"), "/clock.rs"));
}

const CLOCK: Setup = dagon_mcu_stm32f401re::clock_setup!(clock_def);

/// The core clock, Hz.
const SYSCLK_HZ: u32 = CLOCK.out_hz;
const _: () = assert!(
    SYSCLK_HZ >= 1_000_000,
    "the boot core counts microseconds in whole core cycles"
);

/// The link the boot core receives on and its listen window (build.rs).
#[macro_use]
mod link {
    include!(concat!(env!("OUT_DIR"), "/link.rs"));
}

#[allow(dead_code, unused_macros)]
#[macro_use]
mod pins {
    include!(concat!(env!("OUT_DIR"), "/pins.rs"));
}

/// This core's signature verifier, under the key built into it: stage0's
/// small Ed25519, so the boot core has room for the update session.
/// It feeds the IWDG at every step of a check.
const VERIFIER: Ed25519Small<fn()> = Ed25519Small::with_tick(PUBKEY, iwdg::feed);

const CONFIG: BootConfig = BootConfig {
    mcu_id: board::MCU_ID,
    target_tag: checked_tag(TARGET_TAG),
    max_attempts: dagon_mcu_stm32f401re::TRIAL_ATTEMPTS,
};

/// Half period of the halt blink in CPU cycles; `None`: the LED stays off.
#[allow(dead_code)]
const HALT_BLINK_CYCLES: Option<u32> = match board_def::HALT_BLINK_PERIOD_MS {
    Some(ms) => Some(SYSCLK_HZ / 1000 * (ms / 2)),
    None => None,
};

/// CPU cycles per microsecond.
const CYCLES_PER_US: u32 = SYSCLK_HZ / 1_000_000;

/// Microseconds from the cycle counter (DWT CYCCNT at `SYSCLK_HZ`), read
/// at least once per 32-bit wrap (51 s at 84 MHz). Each read converts the
/// cycles since the last one with a 32-bit division (one instruction here;
/// a 64-bit one would link a software divider into the boot core).
struct Cycles {
    last: u32,
    rem: u32,
    us: u64,
}

impl Clock for Cycles {
    fn now_us(&mut self) -> u64 {
        let now = DWT::cycle_count();
        let d = now.wrapping_sub(self.last).saturating_add(self.rem);
        self.last = now;
        self.us += u64::from(d / CYCLES_PER_US);
        self.rem = d % CYCLES_PER_US;
        self.us
    }
}

/// One boot with `chain` (the node's boot extensions, in order; their
/// time limits and observed events in `limits`); the link answers at
/// `link_addr` (`None`: the board's default address).
pub fn run<E: Extension>(chain: E, limits: &'static [BootExtension], link_addr: Option<u8>) -> ! {
    let p = embassy_stm32::init(CLOCK.hal_config());
    // Before the log is read and before any extension runs.
    iwdg::start(board::TRIAL_WDT_MS);
    // SAFETY: the boot core takes the core peripherals nowhere else; the
    // cycle counter is enabled once here and only read afterwards.
    let mut cp = unsafe { cortex_m::Peripherals::steal() };
    cp.DCB.enable_trace();
    cp.DWT.enable_cycle_counter();
    let mut clock = Cycles {
        last: DWT::cycle_count(),
        rem: 0,
        us: 0,
    };
    let mut flash = F401Flash::new(p.FLASH);
    // The link first: the listen window counts from the start.
    if let Some(mut port) = open_link!(p, link_addr, CYCLES_PER_US) {
        let handed_over = Log::load(&mut flash, &board::LOG)
            .and_then(|mut l| l.take_stay(&mut flash))
            == Ok(true);
        if handed_over {
            update_mode(port, flash, &mut clock, None)
        }
        let window = link::LISTEN_WINDOW_MS;
        let feed = &mut iwdg::feed;
        if let Some(stay) = listen(&mut port, &mut clock, window, FEED_EVERY_MS, feed) {
            update_mode(port, flash, &mut clock, Some(stay))
        }
        #[cfg(feature = "listen-time")]
        boot_time::hold(&boot_time::DAGON_LISTEN_US, clock.now_us(), iwdg::feed);
    }
    let mut chain = compose_boot(chain, clock, limits);
    #[cfg(feature = "decide-time")]
    let deciding = DWT::cycle_count();
    let mut boot = BootRun::start(
        &mut chain,
        &mut flash,
        &board::LOG,
        &board::SLOTS,
        &CONFIG,
        &VERIFIER,
        board::TRIAL_WDT_MS,
    );
    let decision = boot.decision();
    // At most 51 s at 84 MHz between the two reads (one 32-bit wrap).
    #[cfg(feature = "decide-time")]
    boot_time::hold(
        &boot_time::DAGON_DECIDE_US,
        u64::from(DWT::cycle_count().wrapping_sub(deciding) / CYCLES_PER_US),
        iwdg::feed,
    );
    // The decision verified the app it starts under this core's key.
    let healthy = !cfg!(feature = "no-confirm") && matches!(decision, Decision::Boot { .. });
    let settled = settle_log(
        &mut flash,
        &board::LOG,
        LINKED_SLOT,
        Cell {
            value: trial::read(),
            max: TRIAL_ATTEMPTS,
            holds: trial::holds(),
        },
        healthy,
    );
    match settled {
        Ok(Settled::Nothing) => {}
        Ok(Settled::Confirmed | Settled::RolledBack) => trial::write(CLEARED),
        // On trial and not fit to run the app: stage0 counts this start
        // (a system reset keeps the trial cell).
        Err(MustReset) => SCB::sys_reset(),
    }
    if let Decision::Boot { entry, trial, .. } = decision {
        if !trial {
            iwdg::set(board::CONFIRMED_WDT_MS);
        }
        boot.before_jump(&mut chain);
        Jump.boot(entry)
    }
    boot.halt(&mut chain);
    #[cfg(boot_led)]
    {
        let off = if board_def::BOOT_LED_ACTIVE_HIGH {
            Level::Low
        } else {
            Level::High
        };
        let mut led = Output::new(take!(p.boot_led), off, Speed::Low);
        if let Some(cycles) = HALT_BLINK_CYCLES {
            loop {
                led.toggle();
                cortex_m::asm::delay(cycles);
            }
        }
    }
    loop {
        cortex_m::asm::wfi();
    }
}

/// Update mode: the boot core's session answers the host over `port`
/// (first the window's `stay`, if any) until REBOOT or 60 s without a
/// request; then the node restarts. The store is opened only here, and
/// the session checks images with [`VERIFIER`], which feeds the watchdog.
fn update_mode(
    mut port: I2cPort,
    flash: F401Flash<'static>,
    clock: &mut Cycles,
    stay: Option<Stay>,
) -> ! {
    static EVENTS: StaticCell<Published<(), 4>> = StaticCell::new();
    let events = EVENTS.uninit().write(Published::new());
    if let Ok(store) = KindSStore::load(flash, &board::LOG, board::SLOTS, &ERASE_REGIONS) {
        let store = store.with_cores(&board::CORE_SLOTS, trial::holds());
        let now: fn() -> u64 = || 0;
        let periods = Watchdog::new(board::TRIAL_WDT_MS, board::CONFIRMED_WDT_MS);
        let chain = compose(Layer::App, events, Nop, Nop, Bounds::new(now, periods));
        let cfg = Config {
            mcu_id: board::MCU_ID,
            port_kind: port_kind::SELF_BOOT,
            target_tag: checked_tag(TARGET_TAG),
            role: role::NODE,
            mode: mode::UPDATE,
            max_payload: max_payload::I2C as u16,
            trial_attempts: TRIAL_ATTEMPTS,
        };
        let mut agent: Agent<_, _, _, BootCore> = Agent::for_writer(cfg, VERIFIER, store, chain);
        let first = stay.as_ref();
        serve(
            &mut port,
            &mut agent,
            clock,
            FEED_EVERY_MS,
            &mut iwdg::feed,
            first,
        );
    }
    SCB::sys_reset()
}

struct Jump;

impl BootSelectS for Jump {
    fn boot(&mut self, entry: u32) -> ! {
        // SAFETY: the boot core enables no interrupts, so clearing NVIC
        // enables and pending bits leaves nothing to fire mid-handover.
        // `entry` is the body of a slot whose header signature and SHA-256
        // were just verified, so it starts with a vector table
        // (initial MSP, reset vector) at a 512-byte aligned address, as VTOR
        // requires on this part.
        unsafe {
            let nvic = &*NVIC::PTR;
            for i in 0..8 {
                nvic.icer[i].write(u32::MAX);
                nvic.icpr[i].write(u32::MAX);
            }
            (*SCB::PTR).vtor.write(entry);
            cortex_m::asm::bootload(entry as *const u32)
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        cortex_m::asm::nop();
    }
}
