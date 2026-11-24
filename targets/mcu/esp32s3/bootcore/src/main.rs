//! ESP32-S3 boot core under DAGON's stage0: the image in the factory
//! partition that stage0 checks and starts at every reset (the boot core
//! slot of `qemu_layout`), with the app handed over by reset
//! (`dagon_core::bootcore::start_handed`). It runs from RAM, on the ROM's
//! flash driver and UART0 as the ROM left them, and the RTC watchdog
//! armed for its own run (the trial period, fed while it waits).
//!
//! At its start (`dagon_node::handed::begin`) it settles its own
//! trial, then
//!
//! - serves recovery mode when stage0's decision started no app (the
//!   result stage0 left in the retained cell),
//! - serves update mode when the app handed over (an UPDATE_STAY in the app
//!   log) or a STAY arrives on UART0 within the definition's listen window,
//! - and otherwise asks stage0 for the app (START_APP in the cell) and
//!   resets.
//!
//! The session is dagon-agent's with the boot core writer over the shared
//! state-log store (`dagon_node::log_store`): it writes the APP slots
//! (detached headers) and sets one pending, checking each image with
//! stage0's verifier under the key built in. After REBOOT, or update mode's
//! 60 s without a request, the core resets without a request, so the next
//! start opens the window again and then asks for the app.
#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

#[cfg(not(feature = "qemu-layout"))]
compile_error!(
    "the ESP32-S3 definition gives no stage0 layout yet: \
     build with the `qemu-layout` feature for the emulator test"
);

mod clock;
#[path = "../../stage0/src/flash.rs"]
mod flash;
#[path = "../../stage0/src/qemu_layout.rs"]
#[allow(dead_code)]
mod qemu_layout;
#[path = "../../stage0/src/rom.rs"]
#[allow(dead_code)]
mod rom;
mod uart;
#[path = "../../stage0/src/wdt.rs"]
#[allow(dead_code)]
mod wdt;

use core::mem::MaybeUninit;

use clock::Cycles;
use dagon_agent::update_mode::{serve_in, Entry};
use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, Config};
use dagon_core::boot::{rebuild, BootConfig, SlotLayout};
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::port_kind;
use dagon_mcu_esp32s3::{MCU_ID, PUBKEY, TRIAL_ATTEMPTS, TRIAL_WDT_MS};
use dagon_node::handed::{begin, CellIo, Logs, Next, Window};
use dagon_node::log_store::{LogMcu, LogStore};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::max_payload;
use dagon_proto::payload::role;
use flash::RomFlash;
use qemu_layout as layout;
use uart::Uart0;

include!(concat!(env!("OUT_DIR"), "/window.rs"));

/// The boot core slot it runs from (the only one: the factory partition).
const RUNNING: u8 = 0;
/// The longest the core waits before it feeds the watchdog again.
const FEED_EVERY_MS: u32 = TRIAL_WDT_MS / 4;

/// The node's tag (`DAGON_TARGET_TAG` at build time); the build fails
/// without a valid one.
const TAG: [u8; 16] = match dagon_mcu_esp32s3::TARGET_TAG_OVERRIDE {
    Some(t) => dagon_mcu_common::tag::checked_tag(t),
    None => panic!("DAGON_TARGET_TAG is unset: the boot core takes only the node's images"),
};

/// This core's verifier, under the key built into it: stage0's small
/// Ed25519, feeding the watchdog at every step of a check.
const VERIFIER: Ed25519Small<fn()> = Ed25519Small::with_tick(PUBKEY, wdt::feed as fn());

/// The ESP32-S3 under stage0 as the shared log store takes it.
enum Esp32s3 {}

impl LogMcu for Esp32s3 {
    const WRITE_UNIT: usize = dagon_mcu_esp32s3::WRITE_UNIT as usize;
    const ERASED: u8 = dagon_mcu_esp32s3::ERASED_VALUE;
    const TRIAL_ATTEMPTS: u8 = TRIAL_ATTEMPTS;
    const SLOT_LAYOUTS: &'static [dagon_port::timeout::SlotLayout] =
        &dagon_mcu_esp32s3::SLOT_LAYOUTS;
    // The definition gives stage0 no region yet.
    const STAGE0: Option<(u32, u32)> = None;
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;
type Session = Agent<LogStore<Fed, Esp32s3>, Chain, Ed25519Small<fn()>, BootCore>;

/// The ROM's flash, feeding the watchdog before every operation and every
/// erased sector: a slot's erase (up to 400 ms a sector) and the session's
/// read-back hash of a large image outlast the trial period.
struct Fed(RomFlash);

const SECTOR: u32 = 0x1000;

impl Flash for Fed {
    fn geometry(&self) -> &[FlashRegion] {
        self.0.geometry()
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        wdt::feed();
        self.0.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        if !len.is_multiple_of(SECTOR) {
            return Err(FlashError::Align);
        }
        let end = addr.checked_add(len).ok_or(FlashError::Range)?;
        for s in (addr..end).step_by(SECTOR as usize) {
            wdt::feed();
            self.0.erase(s, SECTOR)?;
        }
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        wdt::feed();
        self.0.write(addr, data)
    }
    fn sha256(&mut self, addr: u32, len: u32) -> Result<[u8; 32], FlashError> {
        wdt::feed();
        self.0.sha256(addr, len)
    }
}

struct NoIrq;
critical_section::set_impl!(NoIrq);

// SAFETY: one core, and the boot core takes no interrupt: nothing runs
// between acquire and release but the caller.
unsafe impl critical_section::Impl for NoIrq {
    unsafe fn acquire() {}
    unsafe fn release(_: ()) {}
}

extern "C" {
    static mut _bss_start: u32;
    static mut _bss_end: u32;
}

fn read_cell() -> [u32; 2] {
    // SAFETY: retained RTC memory words, read as words.
    layout::CELL.map(|a| unsafe { core::ptr::read_volatile(a as *const u32) })
}

fn write_cell(i: usize, w: u32) {
    // SAFETY: as above; one word per store.
    unsafe { core::ptr::write_volatile(layout::CELL[i] as *mut u32, w) }
}

fn reset() -> ! {
    // SAFETY: the ROM's reset of the main system (the cell kept).
    unsafe { rom::software_reset() }
}

/// Stage0's call of the image's entry point.
#[no_mangle]
#[link_section = ".text.core_start"]
pub extern "C" fn core_start() -> ! {
    // SAFETY: nothing has used .bss yet; zeroed a word at a time.
    unsafe {
        let mut p = &raw mut _bss_start;
        while p < &raw mut _bss_end {
            core::ptr::write_volatile(p, 0);
            p = p.add(1);
        }
    }
    // The core's own run is timed like an image on trial.
    wdt::arm(TRIAL_WDT_MS, layout::SLOW_HZ);
    flash::attach();
    let mut f = RomFlash;
    let mut clock = Cycles::new();
    let mut port = Uart0::new(clock.clone());
    let mut feed = wdt::feed;
    let next = begin(
        &mut f,
        Logs {
            core: &layout::CORE_LOG,
            app: &layout::APP_LOG,
            running: (RUNNING, layout::CORE_PLACES[RUNNING as usize].header),
            max: TRIAL_ATTEMPTS,
        },
        CellIo {
            read: read_cell,
            write: write_cell,
        },
        Window {
            port: &mut port,
            clock: &mut clock,
            window_ms: LISTEN_WINDOW_MS,
            feed_every_ms: FEED_EVERY_MS,
            feed: &mut feed,
        },
        |f: &mut RomFlash| {
            let cfg = BootConfig {
                mcu_id: MCU_ID,
                target_tag: TAG,
                max_attempts: TRIAL_ATTEMPTS,
            };
            let slots = SlotLayout {
                slots: &layout::APP_PLACES,
            };
            // No image passing: the log stays lost and stage0's result
            // brings the core back in recovery mode.
            let _ = rebuild(f, &layout::APP_LOG, &slots, &cfg, &VERIFIER);
        },
    );
    let (entry, stay) = match next {
        Next::Serve(entry, stay) => (entry, stay),
        Next::AskForApp | Next::Reset => reset(),
    };
    let Some(agent) = session(Fed(f), entry) else {
        reset()
    };
    serve_in(
        &mut port,
        agent,
        &mut clock,
        FEED_EVERY_MS,
        &mut feed,
        stay.as_ref(),
        entry,
    );
    reset()
}

/// The boot core's session in `entry`'s mode over the app log and slots,
/// the log created empty when neither face is valid (so recovery mode can
/// write the app); `None` on a flash error.
fn session(flash: Fed, entry: Entry) -> Option<&'static mut Session> {
    static mut EVENTS: MaybeUninit<Published<(), 4>> = MaybeUninit::uninit();
    static mut AGENT: MaybeUninit<Session> = MaybeUninit::uninit();
    let store = LogStore::load_or_create(
        flash,
        &layout::APP_LOG,
        SlotLayout {
            slots: &layout::APP_PLACES,
        },
        &dagon_mcu_esp32s3::ERASE_REGIONS,
        // No app runs while the boot core serves.
        dagon_format::statelog::slot::NONE,
    )
    .ok()?;
    let (events, slot) = (&raw mut EVENTS, &raw mut AGENT);
    // SAFETY: called once, before anything else takes these statics.
    let events: &'static mut Published<(), 4> = unsafe { (*events).write(Published::new()) };
    let now: fn() -> u64 = || 0;
    let periods = Watchdog::new(TRIAL_WDT_MS, dagon_mcu_esp32s3::CONFIRMED_WDT_MS);
    let chain = compose(Layer::App, events, Nop, Nop, Bounds::new(now, periods));
    let cfg = Config {
        mcu_id: MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: TAG,
        role: role::NODE,
        mode: entry.mode(),
        max_payload: max_payload::UART as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    };
    let agent = Agent::for_writer(cfg, VERIFIER, store, chain);
    // SAFETY: as above.
    Some(unsafe { (*slot).write(agent) })
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    reset()
}
