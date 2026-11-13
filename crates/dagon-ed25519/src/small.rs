//! The verification behind [`crate::Ed25519Small`].
//!
//! Field elements are 16 limbs of 16 bits, so that every limb product is
//! one 32-bit multiply (ARMv6-M has no 32 x 32 -> 64 multiply); field and
//! point formulas, constants and the reduction mod L follow TweetNaCl.
//! `[S]B - [k]A` is one double-scalar loop (Shamir's trick).
#![allow(clippy::needless_range_loop)]

use sha2::{Digest, Sha512};

/// A field element mod p = 2^255 - 19; every limb below 2^16 between
/// operations.
type Fe = [u32; 16];
/// A point in extended coordinates (X, Y, Z, T).
type P = [Fe; 4];

const ZERO: Fe = [0; 16];
const ONE: Fe = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// d = -121665 / 121666.
const D: Fe = [
    0x78a3, 0x1359, 0x4dca, 0x75eb, 0xd8ab, 0x4141, 0x0a4d, 0x0070, 0xe898, 0x7779, 0x4079, 0x8cc7,
    0xfe73, 0x2b6f, 0x6cee, 0x5203,
];
/// 2d.
const D2: Fe = [
    0xf159, 0x26b2, 0x9b94, 0xebd6, 0xb156, 0x8283, 0x149a, 0x00e0, 0xd130, 0xeef3, 0x80f2, 0x198e,
    0xfce7, 0x56df, 0xd9dc, 0x2406,
];
/// sqrt(-1).
const I: Fe = [
    0xa0b0, 0x4a0e, 0x1b27, 0xc4ee, 0xe478, 0xad2f, 0x1806, 0x2f43, 0xd7a7, 0x3dfb, 0x0099, 0x2b4d,
    0xdf0b, 0x4fc1, 0x2480, 0x2b83,
];
/// The base point B.
const BX: Fe = [
    0xd51a, 0x8f25, 0x2d60, 0xc956, 0xa7b2, 0x9525, 0xc760, 0x692c, 0xdc5c, 0xfdd6, 0xe231, 0xc0a4,
    0x53fe, 0xcd6e, 0x36d3, 0x2169,
];
const BY: Fe = [
    0x6658, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666, 0x6666,
    0x6666, 0x6666, 0x6666, 0x6666,
];
/// L = 2^252 + 27742317777372353535851937790883648493, little-endian.
const L: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

/// Carries every limb into the next, folding 2^256 = 38 back into limb 0.
fn norm(t: &mut [u64; 16]) -> Fe {
    loop {
        let mut c = 0;
        for x in t.iter_mut() {
            *x += c;
            c = *x >> 16;
            *x &= 0xffff;
        }
        if c == 0 {
            break;
        }
        t[0] += 38 * c;
    }
    t.map(|x| x as u32)
}

fn add(a: &Fe, b: &Fe) -> Fe {
    let mut t = [0u64; 16];
    for i in 0..16 {
        t[i] = (a[i] + b[i]) as u64;
    }
    norm(&mut t)
}

/// a - b + 4p, with 4p = 2^257 - 76 spread so that no limb goes negative.
fn sub(a: &Fe, b: &Fe) -> Fe {
    let mut t = [0u64; 16];
    for i in 0..16 {
        let c = if i == 0 { 0x1ffb4 } else { 0x1fffe };
        t[i] = (a[i] + c - b[i]) as u64;
    }
    norm(&mut t)
}

fn mul(a: &Fe, b: &Fe) -> Fe {
    let mut t = [0u64; 31];
    for i in 0..16 {
        for j in 0..16 {
            t[i + j] += (a[i] * b[j]) as u64;
        }
    }
    let mut r = [0u64; 16];
    for i in 0..16 {
        r[i] = t[i] + if i < 15 { 38 * t[i + 16] } else { 0 };
    }
    norm(&mut r)
}

/// a^(p-2) (the inverse), or a^((p-5)/8) with `p2523`, by TweetNaCl's
/// square-and-multiply chains.
fn pow(a: &Fe, p2523: bool) -> Fe {
    let mut c = *a;
    let top = if p2523 { 250 } else { 253 };
    for k in (0..=top).rev() {
        c = mul(&c, &c);
        let skip = if p2523 { k == 1 } else { k == 2 || k == 4 };
        if !skip {
            c = mul(&c, a);
        }
    }
    c
}

/// The canonical (fully reduced) little-endian bytes of `a`.
fn pack(a: &Fe) -> [u8; 32] {
    let mut t = *a;
    for _ in 0..2 {
        let mut m = t;
        let mut borrow = 0u32;
        for i in 0..16 {
            let pi = match i {
                0 => 0xffed,
                15 => 0x7fff,
                _ => 0xffff,
            };
            let v = t[i].wrapping_sub(pi).wrapping_sub(borrow);
            borrow = v >> 31;
            m[i] = v & 0xffff;
        }
        if borrow == 0 {
            t = m;
        }
    }
    let mut o = [0u8; 32];
    for i in 0..16 {
        o[2 * i] = t[i] as u8;
        o[2 * i + 1] = (t[i] >> 8) as u8;
    }
    o
}

/// The low 255 bits of `b`, unreduced.
fn unpack(b: &[u8; 32]) -> Fe {
    let mut o = [0u32; 16];
    for i in 0..16 {
        o[i] = b[2 * i] as u32 | (b[2 * i + 1] as u32) << 8;
    }
    o[15] &= 0x7fff;
    o
}

fn parity(a: &Fe) -> u8 {
    pack(a)[0] & 1
}

/// p + q, a complete formula (also right for p == q and the identity).
fn padd(p: &P, q: &P) -> P {
    let a = mul(&sub(&p[1], &p[0]), &sub(&q[1], &q[0]));
    let b = mul(&add(&p[0], &p[1]), &add(&q[0], &q[1]));
    let c = mul(&mul(&p[3], &q[3]), &D2);
    let d = mul(&p[2], &q[2]);
    let d = add(&d, &d);
    let (e, f, g, h) = (sub(&b, &a), sub(&d, &c), add(&d, &c), add(&b, &a));
    [mul(&e, &f), mul(&h, &g), mul(&g, &f), mul(&e, &h)]
}

/// Whether [8]p is the identity.
fn small_order(p: &P) -> bool {
    let mut q = *p;
    for _ in 0..3 {
        q = padd(&q, &q);
    }
    pack(&q[0]) == [0; 32] && pack(&q[1]) == pack(&q[2])
}

/// -A for the encoding `pk`; `None` unless `pk` is the canonical
/// encoding of a curve point.
fn decode_neg(pk: &[u8; 32]) -> Option<P> {
    let y = unpack(pk);
    let mut y_bytes = *pk;
    y_bytes[31] &= 0x7f;
    if pack(&y) != y_bytes {
        return None;
    }
    let num = mul(&y, &y);
    let den = add(&ONE, &mul(&num, &D));
    let num = sub(&num, &ONE);
    let den2 = mul(&den, &den);
    let den6 = mul(&mul(&den2, &den2), &den2);
    let t = pow(&mul(&mul(&den6, &num), &den), true);
    let mut x = mul(&mul(&mul(&mul(&t, &num), &den), &den), &den);
    let is_root = |x: &Fe| pack(&mul(&mul(x, x), &den)) == pack(&num);
    if !is_root(&x) {
        x = mul(&x, &I);
    }
    if !is_root(&x) {
        return None;
    }
    let sign = pk[31] >> 7;
    if sign == 1 && pack(&x) == [0; 32] {
        return None;
    }
    if parity(&x) == sign {
        x = sub(&ZERO, &x);
    }
    Some([x, y, ONE, mul(&x, &y)])
}

/// TweetNaCl's modL: the 64-byte little-endian `x` mod L.
fn mod_l(x: &mut [i64; 64]) -> [u8; 32] {
    for i in (32..64).rev() {
        let mut carry = 0;
        let mut j = i - 32;
        while j < i - 12 {
            x[j] += carry - 16 * x[i] * L[j - (i - 32)] as i64;
            carry = (x[j] + 128) >> 8;
            x[j] -= carry << 8;
            j += 1;
        }
        x[j] += carry;
        x[i] = 0;
    }
    let mut carry = 0;
    for j in 0..32 {
        x[j] += carry - (x[31] >> 4) * L[j] as i64;
        carry = x[j] >> 8;
        x[j] &= 255;
    }
    for j in 0..32 {
        x[j] -= carry * L[j] as i64;
    }
    let mut r = [0u8; 32];
    for i in 0..32 {
        x[i + 1] += x[i] >> 8;
        r[i] = (x[i] & 255) as u8;
    }
    r
}

/// Whether the little-endian `s` is below L.
fn below_l(s: &[u8]) -> bool {
    for i in (0..32).rev() {
        if s[i] != L[i] {
            return s[i] < L[i];
        }
    }
    false
}

fn bit(s: &[u8], i: usize) -> bool {
    (s[i >> 3] >> (i & 7)) & 1 == 1
}

/// The crate's predicate (see the crate documentation) for `sig` over
/// `domain` || `msg` under `pk`.
pub fn verify(pk: &[u8; 32], domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
    let (r, s) = sig.split_at(32);
    if !below_l(s) {
        return false;
    }
    let Some(neg_a) = decode_neg(pk) else {
        return false;
    };
    if small_order(&neg_a) {
        return false;
    }
    let mut h = Sha512::new();
    h.update(r);
    h.update(pk);
    h.update(domain);
    for part in msg {
        h.update(part);
    }
    let mut wide = [0i64; 64];
    for (w, b) in wide.iter_mut().zip(h.finalize().iter()) {
        *w = *b as i64;
    }
    let k = mod_l(&mut wide);
    let b = [BX, BY, ONE, mul(&BX, &BY)];
    let b_neg_a = padd(&b, &neg_a);
    let mut q: P = [ZERO, ONE, ONE, ZERO];
    // S and k are below L < 2^253.
    for i in (0..253).rev() {
        q = padd(&q, &q);
        match (bit(s, i), bit(&k, i)) {
            (true, true) => q = padd(&q, &b_neg_a),
            (true, false) => q = padd(&q, &b),
            (false, true) => q = padd(&q, &neg_a),
            (false, false) => {}
        }
    }
    let zi = pow(&q[2], false);
    let mut encoded = pack(&mul(&q[1], &zi));
    encoded[31] ^= parity(&mul(&q[0], &zi)) << 7;
    // R equal to the canonical encoding of [S]B - [k]A: R is canonical and
    // names that point, so R's order is that point's.
    encoded[..] == *r && !small_order(&q)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fe(n: u32) -> Fe {
        let mut f = ZERO;
        f[0] = n & 0xffff;
        f[1] = n >> 16;
        f
    }

    const P_BYTES: [u8; 32] = {
        let mut p = [0xff; 32];
        p[0] = 0xed;
        p[31] = 0x7f;
        p
    };

    /// The constants follow from their definitions.
    #[test]
    fn constants() {
        let minus_one = sub(&ZERO, &ONE);
        assert_eq!(pack(&mul(&I, &I)), pack(&minus_one));
        assert_eq!(pack(&mul(&D, &fe(121666))), pack(&sub(&ZERO, &fe(121665))));
        assert_eq!(pack(&D2), pack(&add(&D, &D)));
        // B's encoding (RFC 8032 section 5.1): y = 4/5, x even.
        assert_eq!(pack(&mul(&BY, &fe(5))), pack(&fe(4)));
        assert_eq!(parity(&BX), 0);
        // On the curve: -x^2 + y^2 = 1 + d x^2 y^2.
        let (x2, y2) = (mul(&BX, &BX), mul(&BY, &BY));
        assert_eq!(
            pack(&sub(&y2, &x2)),
            pack(&add(&ONE, &mul(&D, &mul(&x2, &y2))))
        );
        assert_eq!(L[31], 0x10);
    }

    #[test]
    fn s_range() {
        let mut l_minus_1 = L;
        l_minus_1[0] -= 1;
        let mut two_253 = [0u8; 32];
        two_253[31] = 0x20;
        assert!(below_l(&[0; 32]));
        assert!(below_l(&l_minus_1));
        assert!(!below_l(&L));
        assert!(!below_l(&two_253));
        assert!(!below_l(&[0xff; 32]));
    }

    #[test]
    fn mod_l_reduces() {
        let mut x = [0i64; 64];
        x[..32].copy_from_slice(&L.map(i64::from));
        assert_eq!(mod_l(&mut x), [0; 32]);
        let mut x = [0i64; 64];
        x[..32].copy_from_slice(&L.map(i64::from));
        x[0] += 5;
        let mut five = [0u8; 32];
        five[0] = 5;
        assert_eq!(mod_l(&mut x), five);
        // (2^512 - 1) mod L, computed with Python's integers.
        let want = [
            0, 15, 156, 68, 227, 17, 6, 164, 71, 147, 133, 104, 167, 27, 14, 208, 101, 190, 245,
            23, 210, 115, 236, 206, 61, 154, 48, 124, 27, 65, 153, 3,
        ];
        assert_eq!(mod_l(&mut [255i64; 64]), want);
    }

    /// Each y at or above p (both sign bits) that would name a point is
    /// refused; the same point's canonical encoding is accepted.
    #[test]
    fn decode_refuses_non_canonical() {
        let mut named = 0;
        for y in 0u8..19 {
            for sign in [0u8, 0x80] {
                let mut e = P_BYTES;
                e[0] += y;
                e[31] |= sign;
                assert!(decode_neg(&e).is_none(), "y = p + {y}, sign {sign}");
                let mut canonical = [0u8; 32];
                canonical[0] = y;
                canonical[31] = sign;
                if let Some(pt) = decode_neg(&canonical) {
                    named += 1;
                    canonical[31] &= 0x7f;
                    assert_eq!(pack(&pt[1]), canonical);
                }
            }
        }
        assert!(named > 0);
        // x = 0 with the sign bit: y = 1 (the identity) and y = p - 1.
        let mut one_neg = [0u8; 32];
        one_neg[0] = 1;
        one_neg[31] = 0x80;
        assert!(decode_neg(&one_neg).is_none());
        let mut one = [0u8; 32];
        one[0] = 1;
        assert!(decode_neg(&one).is_some());
        let mut minus_one = P_BYTES;
        minus_one[0] -= 1;
        assert!(decode_neg(&minus_one).is_some());
        minus_one[31] |= 0x80;
        assert!(decode_neg(&minus_one).is_none());
    }

    #[test]
    fn small_order_points() {
        let mut one = [0u8; 32];
        one[0] = 1;
        assert!(small_order(&decode_neg(&one).unwrap()));
        let b = [BX, BY, ONE, mul(&BX, &BY)];
        assert!(!small_order(&b));
    }
}
