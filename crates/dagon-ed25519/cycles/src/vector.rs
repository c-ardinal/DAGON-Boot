// A stage0-shaped signed message: the image header's signing domain and a
// 448-byte header body (byte i is 3 + 7i mod 256), signed under KEY. Shared
// by the M0+ program (src/main.rs) and its host test (tests/m0_cycles.rs).

/// The verifying key.
pub const KEY: [u8; 32] = hex32("03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8");

/// The signature over `dagon_format::header::SIGN_DOMAIN` and [`BODY`].
pub const SIG: [u8; 64] = {
    let a = hex32("c0fa1477ac8e591fad198d607a704807728bd94a67758717db177e7934a771f8");
    let b = hex32("e0498f4d16095ea7285ef6f1b459fd878f993e423e1314e3754bb03fe82d5402");
    let mut s = [0u8; 64];
    let mut i = 0;
    while i < 32 {
        s[i] = a[i];
        s[32 + i] = b[i];
        i += 1;
    }
    s
};

/// The signed header body.
pub const BODY: [u8; 448] = {
    let mut h = [0u8; 448];
    let mut i = 0;
    while i < 448 {
        h[i] = (3 + 7 * i) as u8;
        i += 1;
    }
    h
};

const fn hex32(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nibble(b[2 * i]) << 4 | nibble(b[2 * i + 1]);
        i += 1;
    }
    out
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        _ => c - b'a' + 10,
    }
}
