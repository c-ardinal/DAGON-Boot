//! Clears of a node's link diagnostics. A clear of ever-communicated
//! records is signed with the project's signing key, found and checked as
//! for image signing ([`crate::config::load_key`]); the key never leaves
//! this process.

#![cfg_attr(not(test), allow(dead_code))]

use dagon_format::{ClearSigner, SignError};
use dagon_host::diag::ClearOutcome;
use dagon_host::{Host, HostClock, Target, Transport};
use dagon_proto::diag::ClearScope;
use dagon_sim::block_on;
use ed25519_compact::KeyPair;

/// The project's key pair as the host's signer.
pub struct KeySigner(pub KeyPair);

impl ClearSigner for KeySigner {
    fn key_id(&self) -> [u8; 8] {
        dagon_format::key_id(&self.0.pk)
    }

    fn sign(&self, domain: &[u8], msg: &[&[u8]]) -> Result<[u8; 64], SignError> {
        let mut m = domain.to_vec();
        msg.iter().for_each(|p| m.extend_from_slice(p));
        Ok(*self.0.sk.sign(m, None))
    }
}

/// Clears `scope` at `to`. A signed scope takes its key from `key` before
/// anything is sent, so without one no frame leaves.
pub fn clear<T: Transport, C: HostClock>(
    host: &mut Host<T, C>,
    to: Target,
    scope: ClearScope,
    key: impl FnOnce() -> Result<KeyPair, String>,
) -> Result<ClearOutcome, String>
where
    T::Error: std::fmt::Debug,
{
    let signer = if scope.signed() {
        Some(KeySigner(key()?))
    } else {
        None
    };
    let signer = signer.as_ref().map(|s| s as &dyn ClearSigner);
    block_on(host.link_diag_clear(to, scope, signer)).map_err(|e| match e {
        dagon_host::Error::ConnectDirectly { request, room } => format!(
            "the clear is {request} bytes and the route to the node carries {room}: \
             connect the PC to that node directly"
        ),
        dagon_host::Error::KeyMismatch { signer, node } => format!(
            "the signing key {} is not the node's key {}",
            hex(&signer),
            hex(&node)
        ),
        e => format!("{e:?}"),
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Counts the frames written; nothing answers.
    #[derive(Clone, Default)]
    struct Count(Rc<RefCell<usize>>);

    impl Transport for Count {
        type Error = ();
        fn max_payload(&self) -> usize {
            dagon_proto::max_payload::USB
        }
        fn reconnect_kind(&self) -> dagon_host::Reconnect {
            dagon_host::Reconnect::Stays
        }
        async fn write(&mut self, _: &[u8]) -> Result<(), ()> {
            *self.0.borrow_mut() += 1;
            Ok(())
        }
        async fn read(&mut self, _: &mut [u8]) -> Result<usize, ()> {
            Ok(0)
        }
        async fn reopen(&mut self) -> Result<(), ()> {
            Ok(())
        }
    }

    impl HostClock for Count {
        fn now_ms(&self) -> u64 {
            0
        }
        async fn sleep_ms(&mut self, _: u32) {}
    }

    #[test]
    fn diag_clear_without_key() {
        let t = Count::default();
        let mut host = Host::new(t.clone(), t.clone());
        let no_key = || crate::config::load_key_from(None, None);
        let e = clear(&mut host, Target::Local, ClearScope::All, no_key).unwrap_err();
        assert_eq!(
            e,
            format!(
                "no signing key: pass --key <file> or set {}",
                crate::config::KEY_ENV
            )
        );
        assert_eq!(*t.0.borrow(), 0, "no frame sent");
    }

    /// The signature is the project key's over the domain and the parts.
    #[test]
    fn the_key_pair_signs_domain_and_parts() {
        let kp = KeyPair::from_seed(ed25519_compact::Seed::new([3; 32]));
        let s = KeySigner(kp.clone());
        let sig = s.sign(b"D", &[b"ab", b"c"]).unwrap();
        let sig = ed25519_compact::Signature::from_slice(&sig).unwrap();
        assert!(kp.pk.verify(b"Dabc", &sig).is_ok());
        assert_eq!(s.key_id(), dagon_format::key_id(&kp.pk));
    }
}
