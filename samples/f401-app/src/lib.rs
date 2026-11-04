//! The X-NUCLEO-53L0A1 stacked on the NUCLEO-F401RE: the text of its
//! 4-digit 7-segment display ([`seg`]) and the expander writes that show it
//! ([`shield`]). Pure, tested on the host; the firmware drives them on the
//! board's Arduino I2C.
#![cfg_attr(not(test), no_std)]

pub mod seg;
pub mod shield;
pub mod update;
