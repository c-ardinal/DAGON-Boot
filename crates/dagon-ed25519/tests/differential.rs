//! Both verifiers against `ed25519-dalek`'s `verify_strict` and the
//! predicate written with curve25519-dalek, over random keys and messages
//! from a fixed seed: each signature as made, then with one bit flipped in
//! the signature, in the key and in the message. A verifier that disagrees
//! with the predicate fails; so does `verify_strict`, and every such case is
//! listed in the failure.

mod common;

use common::{dalek_strict, outcomes, predicate, to_hex, Rng};
use std::thread;

const KEYS: usize = 10_000;
const SEED: u64 = 0x4441_474F_4E45_4432;

fn check_key(i: usize) -> Vec<String> {
    let mut rng = Rng(SEED ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut sk = [0u8; 32];
    rng.fill(&mut sk);
    let signer = ed25519_dalek::SigningKey::from_bytes(&sk);
    let pk = signer.verifying_key().to_bytes();
    let mut msg = vec![0u8; rng.below(300)];
    rng.fill(&mut msg);
    let sig = ed25519_dalek::Signer::sign(&signer, &msg).to_bytes();

    let mut cases = vec![("as made", pk, msg.clone(), sig)];
    let mut s = sig;
    s[rng.below(64)] ^= 1 << rng.below(8);
    cases.push(("signature bit", pk, msg.clone(), s));
    let mut k = pk;
    k[rng.below(32)] ^= 1 << rng.below(8);
    cases.push(("key bit", k, msg.clone(), sig));
    if !msg.is_empty() {
        let mut m = msg.clone();
        let at = rng.below(m.len());
        m[at] ^= 1 << rng.below(8);
        cases.push(("message bit", pk, m, sig));
    }

    let mut bad = Vec::new();
    for (what, pk, msg, sig) in cases {
        let expect = predicate(&pk, &msg, &sig);
        if what == "as made" && !expect {
            bad.push(format!(
                "key {i}: the predicate refused a signature as made"
            ));
        }
        let label = || {
            format!(
                "key {i} {what}: pk {} msg {} sig {}",
                to_hex(&pk),
                to_hex(&msg),
                to_hex(&sig)
            )
        };
        for (who, got) in outcomes(&pk, &msg, &sig) {
            if got != expect {
                bad.push(format!(
                    "{}: {who} gave {got}, the predicate {expect}",
                    label()
                ));
            }
        }
        let strict = dalek_strict(&pk, &msg, &sig);
        if strict != expect {
            bad.push(format!(
                "{}: verify_strict gave {strict}, the predicate {expect}",
                label()
            ));
        }
    }
    bad
}

#[test]
fn random_keys_and_bit_flips() {
    let threads = thread::available_parallelism().map_or(1, |n| n.get());
    let bad: Vec<String> = thread::scope(|sc| {
        let workers: Vec<_> = (0..threads)
            .map(|t| {
                sc.spawn(move || {
                    (t..KEYS)
                        .step_by(threads)
                        .flat_map(check_key)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    assert!(
        bad.is_empty(),
        "{} disagreement(s):\n{}",
        bad.len(),
        bad.join("\n")
    );
}
