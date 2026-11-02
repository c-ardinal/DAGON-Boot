//! Target-only pieces: read flash through XIP, and start an image.

use dagon_port::{Flash, FlashError, FlashRegion};

use crate::layout::{FLASH_SIZE, XIP_BASE};

/// Read-only flash through the XIP window (stage0 never writes).
pub struct XipFlash;

impl Flash for XipFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let off = addr.checked_sub(XIP_BASE).ok_or(FlashError::Range)?;
        if off as usize + buf.len() > FLASH_SIZE {
            return Err(FlashError::Range);
        }
        // SAFETY: the range lies inside the memory-mapped XIP window, which is
        // always readable and not written while stage0 runs.
        let src = unsafe { core::slice::from_raw_parts(addr as *const u8, buf.len()) };
        buf.copy_from_slice(src);
        Ok(())
    }

    fn erase(&mut self, _addr: u32, _len: u32) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }

    fn write(&mut self, _addr: u32, _data: &[u8]) -> Result<(), FlashError> {
        Err(FlashError::Hw)
    }
}

/// The LCD backlight (GPIO25) driven straight through the registers, so
/// every stage can show where it is, whatever the clocks and HAL state.
pub mod led {
    const RESETS_CLR: *mut u32 = (0x4000_C000 + 0x3000) as *mut u32;
    const RESET_DONE: *const u32 = (0x4000_C000 + 0x8) as *const u32;
    const IO_BANK0_RESET: u32 = 1 << 5;
    const PADS_BANK0_RESET: u32 = 1 << 8;
    const GPIO25_CTRL: *mut u32 = (0x4001_4000 + 25 * 8 + 4) as *mut u32;
    const FUNCSEL_SIO: u32 = 5;
    const SIO_OUT_SET: *mut u32 = 0xD000_0014 as *mut u32;
    const SIO_OUT_CLR: *mut u32 = 0xD000_0018 as *mut u32;
    const SIO_OE_SET: *mut u32 = 0xD000_0024 as *mut u32;
    const PIN: u32 = 1 << 25;

    /// Cycles per millisecond: ROSC at reset (about 6.5 MHz, varies a lot
    /// between chips) and the 125 MHz PLL clock after HAL init.
    pub const ROSC_MS: u32 = 6_500;
    pub const PLL_MS: u32 = 125_000;

    /// Takes GPIO25 for the backlight, output, on.
    pub fn init() {
        // SAFETY: fixed RP2040 register addresses; releasing IO/PADS bank 0
        // from reset and selecting SIO for GPIO25 touches nothing else.
        unsafe {
            RESETS_CLR.write_volatile(IO_BANK0_RESET | PADS_BANK0_RESET);
            let done = IO_BANK0_RESET | PADS_BANK0_RESET;
            while RESET_DONE.read_volatile() & done != done {}
            GPIO25_CTRL.write_volatile(FUNCSEL_SIO);
            SIO_OE_SET.write_volatile(PIN);
            SIO_OUT_SET.write_volatile(PIN);
        }
    }

    pub fn set(on: bool) {
        let reg = if on { SIO_OUT_SET } else { SIO_OUT_CLR };
        // SAFETY: SIO set/clear registers only affect GPIO25 here.
        unsafe { reg.write_volatile(PIN) };
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
        code2(n, 0)
    }

    /// Like [`code`] with a second group of `k` blinks after a short pause
    /// (`k = 0`: none).
    pub fn code2(n: u8, k: u8) -> ! {
        let c = cycles_per_ms();
        loop {
            set(false);
            wait(CODE_PAUSE_MS, c);
            group(n, c);
            if k > 0 {
                wait(CODE_GAP_MS, c);
                group(k, c);
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

/// Diagnostic builds: a panic blinks the code of the last step reached.
#[cfg(feature = "diag-panic")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    led::init();
    led::code(STEP.load(core::sync::atomic::Ordering::Relaxed))
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
