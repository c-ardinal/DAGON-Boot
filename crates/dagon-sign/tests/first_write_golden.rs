//! Slot geometry and first-install outputs pinned byte for byte (SHA-256),
//! recorded before slots and layouts moved into the MCU definitions. Any
//! change to an address, a slot or a provision layout fails here.

use dagon_format::header::{image_kind, slot_binding, target_tag};
use dagon_format::{sha256, Version};
use dagon_sign::image::{self, sign_image, ImageSpec, Slot};
use dagon_sign::{provision, to_hex};
use ed25519_compact::{KeyPair, Seed};

/// The first-install file of a builtin MCU (its writer takes one file).
fn first_file(
    mcu: &str,
    stage0: Option<&[u8]>,
    boot: (&image::Slot, &[u8]),
    app: (&image::Slot, &[u8]),
) -> Vec<u8> {
    let def = dagon_topology::defs::Registry::builtin().mcu(mcu).unwrap();
    provision::first_install_file(def, stage0, boot, app).unwrap()
}

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

const SLOTS: [SlotRow; 10] = [
    ("rp2040", 2, 0, [0x1000_8000, 0x1000_8200, 0x1_FE00], 0, 0),
    ("rp2040", 2, 1, [0x1002_8000, 0x1002_8200, 0x1_FE00], 0, 1),
    ("rp2040", 1, 0, [0x1004_8000, 0x1004_8200, 0xF_FE00], 0, 0),
    ("rp2040", 1, 1, [0x1014_8000, 0x1014_8200, 0xF_FE00], 0, 1),
    (
        "stm32f401re",
        2,
        0,
        [0x0801_0000, 0x0801_0200, 0xFE00],
        0,
        0,
    ),
    (
        "stm32f401re",
        2,
        1,
        [0x0806_0000, 0x0806_0200, 0x1_FE00],
        0,
        1,
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
const IMAGE_SHA256: [&str; 10] = [
    "a8b386c1c73c9ba707887f58b362f6b817956d03ed5ccd5f8afd72e1a56cad64",
    "67c2440db1b05f70c15d92f5d09631eae0d497a4229ebebfabbe88588e29b569",
    "ef74e3ebec6db1b411471092654e6570a65cc99f61e025c010b563ec933de7f1",
    "0398d1141383d13130aa4c5455b8890e3b564bae042639c19a2bf58a5ff2baff",
    "03aab2f1534d3b7ec198f08ac5c5982670da41086acc88322a46e3e35292cba0",
    "a378c5031c4cd1b58bf6acfc12f8148ab84f9a0759c203fa1a66f92d05119b06",
    "4d396faa0dbd5068055a5954c6b88747e8ce71561cfec8271639cc5da2052ac5",
    "096981f741ef77cac2ab40fd45c5385f5ce95f573035d6aca19920fbe78d8add",
    "a1829d2921be1cb3e7c8968e6aff357f598129b40a6fa94aadb64a3fc197a2ff",
    "f37c0cf5615aa6349193aa61e9c3ffaf3104b0147e3b2467a571122909e88f00",
];
/// F401 flash.bin (stage0, one state log for both targets, boot core A in
/// sector 4, APP-A): 133120 bytes.
const F401_BIN_SHA256: &str = "3fea1467341eeb816d19af5bb2ffa6fc73eaaede153680b946e5e439f455a1c9";

const F401_BIN_LEN: usize = 133_120;
/// RP2040 rescue.uf2: 81920 bytes (160 blocks).
const RP2040_UF2_SHA256: &str = "ea7fb46e7ff57dc9e80b43b0d10859815533ba42337dd585478c906ecadffb6c";
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

/// The engines need exactly what they declare to the definition loader.
#[test]
fn engines_match_their_declared_requirements() {
    use dagon_format::header::HEADER_SIZE;
    use dagon_format::statelog::target;
    use dagon_topology::defs::requirements;
    let (_, img) = signed("rp2040", image_kind::APP, slot_binding::A);
    let face = provision::initial_log(target::APP, &img);
    assert_eq!(face.len() as u32, requirements::LOG_FACE_MIN);
    assert_eq!(HEADER_SIZE as u32, requirements::HEADER);
    // One UF2 block carries one FlashImage page.
    let mut f = provision::FlashImage::default();
    f.put(0x100, &[1]);
    let uf2 = f.to_uf2_with(1, requirements::PAGE);
    assert_eq!(uf2.len(), 512);
    assert_eq!(&uf2[16..20], &requirements::PAGE.to_le_bytes());
}

/// The first write of an MCU that relays starts its app log's peer
/// records with PEERS_KEPT, after the slot's records; an MCU that does
/// not relay gets none.
#[test]
fn the_first_write_of_a_relaying_mcu_starts_its_peer_records() {
    use dagon_format::statelog::{kind, FaceHeader, Record, FACE_HEADER_SIZE, RECORD_SIZE};
    let reg = dagon_topology::defs::Registry::builtin();
    let kinds_of = |mcu: &str, stage0: usize| {
        let def = reg.mcu(mcu).unwrap();
        let (bs, boot) = signed(mcu, image_kind::BOOTCORE, slot_binding::A);
        let (as_, app) = signed(mcu, image_kind::APP, slot_binding::A);
        let f = provision::first_install(
            def,
            Some(&payload(stage0, 0x5A)),
            (&bs, &boot),
            (&as_, &app),
        )
        .unwrap();
        let log = def.layout.app_log.unwrap();
        let base = def.flash_maps[0].base;
        let face = &f.to_bin(base)[(log.faces[0] - base) as usize..][..log.face_size as usize];
        FaceHeader::parse(face[..FACE_HEADER_SIZE].try_into().unwrap()).unwrap();
        face[FACE_HEADER_SIZE..]
            .chunks_exact(RECORD_SIZE)
            .map(|r| <&[u8; RECORD_SIZE]>::try_from(r).unwrap())
            .take_while(|r| !Record::is_erased(r))
            .map(|r| Record::parse(r).unwrap())
            .filter(|r| r.target == dagon_format::statelog::target::APP)
            .map(|r| (r.kind, r.seq))
            .collect::<Vec<_>>()
    };
    assert!(!reg.mcu("rp2040").unwrap().relay_links.is_empty());
    assert_eq!(
        kinds_of("rp2040", 0x3F00),
        [
            (kind::PROVISION, 1),
            (kind::CONFIRMED, 2),
            (kind::PEERS_KEPT, 3)
        ]
    );
    assert!(reg.mcu("stm32f401re").unwrap().relay_links.is_empty());
    let f401: Vec<u8> = kinds_of("stm32f401re", 0x3000)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(f401, [kind::PROVISION, kind::CONFIRMED]);
}

#[test]
#[should_panic(expected = "UF2 builder's requirement")]
fn uf2_builder_refuses_a_sector_its_requirement_rejects() {
    provision::FlashImage::default().to_uf2_with(1, 0x80);
}

#[test]
#[should_panic(expected = "ends at 2^32")]
fn uf2_builder_refuses_a_sector_ending_at_2_pow_32() {
    let mut f = provision::FlashImage::default();
    f.put(0xFFFF_F000, &[1]);
    f.to_uf2_with(1, 0x1000);
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
    assert!(one("stm32f401re", None)
        .unwrap_err()
        .contains("needs stage0"));
    assert!(one("esp32s3", Some(&[0; 4]))
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
    let bin = first_file(
        "stm32f401re",
        Some(&payload(0x3000, 0xA5)),
        (&bs, &boot),
        (&as_, &app),
    );

    let (bs, boot) = signed("rp2040", image_kind::BOOTCORE, slot_binding::A);
    let (as_, app) = signed("rp2040", image_kind::APP, slot_binding::A);
    let uf2 = first_file(
        "rp2040",
        Some(&payload(0x3F00, 0x5A)),
        (&bs, &boot),
        (&as_, &app),
    );
    let got = (images, hex(&bin), bin.len(), hex(&uf2), uf2.len());
    println!("{got:#?}");
    assert_eq!(got.0, IMAGE_SHA256);
    assert_eq!((got.1.as_str(), got.2), (F401_BIN_SHA256, F401_BIN_LEN));
    assert_eq!((got.3.as_str(), got.4), (RP2040_UF2_SHA256, RP2040_UF2_LEN));
}
