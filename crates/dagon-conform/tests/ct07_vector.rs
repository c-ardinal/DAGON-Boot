//! The signed header CT-07 flips bits of (`vectors/ct07_header.bin`) is
//! exactly this header signed with the RFC 8032 section 7.1 TEST 1 key
//! (Ed25519 signatures are deterministic). Regenerate with
//! `DAGON_CONFORM_WRITE_VECTORS=1 cargo test -p dagon-conform --test
//! ct07_vector` after the header layout changes.

use dagon_format::header::*;
use dagon_format::{key_id, Version};
use ed25519_compact::{KeyPair, Seed};

const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/vectors/ct07_header.bin");

fn signed() -> [u8; HEADER_SIZE] {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: McuId::of_name("stm32f401re"),
        slot_binding: slot_binding::A,
        placement: placement::INLINE,
        load_addr: 0x0802_0200,
        payload_size: 1024,
        version: Version::new(1, 1, 0),
        build_flags: build_flags::VERIFY_ENFORCED,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: [0xE9; 32],
        key_id: key_id(&kp.pk),
        target_tag: target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: capabilities::SIG_VERIFIER,
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    h.to_bytes()
}

#[test]
fn the_ct07_header_is_current() {
    let want = signed();
    if std::env::var_os("DAGON_CONFORM_WRITE_VECTORS").is_some() {
        std::fs::write(PATH, want).unwrap();
    }
    assert_eq!(
        std::fs::read(PATH).unwrap(),
        want,
        "regenerate: see the file comment"
    );
}
