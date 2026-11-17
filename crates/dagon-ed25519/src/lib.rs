//! Ed25519 signature verification (RFC 8032) for DAGON's signed
//! structures, as [`SigVerifier`] implementations. Verify only: nothing
//! here signs or holds a secret.
//!
//! # What is accepted
//!
//! Both verifiers accept a signature `R || S` of message `M` under public
//! key `A` exactly when all of these hold, and refuse it otherwise:
//!
//! - `A` and `R` are canonical point encodings: the y coordinate is below
//!   p = 2^255 - 19, and no other encoding names the same point (so x = 0
//!   with the sign bit set is refused);
//! - neither `A` nor `R` is of small order (the identity included);
//! - `S < L`, the order of the base point `B`;
//! - `[S]B == R + [k]A` with `k = SHA-512(R || A || M) mod L`, with no
//!   multiplication by the cofactor.
//!
//! `M` is the domain followed by the message parts
//! ([`SigVerifier::verify`]). One accept set on every node and on the host
//! keeps a signed header acceptable everywhere or nowhere.
//!
//! # Variable time
//!
//! The code runs in variable time. It handles only public data (the key,
//! the signature and the signed bytes), so its timing reveals nothing
//! secret; it must not be reused for anything that handles a secret.
//!
//! # The two verifiers
//!
//! - [`Ed25519Small`]: this crate's own arithmetic with SHA-512 from
//!   `sha2`, small enough for a stage0.
//! - [`Ed25519Compact`] (feature `compact`): `ed25519-compact`'s check,
//!   then the cofactorless equation, which `ed25519-compact` does not
//!   check on its own (it also accepts when the two sides differ by a point
//!   of small order). It costs about two verifications.
#![no_std]
#![forbid(unsafe_code)]

use dagon_format::SigVerifier;

mod small;

/// No tick: the check of [`Ed25519Small::new`], which calls nothing
/// between its steps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoTick;

/// The Ed25519 verifier built from this crate's own arithmetic. With a
/// tick ([`Ed25519Small::with_tick`]) a check calls it at every step of its
/// arithmetic (about 760 times per check, never more than about 700,000
/// Cortex-M0+ cycles apart), so a caller keeps a watchdog fed through a
/// check that takes seconds on a slow clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ed25519Small<T = NoTick> {
    key: [u8; 32],
    tick: T,
}

impl Ed25519Small {
    /// Checks under the 32-byte public key `key`. `const`, so the key can
    /// sit in a program's flash as a constant.
    pub const fn new(key: [u8; 32]) -> Self {
        Self { key, tick: NoTick }
    }
}

impl<F: Fn() + Copy> Ed25519Small<F> {
    /// Checks under `key`, calling `tick` at every step of a check.
    pub const fn with_tick(key: [u8; 32], tick: F) -> Self {
        Self { key, tick }
    }
}

impl SigVerifier for Ed25519Small {
    fn key_id(&self) -> [u8; 8] {
        dagon_format::key_id(&self.key)
    }

    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        small::verify(&self.key, domain, msg, sig, &|| {})
    }
}

impl<F: Fn() + Copy> SigVerifier for Ed25519Small<F> {
    fn key_id(&self) -> [u8; 8] {
        dagon_format::key_id(&self.key)
    }

    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        small::verify(&self.key, domain, msg, sig, &self.tick)
    }
}

/// The Ed25519 verifier backed by `ed25519-compact`, with the cofactorless
/// equation added.
#[cfg(feature = "compact")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ed25519Compact {
    key: [u8; 32],
}

#[cfg(feature = "compact")]
impl Ed25519Compact {
    /// Checks under the 32-byte public key `key`.
    pub const fn new(key: [u8; 32]) -> Self {
        Self { key }
    }
}

#[cfg(feature = "compact")]
impl SigVerifier for Ed25519Compact {
    fn key_id(&self) -> [u8; 8] {
        dagon_format::key_id(&self.key)
    }

    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        let pk = ed25519_compact::PublicKey::new(self.key);
        let Ok(mut st) = pk.verify_incremental(&ed25519_compact::Signature::new(*sig)) else {
            return false;
        };
        st.absorb(domain);
        for part in msg {
            st.absorb(part);
        }
        st.verify().is_ok() && small::verify(&self.key, domain, msg, sig, &|| {})
    }
}
