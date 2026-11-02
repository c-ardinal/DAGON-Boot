//! Little-endian field access. Callers check lengths first; out-of-range
//! offsets are programmer errors and panic.

pub fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(arr(b, off))
}

pub fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(arr(b, off))
}

pub fn put_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn arr<const N: usize>(b: &[u8], off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&b[off..off + N]);
    out
}
