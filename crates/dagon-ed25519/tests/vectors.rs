//! Both verifiers against RFC 8032 section 7.1, the pinned Wycheproof file
//! and boundary vectors written for the crate's predicate. A verifier that
//! disagrees with the expected outcome fails the test; so does
//! `ed25519-dalek`'s `verify_strict` disagreeing with the predicate, and
//! every such case is listed in the failure.

mod common;

use common::{challenge, dalek_strict, hex, outcomes, predicate, to_hex};
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::{Identity, IsIdentity};
use sha2::{Digest, Sha256, Sha512};

const WYCHEPROOF: &[u8] = include_bytes!("vectors/ed25519_test.json");
const WYCHEPROOF_SHA256: &str = "ea80ffebac085e21cb9d3e903e7fa2b448d96702da5b31f061d0b83b430b74b3";
const WYCHEPROOF_BYTES: usize = 172_628;
const WYCHEPROOF_CASES: usize = 210;
const RFC8032: &str = include_str!("vectors/rfc8032.txt");
const RFC8032_SHA256: &str = "590728244b421636fe117eba9ddf1aa47b104d8c411b805d5fda56fea7668629";

/// One case: what it is, the inputs and the outcome every verifier and
/// `verify_strict` must give.
struct Case {
    name: String,
    pk: [u8; 32],
    msg: Vec<u8>,
    sig: [u8; 64],
    expect: bool,
}

/// Runs every case; returns the list of disagreements (empty when none).
fn disagreements(cases: &[Case]) -> Vec<String> {
    let mut out = Vec::new();
    for c in cases {
        for (who, got) in outcomes(&c.pk, &c.msg, &c.sig) {
            if got != c.expect {
                out.push(format!(
                    "{}: {who} gave {got}, expected {}",
                    c.name, c.expect
                ));
            }
        }
        let strict = dalek_strict(&c.pk, &c.msg, &c.sig);
        if strict != c.expect {
            out.push(format!(
                "{}: verify_strict gave {strict}, the predicate {} (pk {}, sig {})",
                c.name,
                c.expect,
                to_hex(&c.pk),
                to_hex(&c.sig)
            ));
        }
    }
    out
}

fn assert_all(cases: &[Case]) {
    let bad = disagreements(cases);
    assert!(
        bad.is_empty(),
        "{} disagreement(s):\n{}",
        bad.len(),
        bad.join("\n")
    );
}

fn arr<const N: usize>(b: &[u8]) -> [u8; N] {
    b.try_into().expect("length")
}

#[test]
fn rfc8032_section_7_1() {
    assert_eq!(to_hex(&Sha256::digest(RFC8032.as_bytes())), RFC8032_SHA256);
    let mut cases = Vec::new();
    for line in RFC8032.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        let (sk, pk, sig) = (
            arr::<32>(&hex(f[1])),
            arr::<32>(&hex(f[2])),
            arr(&hex(f[3])),
        );
        let msg = if f[4] == "-" { Vec::new() } else { hex(f[4]) };
        // The transcription: the key pair and signature follow from the secret key.
        let signer = ed25519_dalek::SigningKey::from_bytes(&sk);
        assert_eq!(signer.verifying_key().to_bytes(), pk, "TEST {}", f[0]);
        let ours = ed25519_dalek::Signer::sign(&signer, &msg).to_bytes();
        assert_eq!(ours, sig, "TEST {}", f[0]);
        let mut bad = sig;
        bad[0] ^= 1;
        cases.push(Case {
            name: format!("TEST {}", f[0]),
            pk,
            msg: msg.clone(),
            sig,
            expect: true,
        });
        cases.push(Case {
            name: format!("TEST {} R flipped", f[0]),
            pk,
            msg,
            sig: bad,
            expect: false,
        });
    }
    let names: Vec<&str> = RFC8032
        .lines()
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(names, ["1", "2", "3", "1024", "SHA(abc)"]);
    assert_all(&cases);
}

/// The domain and the message parts are one message: split anywhere, the
/// outcome is the same.
#[test]
fn domain_and_parts_are_one_message() {
    use dagon_ed25519::{Ed25519Compact, Ed25519Small};
    use dagon_format::SigVerifier;
    let line = RFC8032.lines().nth(3).unwrap();
    let f: Vec<&str> = line.split(' ').collect();
    let (pk, sig, msg) = (arr::<32>(&hex(f[2])), arr::<64>(&hex(f[3])), hex(f[4]));
    let verifiers: [&dyn SigVerifier; 2] = [&Ed25519Small::new(pk), &Ed25519Compact::new(pk)];
    for v in verifiers {
        assert!(v.verify(&msg[..11], &[&msg[11..300], &[], &msg[300..]], &sig));
        assert!(!v.verify(&msg[..11], &[&msg[11..300]], &sig));
        assert_eq!(v.key_id(), dagon_format::key_id(&pk));
    }
}

#[test]
fn wycheproof_pinned_file() {
    // The file is checked before any case is read.
    assert_eq!(WYCHEPROOF.len(), WYCHEPROOF_BYTES);
    assert_eq!(to_hex(&Sha256::digest(WYCHEPROOF)), WYCHEPROOF_SHA256);
    let doc: serde_json::Value = serde_json::from_slice(WYCHEPROOF).unwrap();
    assert_eq!(doc["numberOfTests"], WYCHEPROOF_CASES);
    let mut cases = Vec::new();
    let mut seen = 0;
    for g in doc["testGroups"].as_array().unwrap() {
        let pk = hex(g["publicKey"]["pk"].as_str().unwrap());
        for t in g["tests"].as_array().unwrap() {
            seen += 1;
            let id = t["tcId"].as_u64().unwrap();
            let msg = hex(t["msg"].as_str().unwrap());
            let sig = hex(t["sig"].as_str().unwrap());
            let flags: Vec<&str> = t["flags"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap())
                .collect();
            let result = t["result"].as_str().unwrap();
            // A signature or key of another length never reaches a verifier
            // (the header holds 64 and 32 bytes); Wycheproof calls all of them invalid.
            let (Ok(pk), Ok(sig)) = (
                <[u8; 32]>::try_from(&pk[..]),
                <[u8; 64]>::try_from(&sig[..]),
            ) else {
                assert_eq!(result, "invalid", "tcId {id}");
                continue;
            };
            let expect = match (result, flags.as_slice()) {
                ("valid", _) => true,
                ("invalid", _) => false,
                ("acceptable", ["SmallOrderPublicKey"]) => false,
                // Cofactored-valid with a mixed-order key: the cofactorless
                // equation decides.
                ("acceptable", ["MixedOrderPublicKey"]) => predicate(&pk, &msg, &sig),
                other => panic!("tcId {id}: no rule for {other:?}"),
            };
            if result != "acceptable" {
                assert_eq!(
                    predicate(&pk, &msg, &sig),
                    expect,
                    "the predicate itself, tcId {id}"
                );
            }
            cases.push(Case {
                name: format!("tcId {id} {flags:?}"),
                pk,
                msg,
                sig,
                expect,
            });
        }
    }
    assert_eq!(seen, WYCHEPROOF_CASES);
    assert_all(&cases);
}

// Boundary vectors. The key is RFC 8032 TEST 1's secret scalar, the message fixed.

const MSG: &[u8] = b"DAGON boundary vector";
const L_HEX: &str = "edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010";
const P_HEX: &str = "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f";

/// Little-endian sum, modulo 2^256.
fn add32(x: &[u8; 32], y: &[u8; 32]) -> [u8; 32] {
    let mut o = [0u8; 32];
    let mut c = 0u16;
    for i in 0..32 {
        let v = x[i] as u16 + y[i] as u16 + c;
        o[i] = v as u8;
        c = v >> 8;
    }
    o
}

fn sign_flipped(mut e: [u8; 32]) -> [u8; 32] {
    e[31] ^= 0x80;
    e
}

fn secret() -> (Scalar, EdwardsPoint) {
    let sk = hex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
    let h = Sha512::digest(&sk);
    let mut a: [u8; 32] = h[..32].try_into().unwrap();
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let a = Scalar::from_bytes_mod_order(a);
    (a, EdwardsPoint::mul_base(&a))
}

/// A nonce from a counter: the boundary searches below try one after another.
fn nonce(i: u64) -> Scalar {
    let mut wide = [0u8; 64];
    wide[..8].copy_from_slice(&i.to_le_bytes());
    wide[8] = 0xA5;
    Scalar::from_bytes_mod_order_wide(&Sha512::digest(wide).into())
}

fn sig_bytes(r: &[u8; 32], s: &[u8; 32]) -> [u8; 64] {
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(r);
    sig[32..].copy_from_slice(s);
    sig
}

/// The eight points of order dividing 8, from one of order 8.
fn torsion() -> Vec<EdwardsPoint> {
    let g = CompressedEdwardsY(arr(&hex(
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    )))
    .decompress()
    .unwrap();
    assert!(g.is_small_order() && !(g + g + g + g).is_identity());
    let mut out = vec![EdwardsPoint::identity()];
    for _ in 1..8 {
        out.push(out.last().unwrap() + g);
    }
    out
}

/// Every 32-byte string that decodes (leniently: y taken mod p, x = 0
/// with either sign) to a point of small order: the canonical encodings
/// and the non-canonical ones.
fn small_order_encodings() -> Vec<([u8; 32], bool)> {
    let mut out = Vec::new();
    for t in torsion() {
        let c = t.compress().0;
        let mut y = c;
        y[31] &= 0x7f;
        // y + p, when it stays below 2^255.
        let sum = add32(&y, &arr(&hex(P_HEX)));
        let mut alts = vec![c, sign_flipped(c)];
        // y and p are below 2^255, so the sum has no carry out of bit 255.
        if sum[31] & 0x80 == 0 {
            alts.extend([sum, sign_flipped(sum)]);
        }
        for e in alts {
            if CompressedEdwardsY(e).decompress() == Some(t) && !out.iter().any(|(o, _)| *o == e) {
                out.push((e, e == c));
            }
        }
    }
    out
}

#[test]
fn boundary_vectors() {
    let (a, ap) = secret();
    let pk = ap.compress().0;
    let mut cases = Vec::new();
    let r0 = nonce(0);
    let rp = EdwardsPoint::mul_base(&r0).compress().0;
    let k = challenge(&rp, &pk, MSG);
    let s = r0 + k * a;
    let valid = sig_bytes(&rp, &s.to_bytes());
    cases.push(Case {
        name: "valid".into(),
        pk,
        msg: MSG.into(),
        sig: valid,
        expect: true,
    });

    // S at and past L: S + L, L - 1, L, 2^253.
    let l: [u8; 32] = arr(&hex(L_HEX));
    let mut l_minus_1 = l;
    l_minus_1[0] -= 1;
    let mut two_253 = [0u8; 32];
    two_253[31] = 0x20;
    for (name, sv) in [
        ("S + L", add32(&s.to_bytes(), &l)),
        ("S = L - 1", l_minus_1),
        ("S = L", l),
        ("S = 2^253", two_253),
    ] {
        cases.push(Case {
            name: name.into(),
            pk,
            msg: MSG.into(),
            sig: sig_bytes(&rp, &sv),
            expect: false,
        });
    }

    let enc = small_order_encodings();
    // Two per point with x = 0 and sign set, and y + p where it fits.
    assert_eq!(enc.iter().filter(|(_, c)| *c).count(), 8);
    let g8 = torsion()[1];
    for (e, canonical) in &enc {
        let t = CompressedEdwardsY(*e).decompress().unwrap();
        // Small-order A: R = [r]B, S = r holds the equation once [k]A = O.
        let (r, k) = (0..)
            .map(|i| {
                (
                    nonce(i),
                    challenge(&EdwardsPoint::mul_base(&nonce(i)).compress().0, e, MSG),
                )
            })
            .find(|(_, k)| (k * t).is_identity())
            .unwrap();
        let rb = EdwardsPoint::mul_base(&r).compress().0;
        assert!((k * t).is_identity());
        cases.push(Case {
            name: format!("small-order A {} (canonical {canonical})", to_hex(e)),
            pk: *e,
            msg: MSG.into(),
            sig: sig_bytes(&rb, &r.to_bytes()),
            expect: false,
        });
        // Small-order R under a mixed-order key A' = [a]B + G8: S = k a holds
        // the equation once R = -[k]G8.
        let mixed = (ap + g8).compress().0;
        let (msg, k) = (0u32..)
            .map(|i| [MSG, &i.to_le_bytes()].concat())
            .map(|m| {
                let k = challenge(e, &mixed, &m);
                (m, k)
            })
            .find(|(_, k)| -(k * g8) == t)
            .unwrap();
        cases.push(Case {
            name: format!("small-order R {} (canonical {canonical})", to_hex(e)),
            pk: mixed,
            msg,
            sig: sig_bytes(e, &(k * a).to_bytes()),
            expect: false,
        });
    }

    // R off by a torsion point: [S]B - [k]A - R is of small order but not
    // the identity. ed25519-compact on its own accepts this.
    for (i, t) in torsion().into_iter().enumerate().skip(1) {
        let r = nonce(100 + i as u64);
        let rt = (EdwardsPoint::mul_base(&r) + t).compress().0;
        let k = challenge(&rt, &pk, MSG);
        let sig = sig_bytes(&rt, &(r + k * a).to_bytes());
        let lib = ed25519_compact::PublicKey::new(pk)
            .verify(MSG, &ed25519_compact::Signature::new(sig))
            .is_ok();
        assert!(lib, "torsion offset {i}: ed25519-compact alone accepts");
        cases.push(Case {
            name: format!("R + torsion {i}"),
            pk,
            msg: MSG.into(),
            sig,
            expect: false,
        });
    }

    // A mixed-order key with a signature that holds without the cofactor.
    for (i, tg) in torsion().into_iter().enumerate().skip(1) {
        let mixed_p = ap + tg;
        let mixed = mixed_p.compress().0;
        let (r, rt, k) = (0..)
            .flat_map(|j| torsion().into_iter().map(move |tr| (nonce(200 + j), tr)))
            .map(|(r, tr)| {
                let rt = (EdwardsPoint::mul_base(&r) + tr).compress().0;
                (r, rt, challenge(&rt, &mixed, MSG), tr)
            })
            .find(|(_, _, k, tr)| -(k * tg) == *tr)
            .map(|(r, rt, k, _)| (r, rt, k))
            .unwrap();
        let sig = sig_bytes(&rt, &(r + k * a).to_bytes());
        cases.push(Case {
            name: format!("mixed-order A {i}, cofactorless"),
            pk: mixed,
            msg: MSG.into(),
            sig,
            expect: true,
        });
    }

    // Expected outcomes above are written by hand; the predicate written
    // with another curve implementation gives the same ones.
    for c in &cases {
        assert_eq!(
            predicate(&c.pk, &c.msg, &c.sig),
            c.expect,
            "the predicate itself, {}",
            c.name
        );
    }
    assert_all(&cases);
}

/// A canonical-y key and its non-canonical encoding (y + p) are not the
/// same key: the hash covers the bytes. The encodings above the field
/// prime that name a point at all are refused as keys and as R, whatever
/// the signature.
#[test]
fn non_canonical_encodings_are_refused() {
    let (_, ap) = secret();
    let p = hex(P_HEX);
    let mut found = 0;
    for y in 0u8..19 {
        for sign in [0u8, 0x80] {
            let mut e: [u8; 32] = arr(&p);
            e[0] = e[0].wrapping_add(y);
            e[31] |= sign;
            let Some(pt) = CompressedEdwardsY(e).decompress() else {
                continue;
            };
            found += 1;
            // As a key, with R a valid-looking point; as R, under a valid key.
            let r = EdwardsPoint::mul_base(&nonce(7)).compress().0;
            for (pk, sig) in [
                (e, sig_bytes(&r, &[1; 32])),
                (ap.compress().0, sig_bytes(&e, &[1; 32])),
            ] {
                for (who, got) in outcomes(&pk, MSG, &sig) {
                    assert!(!got, "{who} accepted non-canonical {}", to_hex(&e));
                }
            }
            assert_ne!(pt.compress().0, e);
        }
    }
    assert!(found > 0);
}
