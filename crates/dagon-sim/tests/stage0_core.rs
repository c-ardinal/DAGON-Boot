//! The simulated stage0 checks a boot core as the real one does
//! (`dagon_core::bootcore::check_inline`): a core whose header signature
//! does not verify under the node's key never starts, on each layout with
//! boot core slots.

use dagon_sim::*;
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 TEST 1 key (public test data): the node's key.
fn node_key() -> KeyPair {
    KeyPair::from_seed(Seed::new([
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ]))
}

/// A key the node does not hold.
fn other_key() -> KeyPair {
    KeyPair::from_seed(Seed::new([7; 32]))
}

/// Each MCU with boot core slots: its flash, its layout and the sample's
/// app for it (signed by the node's key).
type Node = (&'static str, fn(u64) -> SimFlash, Layout, &'static [u8]);

fn nodes() -> [Node; 2] {
    [
        (
            "stm32f401re",
            SimFlash::stm32f401xe,
            Layout::STM32F401XE,
            sample::SEG_1_0_0,
        ),
        (
            "rp2040",
            SimFlash::rp2040_4m,
            Layout::RP2040_HUB,
            sample::HUB_1_0_0,
        ),
    ]
}

/// Whether stage0 starts a boot core of a node provisioned with the boot
/// core `core` makes and the app `app`, its store verifying under the
/// node's key.
fn starts((_, flash, layout, app): Node, core: impl FnOnce(&SimStore) -> Vec<u8>) -> bool {
    assert_eq!(sample::PUBKEY, *node_key().pk);
    let mut store = SimStore::new(flash(1), layout, sample::PUBKEY);
    let image = core(&store);
    store.provision_core(&image).unwrap();
    store.provision(0, app).unwrap();
    store.start_core()
}

#[test]
fn stage0_starts_a_boot_core_signed_by_the_nodes_key() {
    for n in nodes() {
        let signed = |s: &SimStore| s.first_core(|m| *node_key().sk.sign(m, None)).unwrap();
        assert!(starts(n, signed), "{}", n.0);
    }
}

#[test]
fn stage0_starts_no_boot_core_signed_by_another_key() {
    for n in nodes() {
        let forged = |s: &SimStore| s.first_core(|m| *other_key().sk.sign(m, None)).unwrap();
        assert!(!starts(n, forged), "{}", n.0);
    }
}

#[test]
fn stage0_starts_no_boot_core_whose_header_changed_after_signing() {
    for n in nodes() {
        let changed = |s: &SimStore| {
            let mut c = s.first_core(|m| *node_key().sk.sign(m, None)).unwrap();
            // The version's patch byte, inside the signed bytes; the body
            // and its digest stay as signed.
            c[0x018] ^= 1;
            c
        };
        assert!(!starts(n, changed), "{}", n.0);
    }
}
