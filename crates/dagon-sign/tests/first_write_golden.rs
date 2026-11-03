//! Slot geometry and first-install outputs pinned byte for byte (SHA-256),
//! recorded before slots and layouts moved into the MCU definitions. Any
//! change to an address, a slot or a provision layout fails here.

use dagon_format::header::{image_kind, slot_binding, target_tag};
use dagon_format::{sha256, Version};
use dagon_sign::image::{self, sign_image, ImageSpec, Slot};
use dagon_sign::{provision, to_hex};
use ed25519_compact::{KeyPair, Seed};

/// RFC 8032 section 7.1 "TEST 1" seed (public test data).
const TEST_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

const MCUS: [(&str, &str); 3] = [
    ("rp2040", "self-boot"),
    ("stm32f401re", "self-boot"),
    ("esp32s3", "vendor-stage2"),
];

/// mcu, kind, slot, [header_addr, load_addr, max_payload], placement, binding.
type SlotRow = (&'static str, u8, u8, [u32; 3], u8, u8);

const SLOTS: [SlotRow; 9] = [
    ("rp2040", 2, 0, [0x1000_8000, 0x1000_8200, 0x1_FE00], 0, 0),
    ("rp2040", 2, 1, [0x1002_8000, 0x1002_8200, 0x1_FE00], 0, 1),
    ("rp2040", 1, 0, [0x1004_8000, 0x1004_8200, 0xF_FE00], 0, 0),
    ("rp2040", 1, 1, [0x1014_8000, 0x1014_8200, 0xF_FE00], 0, 1),
    (
        "stm32f401re",
        2,
        0,
        [0x0800_7E00, 0x0800_0000, 0x7E00],
        2,
        0,
    ),
    (
        "stm32f401re",
        1,
        0,
        [0x0802_0000, 0x0802_0200, 0x1_FE00],
        0,
        0,
    ),
    (
        "stm32f401re",
        1,
        1,
        [0x0804_0000, 0x0804_0200, 0x1_FE00],
        0,
        1,
    ),
    ("esp32s3", 1, 0, [0x71_0000, 0x11_0000, 0x30_0000], 1, 0xFF),
    ("esp32s3", 3, 0, [0x71_2000, 0x1_0000, 0x10_0000], 1, 0xFF),
];

/// SHA-256 of each signed image, in `SLOTS` order.
const IMAGE_SHA256: [&str; 9] = [
    "9bc167e2b4ce2d8748ab5176dbd593e49660177cb10d19c7face2d31141e98cd",
    "45efbf2ff884dcfcc800d77969e6474d704321597e45426ea77e4883b2798ecc",
    "0fbf0acc6a4842b8fb658a6a901d6bd6606f6ef46edc70e9ca0ab21635017c0e",
    "455bcaa34d1174314cdd6794bbfd96bcb9d021615bd3b612a64859ddd2bb7396",
    "1b8c15efeb6bf5cfecd43fb4a735e6a4e4f4654e21bc262dd2186389a119835e",
    "c2a364d9ead164338b1a31f0eb77f48dff34a76d38bdd9f56b88e95d1c0fb622",
    "bcb4e5d10aa41e62c3cba546d965eaaa3a59e50e9c94fcf59ba2b73c1ea7e4cf",
    "b8a7614b2de0d1393189121b272ad9147c068e192d03fcdaf57995a9522d6af0",
    "d35b69b885455d466565b721d49e60d00c468fbd23255db87bc766e766a126a8",
];
/// F401 flash.bin: 133120 bytes.
const F401_BIN_SHA256: &str = "165128c72cbe402b472a0145f16b1ce22c3c94889f0cf4ecea8a77ba190fd2ea";
const F401_BIN_LEN: usize = 133_120;
/// RP2040 rescue.uf2: 81920 bytes (160 blocks).
const RP2040_UF2_SHA256: &str = "1236d745960c7709c20185f7d269ad5d5eb5599beed5bfd03ec75ade75e4b113";
const RP2040_UF2_LEN: usize = 81_920;

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new(TEST_SEED))
}

fn payload(n: usize, salt: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(13) ^ salt).collect()
}

fn signed(mcu: &str, kind: u8, slot: u8) -> (Slot, Vec<u8>) {
    let port = MCUS.iter().find(|(m, _)| *m == mcu).unwrap().1;
    let s = image::slot(mcu, kind, slot).unwrap();
    let (flags, caps) = image::default_bits(kind);
    let spec = ImageSpec {
        image_kind: kind,
        port_kind: image::port_kind_of(port).unwrap(),
        mcu_id: image::mcu_id_of(mcu).unwrap(),
        slot: s,
        version: Version::new(1, 2, 3),
        build_flags: flags,
        release_seq: 7,
        min_bootcore_version: Version::new(1, 0, 0),
        target_tag: target_tag(mcu),
        proto_version: 1,
        capabilities: caps,
    };
    let img = sign_image(&spec, &payload(1500, kind ^ slot), &key()).unwrap();
    (s, img)
}

fn hex(b: &[u8]) -> String {
    to_hex(&sha256(b))
}

#[test]
fn every_slot_is_where_it_was() {
    for (mcu, kind, slot, [h, l, m], placement, binding) in SLOTS {
        let want = Slot {
            header_addr: h,
            load_addr: l,
            max_payload: m,
            placement,
            slot_binding: binding,
        };
        assert_eq!(
            image::slot(mcu, kind, slot),
            Some(want),
            "{mcu} {kind} {slot}"
        );
    }
    let mut found = 0;
    for (mcu, _) in MCUS {
        for kind in 0..=u8::MAX {
            for slot in 0..=u8::MAX {
                found += usize::from(image::slot(mcu, kind, slot).is_some());
            }
        }
    }
    assert_eq!(found, SLOTS.len());
    assert_eq!(image::slot("fake", image_kind::APP, slot_binding::A), None);
}

#[test]
fn first_install_follows_the_writer_and_layout_of_the_definition() {
    let reg = dagon_topology::defs::Registry::builtin();
    let (bs, boot) = signed("esp32s3", image_kind::APP, slot_binding::A);
    let one = |mcu: &str, stage0: Option<&[u8]>| {
        provision::first_install_file(reg.mcu(mcu).unwrap(), stage0, (&bs, &boot), (&bs, &boot))
    };
    assert!(one("esp32s3", None)
        .unwrap_err()
        .contains("partition by partition"));
    assert!(one("rp2040", None).unwrap_err().contains("needs stage0"));
    assert!(one("stm32f401re", Some(&[0; 4]))
        .unwrap_err()
        .contains("no stage0 region"));
}

#[test]
fn signed_images_and_first_install_files_are_byte_identical() {
    let images: Vec<String> = SLOTS
        .iter()
        .map(|(mcu, kind, slot, ..)| hex(&signed(mcu, *kind, *slot).1))
        .collect();
    let (bs, boot) = signed("stm32f401re", image_kind::BOOTCORE, slot_binding::A);
    let (as_, app) = signed("stm32f401re", image_kind::APP, slot_binding::A);
    let bin = provision::f401_flash((&bs, &boot), (&as_, &app));
    let (bs, boot) = signed("rp2040", image_kind::BOOTCORE, slot_binding::A);
    let (as_, app) = signed("rp2040", image_kind::APP, slot_binding::A);
    let uf2 = provision::hub_rescue_uf2(&payload(0x3F00, 0x5A), (&bs, &boot), (&as_, &app));
    let got = (images, hex(&bin), bin.len(), hex(&uf2), uf2.len());
    println!("{got:#?}");
    assert_eq!(got.0, IMAGE_SHA256);
    assert_eq!((got.1.as_str(), got.2), (F401_BIN_SHA256, F401_BIN_LEN));
    assert_eq!((got.3.as_str(), got.4), (RP2040_UF2_SHA256, RP2040_UF2_LEN));
}
