//! Build-time verifying key helpers for firmware.
//!
//! Boards embed the public key named by the `DAGON_PUBKEY` environment
//! variable (the public key file from `dagon key gen`; 64 hex digits) and
//! may fall back to
//! [`TEST_PUBKEY`]. Firmware built with the test key accepts any image signed
//! with its published secret, so it must say so (HELLO, displays) via
//! [`is_test_key`].

/// RFC 8032 section 7.1 TEST 1 public key. Its secret key is public test data.
pub const TEST_PUBKEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// Parses 64 hex digits. Meant for `const` use, where a bad value fails the
/// build.
pub const fn pubkey_from_hex(s: &str) -> [u8; 32] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("public key: not a hex digit"),
        }
    }
    let b = s.as_bytes();
    assert!(b.len() == 64, "public key: need 64 hex digits");
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nib(b[2 * i]) << 4 | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

pub const fn is_test_key(key: &[u8; 32]) -> bool {
    let mut i = 0;
    while i < 32 {
        if key[i] != TEST_PUBKEY[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_test_key() {
        let k = pubkey_from_hex("D75A980182B10AB7D54BFED3C964073A0EE172F3DAA62325AF021A68F707511A");
        assert!(is_test_key(&k));
        let mut other = k;
        other[31] ^= 1;
        assert!(!is_test_key(&other));
    }
}
