//! What the vector and differential tests share: both verifiers, hex, and
//! the crate's predicate written a second time with curve25519-dalek's
//! arithmetic, so expected outcomes come from an implementation that shares
//! no code with the one under test.
#![allow(dead_code)]

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use dagon_ed25519::{Ed25519Compact, Ed25519Small};
use dagon_format::SigVerifier;
use sha2::{Digest, Sha512};

pub fn hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Each verifier's outcome for `sig` over `msg` (no domain) under `pk`.
pub fn outcomes(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> [(&'static str, bool); 2] {
    [
        (
            "Ed25519Small",
            Ed25519Small::new(*pk).verify(b"", &[msg], sig),
        ),
        (
            "Ed25519Compact",
            Ed25519Compact::new(*pk).verify(b"", &[msg], sig),
        ),
    ]
}

/// `ed25519-dalek`'s `verify_strict`.
pub fn dalek_strict(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(pk) else {
        return false;
    };
    vk.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig))
        .is_ok()
}

/// The point `b` encodes, only when `b` is its canonical encoding.
pub fn decode_canonical(b: &[u8; 32]) -> Option<EdwardsPoint> {
    let p = CompressedEdwardsY(*b).decompress()?;
    (p.compress().0 == *b).then_some(p)
}

pub fn challenge(r: &[u8], a: &[u8], msg: &[u8]) -> Scalar {
    let mut h = Sha512::new();
    h.update(r);
    h.update(a);
    h.update(msg);
    Scalar::from_bytes_mod_order_wide(&h.finalize().into())
}

/// The predicate: canonical A and R, neither of small order, S < L and
/// [S]B == R + [k]A without the cofactor.
pub fn predicate(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    let r: [u8; 32] = sig[..32].try_into().unwrap();
    let s: [u8; 32] = sig[32..].try_into().unwrap();
    let (Some(a), Some(rp)) = (decode_canonical(pk), decode_canonical(&r)) else {
        return false;
    };
    if a.is_small_order() || rp.is_small_order() {
        return false;
    }
    let Some(s) = Option::<Scalar>::from(Scalar::from_canonical_bytes(s)) else {
        return false;
    };
    let k = challenge(&r, pk, msg);
    EdwardsPoint::vartime_double_scalar_mul_basepoint(&k, &-a, &s) == rp
}

/// Deterministic generator for the tests' keys and messages (SplitMix64).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn fill(&mut self, out: &mut [u8]) {
        for b in out {
            *b = self.next() as u8;
        }
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
