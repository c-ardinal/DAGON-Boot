//! The boot core of a node without extensions: template/boot.rs with the
//! wiring `dagon gen` writes for no extension, together with the update
//! session the boot core serves in update and recovery mode (dagon-agent
//! as the boot core's writer, over this MCU's slot store and verifier).
//! scripts/bootcore_size.py builds it and checks that the two fit the boot
//! core slot together.
#![no_std]
#![no_main]

use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, Config};
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Layer, Nop, Published, Watchdog};
use dagon_format::header::{mcu_id, port_kind};
use dagon_mcu_common::guard::Watch;
use dagon_mcu_common::shared::{driver, FlashDriver, SharedFlash};
use dagon_mcu_rp2040::flash::{RpFlash, REGIONS};
use dagon_mcu_rp2040::layout::watchdog_ms;
use dagon_mcu_rp2040::store::AppStore;
use dagon_mcu_rp2040::tag::checked_tag;
use dagon_mcu_rp2040::{PUBKEY, TARGET_TAG, TRIAL_ATTEMPTS};
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};

mod wiring {
    pub const BOOT_EXTENSIONS: &[dagon_events::BootExtension] = &[];
    pub type BootChain = dagon_events::Nop;
    pub const BOOT_CHAIN: BootChain = dagon_events::Nop;
}

#[dagon_rp2040_bootcore::__entry]
fn main() -> ! {
    if core::hint::black_box(false) {
        session()
    }
    dagon_rp2040_bootcore::run(wiring::BOOT_CHAIN, wiring::BOOT_EXTENSIONS)
}

/// The watchdog reports a long erase gives; the boot core's own watchdog
/// handling comes with update mode.
struct NoReports;

impl Watch for NoReports {
    fn busy(&self, _: usize) {}
    fn busy_for(&self, _: usize, _: u64) {}
    fn idle(&self, _: usize) {}
    fn feed(&self) {}
    fn set_trial(&self, _: bool) {}
    fn reboot(&self) -> ! {
        cortex_m::peripheral::SCB::sys_reset()
    }
}

/// The update session as the boot core's writer, answering requests from
/// a buffer the optimizer cannot see through, so all of it is linked.
fn session() -> ! {
    static REQUEST: [u8; 300] = [0; 300];
    static REPORTS: NoReports = NoReports;
    // SAFETY: this branch never runs (`main`); it only links the session.
    let p = unsafe { embassy_rp::Peripherals::steal() };
    let events: &'static mut Published<(), 4> =
        cortex_m::singleton!(: Published<(), 4> = Published::new()).unwrap();
    let chip: &'static mut FlashDriver<RpFlash> =
        cortex_m::singleton!(: FlashDriver<RpFlash> = driver(RpFlash::new(p.FLASH))).unwrap();
    let now: fn() -> u64 = || 0;
    let periods = Watchdog::new(watchdog_ms(true), watchdog_ms(false));
    let chain = compose(Layer::App, events, Nop, Nop, Bounds::new(now, periods));
    let store = AppStore::new(SharedFlash::new(chip, &REGIONS), &REPORTS, 0, &[]);
    let cfg = Config {
        mcu_id: mcu_id::RP2040,
        port_kind: port_kind::SELF_BOOT,
        target_tag: checked_tag(TARGET_TAG),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::USB as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    };
    let mut agent: Agent<_, _, _, BootCore> =
        Agent::for_writer(cfg, Ed25519Small::new(PUBKEY), store, chain);
    loop {
        let request = core::hint::black_box(&REQUEST[..]);
        core::hint::black_box(agent.handle(request).map(|r| r.frame.len()));
    }
}
