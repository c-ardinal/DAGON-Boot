//! Stage0's diagnostic output on a board's `[boot]` LED (the generated
//! `BoardDiag` calls [`report`]), driven straight through the registers.

/// RCC AHB1 peripheral clock enable register (RM0368, RCC_AHB1ENR): bit n
/// clocks GPIO port n (A = 0 … E = 4, H = 7).
const RCC_AHB1ENR: *mut u32 = 0x4002_3830 as *mut u32;
/// GPIO port A; port n is 0x400 bytes further each (RM0368, memory map).
const GPIOA: u32 = 0x4002_0000;
const GPIO_STRIDE: u32 = 0x400;
/// Offsets of GPIOx_MODER and GPIOx_BSRR.
const MODER: u32 = 0x00;
const BSRR: u32 = 0x18;
/// Stage0 runs on the HSI oscillator, 16 MHz from reset (RM0368, reset
/// and clock control): CPU cycles per millisecond.
const CYCLES_PER_MS: u32 = 16_000;

/// Takes the LED on pin `pin` of GPIO port `port` (`PA5` → `(0, 5)`), lit
/// at `active_high`'s level, and shows `code` until reset for a board
/// whose halt pattern blinks every `period_ms`
/// (`dagon_core::bootcore::blink_code`).
pub fn report((port, pin): (u32, u32), active_high: bool, period_ms: u32, code: u8) -> ! {
    let base = GPIOA + port * GPIO_STRIDE;
    let moder = (base + MODER) as *mut u32;
    let bsrr = (base + BSRR) as *mut u32;
    // SAFETY: fixed STM32F401 RCC and GPIO registers of the board's LED
    // port; clocking the port and making one pin an output (MODER 01)
    // touches no other pin, and nothing else runs while stage0 halts.
    unsafe {
        RCC_AHB1ENR.write_volatile(RCC_AHB1ENR.read_volatile() | 1 << port);
        let m = moder.read_volatile() & !(0b11 << (pin * 2));
        moder.write_volatile(m | 0b01 << (pin * 2));
    }
    let set = |on: bool| {
        let bit = if on == active_high { 1 << pin } else { 1 << (pin + 16) };
        // SAFETY: BSRR sets or resets only the bits written: this pin.
        unsafe { bsrr.write_volatile(bit) };
    };
    dagon_core::bootcore::blink_code(code, period_ms, set, |ms| {
        cortex_m::asm::delay(ms.saturating_mul(CYCLES_PER_MS))
    })
}
