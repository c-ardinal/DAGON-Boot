//! The test app: runs from flash through the MMU and tells so.
#![no_std]
#![no_main]

use dagon_esp32s3_stage0_qemu::{say, stay, tell_measure};

#[no_mangle]
pub extern "C" fn app_start() -> ! {
    tell_measure();
    say(c"app: running from flash\n");
    stay()
}
