//! The committed F401 boot core the page's ROM-path tests use is exactly
//! what signing produces with the RFC 8032 section 7.1 TEST 1 key (Ed25519
//! signatures are deterministic), and dagon-host accepts it. Regenerate with
//! `DAGON_WASM_WRITE_FIXTURES=1 cargo test -p dagon-wasm --test fixtures`.

use dagon_format::header::{image_kind, target_tag};
use dagon_format::Version;
use dagon_sign::image::{self, sign_image, ImageSpec, TEST_PUBLIC_KEY};
use ed25519_compact::{KeyPair, Seed};

const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/f401-bootcore-1.0.0.img"
);

fn bootcore() -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::BOOTCORE);
    let mcu = "stm32f401re";
    let spec = ImageSpec {
        image_kind: image_kind::BOOTCORE,
        port_kind: image::port_kind_of("self-boot").unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: image::slot(mcu, image_kind::BOOTCORE, 0).unwrap(),
        version: Version::new(1, 0, 0),
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(0, 0, 0),
        target_tag: target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: caps,
    };
    sign_image(&spec, &[0xB0; 2000], &KeyPair::from_seed(Seed::new(SEED))).unwrap()
}

#[test]
fn f401_bootcore_fixture_is_current() {
    let want = bootcore();
    if std::env::var_os("DAGON_WASM_WRITE_FIXTURES").is_some() {
        std::fs::write(PATH, &want).unwrap();
    }
    assert_eq!(
        std::fs::read(PATH).unwrap(),
        want,
        "regenerate: see the file comment"
    );
    dagon_host::rom::check_bootcore(&want, &TEST_PUBLIC_KEY).unwrap();
}
