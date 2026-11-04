//! Target-only pieces: the XIP flash reader, a diagnostic LED, and starting
//! an image.

use crate::layout::XIP_BASE;
use crate::mapped::Mapped;

/// Read-only flash through the XIP window (stage0 never writes).
pub fn xip_flash() -> Mapped {
    // SAFETY: the XIP window maps the whole flash read-only at XIP_BASE and
    // nothing writes flash while stage0 runs.
    unsafe {
        Mapped::new(
            XIP_BASE as *const u8,
            XIP_BASE,
            crate::layout::MIN_FLASH_SIZE,
        )
    }
}

/// A diagnostic LED (the board names the GPIO: the boot images take its
/// definition's `[boot] led`, an app its board package's) driven straight
/// through the registers, so every stage can show where it is, whatever
/// the clocks and HAL state.
pub mod led {
    use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    const NONE: u8 = u8::MAX;
    static GPIO: AtomicU8 = AtomicU8::new(NONE);
    /// The LED lights at a high level (else at low).
    static ACTIVE_HIGH: AtomicBool = AtomicBool::new(true);

    const RESETS_CLR: *mut u32 = (0x4000_C000 + 0x3000) as *mut u32;
    const RESET_DONE: *const u32 = (0x4000_C000 + 0x8) as *const u32;
    const IO_BANK0_RESET: u32 = 1 << 5;
    const PADS_BANK0_RESET: u32 = 1 << 8;
    const IO_BANK0: u32 = 0x4001_4000;
    const FUNCSEL_SIO: u32 = 5;
    const SIO_OUT_SET: *mut u32 = 0xD000_0014 as *mut u32;
    const SIO_OUT_CLR: *mut u32 = 0xD000_0018 as *mut u32;
    const SIO_OE_SET: *mut u32 = 0xD000_0024 as *mut u32;

    fn mask() -> u32 {
        match GPIO.load(Ordering::Relaxed) {
            NONE => 0,
            g => 1 << g,
        }
    }

    /// Names the LED's GPIO (0..=29) and the level that lights it, without
    /// touching it, for the panic handler of the `diag-panic` feature.
    pub fn select(gpio: u8, active_high: bool) {
        if gpio < 30 {
            GPIO.store(gpio, Ordering::Relaxed);
            ACTIVE_HIGH.store(active_high, Ordering::Relaxed);
        }
    }

    /// The level that lights the selected LED is high.
    pub fn active_high() -> bool {
        ACTIVE_HIGH.load(Ordering::Relaxed)
    }

    /// The selected GPIO, if any.
    pub fn selected() -> Option<u8> {
        Some(GPIO.load(Ordering::Relaxed)).filter(|&g| g != NONE)
    }

    /// Cycles per millisecond: ROSC at reset (about 6.5 MHz, varies a lot
    /// between chips) and the 125 MHz PLL clock after HAL init.
    pub const ROSC_MS: u32 = 6_500;
    pub const PLL_MS: u32 = 125_000;

    /// Selects `gpio` (0..=29) as the LED lit at `active_high`'s level and
    /// takes it: output, on.
    pub fn init(gpio: u8, active_high: bool) {
        select(gpio, active_high);
        let Some(g) = selected() else { return };
        let ctrl = (IO_BANK0 + g as u32 * 8 + 4) as *mut u32;
        // SAFETY: fixed RP2040 register addresses; releasing IO/PADS bank 0
        // from reset and selecting SIO for one GPIO touches nothing else.
        unsafe {
            RESETS_CLR.write_volatile(IO_BANK0_RESET | PADS_BANK0_RESET);
            let done = IO_BANK0_RESET | PADS_BANK0_RESET;
            while RESET_DONE.read_volatile() & done != done {}
            ctrl.write_volatile(FUNCSEL_SIO);
        }
        set(true);
        // SAFETY: as above; output enable of the selected GPIO only.
        unsafe { SIO_OE_SET.write_volatile(mask()) };
    }

    /// Lights (`on`) or darkens the LED, at its polarity.
    pub fn set(on: bool) {
        let high = on == ACTIVE_HIGH.load(Ordering::Relaxed);
        let reg = if high { SIO_OUT_SET } else { SIO_OUT_CLR };
        // SAFETY: SIO set/clear registers only affect the selected GPIO
        // (none if no LED was selected).
        unsafe { reg.write_volatile(mask()) };
    }

    fn wait(ms: u32, cycles_per_ms: u32) {
        cortex_m::asm::delay(ms.saturating_mul(cycles_per_ms));
    }

    const CLK_SYS_CTRL: *const u32 = (0x4000_8000 + 0x3C) as *const u32;
    const CODE_BLINK_MS: u32 = 250;
    const CODE_PAUSE_MS: u32 = 2_000;
    const CODE_GAP_MS: u32 = 800;

    /// Cycles per millisecond for the clock clk_sys runs on now: the PLL
    /// (aux source) after HAL init, else the reset clock.
    pub fn cycles_per_ms() -> u32 {
        // SAFETY: read-only access to a clocks register.
        let aux = unsafe { CLK_SYS_CTRL.read_volatile() } & 1 != 0;
        if aux {
            PLL_MS
        } else {
            ROSC_MS
        }
    }

    /// Repeats forever: `n` blinks, then a long dark pause. Counting the
    /// blinks tells which code (see the hub README) stopped the board.
    pub fn code(n: u8) -> ! {
        codes(&[n])
    }

    /// Like [`code`] with several groups separated by short pauses.
    pub fn codes(groups: &[u8]) -> ! {
        let c = cycles_per_ms();
        loop {
            set(false);
            wait(CODE_PAUSE_MS, c);
            for (i, &n) in groups.iter().enumerate() {
                if i > 0 {
                    wait(CODE_GAP_MS, c);
                }
                group(n, c);
            }
        }
    }

    fn group(n: u8, c: u32) {
        for _ in 0..n {
            set(true);
            wait(CODE_BLINK_MS, c);
            set(false);
            wait(CODE_BLINK_MS, c);
        }
    }

    /// Blinks forever with `ms` on and off: a visible halt.
    pub fn halt(ms: u32, cycles_per_ms: u32) -> ! {
        loop {
            set(false);
            wait(ms, cycles_per_ms);
            set(true);
            wait(ms, cycles_per_ms);
        }
    }
}

/// Diagnostic step code: the last step a stage reached, shown by the
/// diagnostic panic handler as a blink count.
pub static STEP: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub fn step(n: u8) {
    STEP.store(n, core::sync::atomic::Ordering::Relaxed);
}

/// With the `diag-panic` feature (the hub app enables it in every build,
/// stage0 and the boot core in their diag builds) a panic blinks the code
/// of the last step reached on the LED named by [`led::select`] (or
/// [`led::init`]); with none named it just stops.
#[cfg(feature = "diag-panic")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    match led::selected() {
        Some(gpio) => {
            led::init(gpio, led::active_high());
            led::code(STEP.load(core::sync::atomic::Ordering::Relaxed))
        }
        None => loop {
            cortex_m::asm::nop();
        },
    }
}

/// The boot core trial cell (`dagon_core::bootcore::trial_cell`) in the
/// watchdog's SCRATCH0 and SCRATCH1: they keep their value through every
/// reset except power-on and the RUN pin (RP2040 datasheet section 4.7,
/// "Scratch registers"). The boot ROM uses SCRATCH4-7 only.
pub mod trial {
    const SCRATCH: [*mut u32; 2] = [0x4005_800C as *mut u32, 0x4005_8010 as *mut u32];

    pub fn read() -> [u32; 2] {
        // SAFETY: fixed RP2040 watchdog scratch registers; reading has no
        // side effect.
        SCRATCH.map(|r| unsafe { r.read_volatile() })
    }

    pub fn write(cell: [u32; 2]) {
        for (r, v) in SCRATCH.into_iter().zip(cell) {
            // SAFETY: as above; nothing else in DAGON uses SCRATCH0-1.
            unsafe { r.write_volatile(v) };
        }
    }

    const CTRL: *mut u32 = 0x4005_8000 as *mut u32;
    const LOAD: *mut u32 = 0x4005_8004 as *mut u32;
    const TICK: *mut u32 = 0x4005_802C as *mut u32;
    const CTRL_ENABLE: u32 = 1 << 30;
    const TICK_ENABLE: u32 = 1 << 9;
    /// clk_ref cycles per tick: the ring oscillator's nominal 6.5 MHz at
    /// reset, so a tick is about 1 µs (it varies with the chip).
    const TICK_CYCLES: u32 = 6;
    const LOAD_MAX: u32 = 0x00FF_FFFF;

    /// Starts the watchdog at its longest period (about 8 s on a nominal
    /// ring oscillator: the counter runs down twice per tick, erratum
    /// RP2040-E1) before stage0 jumps to a core on trial, so a core that
    /// hangs before it takes the watchdog over is reset and its start
    /// counted.
    pub fn arm_watchdog() {
        // SAFETY: fixed RP2040 watchdog registers; the started core takes
        // the watchdog over (embassy-rp `Watchdog::start`).
        unsafe {
            TICK.write_volatile(TICK_ENABLE | TICK_CYCLES);
            LOAD.write_volatile(LOAD_MAX);
            CTRL.write_volatile(CTRL_ENABLE);
        }
    }
}

/// Starts the image whose vector table is at XIP address `body`.
/// Interrupts must not be in use (NVIC enables and pendings are cleared).
pub fn jump(body: u32) -> ! {
    let vtor = body;
    // SAFETY: `body` holds a verified image linked for this address; the
    // caller has no live interrupt sources, so switching VTOR and the stack
    // pointer here hands the core over cleanly.
    unsafe {
        let nvic = &*cortex_m::peripheral::NVIC::PTR;
        nvic.icer[0].write(u32::MAX);
        nvic.icpr[0].write(u32::MAX);
        (*cortex_m::peripheral::SCB::PTR).vtor.write(vtor);
        cortex_m::asm::bootload(vtor as *const u32)
    }
}
