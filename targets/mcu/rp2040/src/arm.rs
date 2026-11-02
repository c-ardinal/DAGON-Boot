//! Target-only pieces: the XIP flash reader, a diagnostic LED, and starting
//! an image.

use crate::layout::XIP_BASE;
use crate::mapped::Mapped;

/// Read-only flash through the XIP window (stage0 never writes).
pub fn xip_flash() -> Mapped {
    // SAFETY: the XIP window maps the whole flash read-only at XIP_BASE and
    // nothing writes flash while stage0 runs.
    unsafe { Mapped::new(XIP_BASE as *const u8, XIP_BASE, crate::layout::MIN_FLASH_SIZE) }
}

/// A diagnostic LED (the board package names the GPIO, e.g. a display
/// backlight) driven straight through the registers, so every stage can
/// show where it is, whatever the clocks and HAL state.
pub mod led {
    use core::sync::atomic::{AtomicU8, Ordering};

    const NONE: u8 = u8::MAX;
    static GPIO: AtomicU8 = AtomicU8::new(NONE);

    /// The boot images' (stage0, boot core) LED: `DAGON_DIAG_LED` at build
    /// time (a decimal GPIO number), else GPIO25, the backlight of the hub
    /// board (Waveshare RP2040-Touch-LCD-1.28).
    pub const BOOT_GPIO: u8 = match option_env!("DAGON_DIAG_LED") {
        Some(v) => parse_gpio(v.as_bytes()),
        None => 25,
    };

    const fn parse_gpio(v: &[u8]) -> u8 {
        let mut n: u32 = 0;
        let mut i = 0;
        while i < v.len() {
            assert!(v[i].is_ascii_digit(), "DAGON_DIAG_LED: a decimal GPIO number");
            n = n * 10 + (v[i] - b'0') as u32;
            i += 1;
        }
        assert!(!v.is_empty() && n < 30, "DAGON_DIAG_LED: GPIO 0..=29");
        n as u8
    }

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

    /// Names the LED's GPIO (0..=29) without touching it, for the panic
    /// handler of the `diag-panic` feature.
    pub fn select(gpio: u8) {
        if gpio < 30 {
            GPIO.store(gpio, Ordering::Relaxed);
        }
    }

    /// The selected GPIO, if any.
    pub fn selected() -> Option<u8> {
        Some(GPIO.load(Ordering::Relaxed)).filter(|&g| g != NONE)
    }

    /// Cycles per millisecond: ROSC at reset (about 6.5 MHz, varies a lot
    /// between chips) and the 125 MHz PLL clock after HAL init.
    pub const ROSC_MS: u32 = 6_500;
    pub const PLL_MS: u32 = 125_000;

    /// Selects `gpio` (0..=29) as the LED and takes it: output, on.
    pub fn init(gpio: u8) {
        select(gpio);
        let Some(g) = selected() else { return };
        let ctrl = (IO_BANK0 + g as u32 * 8 + 4) as *mut u32;
        // SAFETY: fixed RP2040 register addresses; releasing IO/PADS bank 0
        // from reset and selecting SIO for one GPIO touches nothing else.
        unsafe {
            RESETS_CLR.write_volatile(IO_BANK0_RESET | PADS_BANK0_RESET);
            let done = IO_BANK0_RESET | PADS_BANK0_RESET;
            while RESET_DONE.read_volatile() & done != done {}
            ctrl.write_volatile(FUNCSEL_SIO);
            SIO_OE_SET.write_volatile(mask());
            SIO_OUT_SET.write_volatile(mask());
        }
    }

    pub fn set(on: bool) {
        let reg = if on { SIO_OUT_SET } else { SIO_OUT_CLR };
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
            led::init(gpio);
            led::code(STEP.load(core::sync::atomic::Ordering::Relaxed))
        }
        None => loop {
            cortex_m::asm::nop();
        },
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
