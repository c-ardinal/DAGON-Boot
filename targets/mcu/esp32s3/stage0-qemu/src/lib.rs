//! What both test images share: the ROM's text output and the retained
//! cell of stage0's emulator layout.
#![no_std]

extern "C" {
    fn ets_printf(fmt: *const u8, ...) -> i32;
    fn software_reset() -> !;
}

/// The retained cell's place (stage0's `qemu_layout::CELL`).
#[allow(dead_code)]
#[path = "../../stage0/src/qemu_layout.rs"]
mod layout;

/// Writes `line` (NUL-terminated) on the ROM's text output.
pub fn say(line: &core::ffi::CStr) {
    // SAFETY: a NUL-terminated string without conversions.
    unsafe { ets_printf(line.as_ptr().cast()) };
}

/// Writes `line` with the two words (`%08x`) on the ROM's text output.
pub fn say2(line: &core::ffi::CStr, a: u32, b: u32) {
    // SAFETY: a NUL-terminated format with two `%08x` conversions.
    unsafe { ets_printf(line.as_ptr().cast(), a, b) };
}

pub fn read_cell() -> [u32; 2] {
    // SAFETY: retained RTC memory words.
    layout::CELL.map(|a| unsafe { core::ptr::read_volatile(a as *const u32) })
}

pub fn write_cell(v: [u32; 2]) {
    for (a, w) in layout::CELL.into_iter().zip(v) {
        // SAFETY: as above, a word per store.
        unsafe { core::ptr::write_volatile(a as *mut u32, w) }
    }
}

/// Resets the main system (the RTC domain, and the cell, kept).
pub fn reset() -> ! {
    // SAFETY: the ROM's reset.
    unsafe { software_reset() }
}

pub fn stay() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    say(c"panic\n");
    stay()
}
