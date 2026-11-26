//! The X-NUCLEO-53L0A1 stacked on the NUCLEO-F401RE: the text of its
//! 4-digit 7-segment display ([`seg`]), the expander writes that show it
//! ([`shield`]) and the showing itself over a bus and a time the firmware
//! gives ([`screen`]). Tested on the host; the firmware gives the board's
//! Arduino I2C and its timer.
#![cfg_attr(not(test), no_std)]

pub mod screen;
pub mod seg;
pub mod shield;
pub mod update;
