//! The test boot core: tells the cell it was started with, then hands off
//! as a boot core does (`dagon_core::bootcore::hand_off`): on a result it
//! clears the cell and stays (recovery mode), otherwise it writes
//! START_APP and resets.
#![no_std]
#![no_main]

use dagon_core::bootcore::{hand_off, HandOff, CLEARED, START_APP};
use dagon_esp32s3_stage0_qemu::{read_cell, reset, say, say2, stay, tell_measure, write_cell};

#[no_mangle]
pub extern "C" fn core_start() -> ! {
    tell_measure();
    let cell = read_cell();
    say2(c"core: started, cell %08x %08x\n", cell[0], cell[1]);
    match hand_off(cell) {
        HandOff::Stay(r) => {
            write_cell(CLEARED);
            say2(c"core: recovery mode, refusal %u\n", r as u32, 0);
            stay()
        }
        HandOff::Request => {
            write_cell(START_APP);
            say(c"core: asks for the app\n");
            reset()
        }
    }
}
