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

/// Writes `line` with four words on the ROM's text output.
pub fn say4(line: &core::ffi::CStr, w: [u32; 4]) {
    // SAFETY: a NUL-terminated format with four word conversions.
    unsafe { ets_printf(line.as_ptr().cast(), w[0], w[1], w[2], w[3]) };
}

/// The records of stage0's measuring build (`stage0/src/measure.rs`:
/// `RECORD`, the record of the start before after it, `MAGIC`, the word
/// order) on the ROM's text output, each that is there: the last start's
/// as `measure:` lines, the one before as `measure-1:`; nothing in the
/// emulator test of the normal build.
pub fn tell_measure() {
    tell_record(0x5000_1E00, c"measure:");
    tell_record(0x5000_1E80, c"measure-1:");
}

fn tell_record(at: u32, tag: &core::ffi::CStr) {
    const MAGIC: u32 = 0x314D_4744;
    // SAFETY: RTC slow memory words.
    let w = |i: u32| unsafe { core::ptr::read_volatile((at + 4 * i) as *const u32) };
    if w(0) != MAGIC {
        return;
    }
    let f = |i: u32| [w(i), w(i + 1), w(i + 2), w(i + 3)];
    let lines: [(&core::ffi::CStr, [u32; 4]); 6] = [
        (c" rom %u MHz, reset %u, path %u\n", [w(1), w(2), w(22), 0]),
        (c" cycles %u, systimer %u, rtc %u\n", [w(3), w(4), w(5), 0]),
        (
            c" verify %u cycles in %u, hash %u cycles for %u bytes\n",
            f(6),
        ),
        (c" earlier %u, changed %u, %08x..%08x\n", f(10)),
        (c" store %08x %08x %08x %08x\n", f(14)),
        (c" store %08x %08x %08x %08x\n", f(18)),
    ];
    for (line, words) in lines {
        say(tag);
        say4(line, words);
    }
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
