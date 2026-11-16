//! The boot core of a node without extensions: template/boot.rs with the
//! wiring `dagon gen` writes for no extension. scripts/bootcore_size.py
//! builds it and checks it fits the boot core slot.
#![no_std]
#![no_main]

mod wiring {
    pub const BOOT_EXTENSIONS: &[dagon_events::BootExtension] = &[];
    pub type BootChain = dagon_events::Nop;
    pub const BOOT_CHAIN: BootChain = dagon_events::Nop;
}

dagon_stm32f401re_bootcore::boot!(wiring);
