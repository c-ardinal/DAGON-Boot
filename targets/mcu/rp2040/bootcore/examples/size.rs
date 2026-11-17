//! The boot core of a node without extensions: template/boot.rs with the
//! wiring `dagon gen` writes for no extension. It links the update session
//! the boot core serves in update and recovery mode (dagon-agent as the
//! boot core's writer, over this MCU's slot store and verifier).
//! scripts/bootcore_size.py builds it and checks that it fits the boot
//! core slot.
#![no_std]
#![no_main]

mod wiring {
    pub const BOOT_EXTENSIONS: &[dagon_events::BootExtension] = &[];
    pub type BootChain = dagon_events::Nop;
    pub const BOOT_CHAIN: BootChain = dagon_events::Nop;
    /// No address from a topology: the board's default.
    pub const LINK_ADDR: Option<u8> = None;
}

dagon_rp2040_bootcore::boot!(wiring);
