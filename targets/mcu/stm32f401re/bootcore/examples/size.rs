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
use dagon_format::header::port_kind;
use dagon_mcu_stm32f401re::flash::F401Flash;
use dagon_mcu_stm32f401re::layout as board;
use dagon_mcu_stm32f401re::tag::checked_tag;
use dagon_mcu_stm32f401re::{trial, KindSStore, PUBKEY, TARGET_TAG, TRIAL_ATTEMPTS};
use dagon_proto::max_payload;
use dagon_proto::payload::{mode, role};

mod wiring {
    pub const BOOT_EXTENSIONS: &[dagon_events::BootExtension] = &[];
    pub type BootChain = dagon_events::Nop;
    pub const BOOT_CHAIN: BootChain = dagon_events::Nop;
}

#[dagon_stm32f401re_bootcore::__entry]
fn main() -> ! {
    if core::hint::black_box(false) {
        session()
    }
    dagon_stm32f401re_bootcore::run(wiring::BOOT_CHAIN, wiring::BOOT_EXTENSIONS)
}

/// The update session as the boot core's writer, answering requests from
/// a buffer the optimizer cannot see through, so all of it is linked.
fn session() -> ! {
    static REQUEST: [u8; 300] = [0; 300];
    // SAFETY: this branch never runs (`main`); it only links the session.
    let p = unsafe { embassy_stm32::Peripherals::steal() };
    let events: &'static mut Published<(), 4> =
        cortex_m::singleton!(: Published<(), 4> = Published::new()).unwrap();
    let now: fn() -> u64 = || 0;
    let periods = Watchdog::new(board::TRIAL_WDT_MS, board::CONFIRMED_WDT_MS);
    let chain = compose(Layer::App, events, Nop, Nop, Bounds::new(now, periods));
    let flash = F401Flash::new(p.FLASH);
    let Ok(store) = KindSStore::load(flash, &board::LOG, board::SLOTS, &[]) else {
        loop {}
    };
    let store = store.with_cores(&board::CORE_SLOTS, trial::holds());
    let cfg = Config {
        mcu_id: board::MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: checked_tag(TARGET_TAG),
        role: role::NODE,
        mode: mode::APP,
        max_payload: max_payload::I2C as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    };
    let mut agent: Agent<_, _, _, BootCore> =
        Agent::for_writer(cfg, Ed25519Small::new(PUBKEY), store, chain);
    loop {
        let request = core::hint::black_box(&REQUEST[..]);
        core::hint::black_box(agent.handle(request).map(|r| r.frame.len()));
    }
}
