//! The entry point of an image whose body starts with an Armv6-M or
//! Armv7-M vector table (the stack pointer's initial value, then the reset
//! vector), as stage0 reads it before it starts an app
//! (`dagon_core::bootcore::Apps::entry`).

/// The reset vector in `head`, the body's first 8 bytes, without its
/// Thumb bit: the address the core starts at.
pub fn entry(head: [u8; 8], _body: u32) -> u32 {
    u32::from_le_bytes([head[4], head[5], head[6], head[7]]) & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reset_vector_without_its_thumb_bit() {
        let mut head = [0u8; 8];
        head[..4].copy_from_slice(&0x2001_0000u32.to_le_bytes());
        head[4..].copy_from_slice(&0x1004_8301u32.to_le_bytes());
        assert_eq!(entry(head, 0x1004_8200), 0x1004_8300);
    }
}
