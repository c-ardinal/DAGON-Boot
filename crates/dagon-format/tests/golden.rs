//! Golden vectors in `golden/`. Each test rebuilds the vector from fixed
//! inputs, compares it byte for byte with the file, then parses and verifies
//! the file. Set `DAGON_REGEN_GOLDEN=1` to rewrite the files instead.
//!
//! The signing key is the RFC 8032 section 7.1 "TEST 1" key (public test
//! data, never a release key); images carry the TEST_KEY build flag.

use dagon_format::bundle::{self, dest, BundleItem, BundleView, NO_ENTRY};
use dagon_format::header::{self, *};
use dagon_format::manifest::{self, ManifestEntry, ManifestFields, ManifestView};
use dagon_format::statelog::{self, kind, reason, FaceHeader, Record};
use dagon_format::{crc32, key_id, sha256, FormatError, Version};
use ed25519_compact::{KeyPair, Seed};
use std::path::PathBuf;

const TEST_SEED: [u8; 32] =
    hex32("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
const TEST_PUBKEY: [u8; 32] =
    hex32("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");

const fn hex32(s: &str) -> [u8; 32] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            _ => c - b'a' + 10,
        }
    }
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nib(b[2 * i]) << 4 | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

fn sign(domain: &[u8], msg: &[u8]) -> [u8; 64] {
    let kp = KeyPair::from_seed(Seed::new(TEST_SEED));
    *kp.sk.sign([domain, msg].concat(), None)
}

fn golden(name: &str, built: &[u8]) -> Vec<u8> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "golden", name]
        .iter()
        .collect();
    if std::env::var_os("DAGON_REGEN_GOLDEN").is_some() {
        std::fs::write(&path, built).unwrap();
    }
    let file = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        file == built,
        "{name} differs from the bytes built from fixed inputs"
    );
    file
}

fn body(slot: u8) -> Vec<u8> {
    (0..1024u32)
        .map(|i| (i * 7 + 3 + slot as u32 * 0x40) as u8)
        .collect()
}

/// Signed F401 APP image (header + body) for `slot`.
fn image(slot: u8) -> Vec<u8> {
    let body = body(slot);
    let slot_base = [0x0802_0000u32, 0x0804_0000][slot as usize];
    let mut h = ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::STM32F401XE,
        slot_binding: slot,
        placement: placement::INLINE,
        load_addr: slot_base + 0x200,
        payload_size: body.len() as u32,
        version: Version::new(1, 1, 0),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(&body),
        key_id: key_id(&TEST_PUBKEY),
        target_tag: target_tag("seg-f401-7seg"),
        proto_version: 1,
        capabilities: capabilities::CONFIRM_BY_HUB,
        signature: [0; 64],
    };
    h.signature = sign(header::SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]);
    [&h.to_bytes()[..], &body].concat()
}

/// Canonical binary of the desk demo: hub (RP2040) with seg (F401 on I2C0
/// GPIO16/17, address 0x42, NRST GPIO18, BOOT0 GPIO28) and watch (ESP32-S3
/// on PIO UART GPIO26/27 at 115200).
fn topology() -> Vec<u8> {
    use dagon_format::topology::{self as tp, *};
    let node = |index, role, mcu, port, attempts, listen, tiers, rank, tag| TopoNode {
        index,
        role,
        mcu_id: mcu,
        port_kind: port,
        max_attempts: attempts,
        listen_window_ms: listen,
        map_id: map_id::DEFAULT,
        recovery_tiers: tiers,
        order_rank: rank,
        target_tag: target_tag(tag),
    };
    let nodes = [
        node(
            0,
            tp::role::COORDINATOR,
            mcu_id::RP2040,
            port_kind::SELF_BOOT,
            3,
            0,
            [tier::LOCAL, tier::NONE, tier::NONE],
            2,
            "hub-rp2040-lcd",
        ),
        node(
            1,
            tp::role::NODE,
            mcu_id::STM32F401XE,
            port_kind::SELF_BOOT,
            3,
            50,
            [tier::LOCAL, tier::REDISTRIBUTE, tier::ROM_FORCED],
            0,
            "seg-f401-7seg",
        ),
        node(
            2,
            tp::role::NODE,
            mcu_id::ESP32S3,
            port_kind::VENDOR_STAGE2,
            1,
            0,
            [tier::LOCAL, tier::REDISTRIBUTE, tier::ROM_COOP],
            1,
            "watch-s3",
        ),
    ];
    let links = [
        TopoLink {
            from: 0,
            to: 1,
            iface: iface::I2C_BUS,
            control: control::FULL,
            tx_pin: 16,
            rx_pin: 17,
            nrst_pin: 18,
            boot_pin: 28,
            baud: 400_000,
            rom_entry: rom_entry::STM32_I2C_ROM,
            i2c_addr: 0x42,
            bus_id: 0,
        },
        TopoLink {
            from: 0,
            to: 2,
            iface: iface::PIO_UART,
            control: control::COOPERATIVE,
            tx_pin: 26,
            rx_pin: 27,
            nrst_pin: tp::NONE,
            boot_pin: tp::NONE,
            baud: 115_200,
            rom_entry: rom_entry::FORCE_DOWNLOAD_BOOT,
            i2c_addr: tp::NONE,
            bus_id: tp::NONE,
        },
    ];
    let mut out = vec![0u8; tp::MAX_SIZE];
    let n = tp::encode(&nodes, &links, &[], &mut out).unwrap();
    out.truncate(n);
    out
}

/// Asserts the planned field offsets directly, independent of the encoder.
fn expect_at(file: &[u8], table: &[(usize, &[u8])]) {
    for (off, bytes) in table {
        assert_eq!(
            &file[*off..off + bytes.len()],
            *bytes,
            "bytes at {off:#05x}"
        );
    }
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

fn manifest_bytes(images: &[Vec<u8>]) -> Vec<u8> {
    let entries: Vec<ManifestEntry> = images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let raw: [u8; HEADER_SIZE] = img[..HEADER_SIZE].try_into().unwrap();
            let h = ImageHeader::parse(&raw).unwrap();
            ManifestEntry {
                node_index: 1,
                image_kind: h.image_kind,
                slot_binding: h.slot_binding,
                flags: manifest::entry_flags::REQUIRED,
                version: h.version,
                payload_size: h.payload_size,
                proto_version: h.proto_version,
                header_sha256: header_digest(&raw),
                order_rank: i as u8,
            }
        })
        .collect();
    let fields = ManifestFields {
        release_seq: 1,
        topology_sha256: sha256(&topology()),
        release_name: target_tag("release-1.1"),
        key_id: key_id(&TEST_PUBKEY),
    };
    let mut out = vec![0u8; manifest::total_size(entries.len(), 0)];
    let n = manifest::encode(&fields, &entries, &[], &mut out).unwrap();
    let body = n - manifest::SIG_SIZE;
    let sig = sign(manifest::SIGN_DOMAIN, &out[..body]);
    out[body..].copy_from_slice(&sig);
    out
}

fn release() -> Vec<u8> {
    let images = [image(0), image(1)];
    let man = manifest_bytes(&images);
    let topo = topology();
    let parts: [(&[u8], u8, u8); 3] = [
        (&images[0], 0, dest::SUB_STORE),
        (&images[1], 1, dest::SUB_STORE),
        (&topo, NO_ENTRY, dest::TOPOLOGY),
    ];
    let mut off = bundle::head_size(parts.len());
    let man_off = off;
    off += man.len().next_multiple_of(4);
    let mut items = Vec::new();
    for (bytes, entry_index, d) in parts {
        items.push(BundleItem {
            entry_index,
            dest: d,
            offset: off as u32,
            length: bytes.len() as u32,
            crc32: crc32(bytes),
        });
        off += bytes.len().next_multiple_of(4);
    }
    let mut out = vec![0u8; off];
    bundle::encode_head(&items, man_off as u32, man.len() as u32, &mut out).unwrap();
    out[man_off..man_off + man.len()].copy_from_slice(&man);
    for (it, (bytes, _, _)) in items.iter().zip(parts) {
        out[it.offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
    out
}

#[test]
fn test_key_is_rfc8032_test1() {
    let kp = KeyPair::from_seed(Seed::new(TEST_SEED));
    assert_eq!(*kp.pk, TEST_PUBKEY);
}

#[test]
fn image_header_vector() {
    let file = golden("image_f401_app_a.img", &image(0));
    let raw: [u8; HEADER_SIZE] = file[..HEADER_SIZE].try_into().unwrap();
    let h = verify_header(&raw, &TEST_PUBKEY).unwrap();
    assert_eq!(h.to_bytes(), raw, "parse/serialise round trip");
    assert_eq!(h.version, Version::new(1, 1, 0));
    assert_eq!(
        &raw[0x018..0x01C],
        &[0x00, 0x00, 0x01, 0x01],
        "version u32 LE at 0x018"
    );
    assert_eq!(h.payload_sha256, sha256(&file[HEADER_SIZE..]));
    let kid = key_id(&TEST_PUBKEY);
    let sha = sha256(&file[HEADER_SIZE..]);
    expect_at(
        &file,
        &[
            (0x000, b"DGIM"),
            (0x004, &[1, 0]),                   // hdr_version
            (0x006, &[0x00, 0x02]),             // hdr_size 512
            (0x008, &[0x01, 0x01]),             // image_kind APP, port_kind SELF_BOOT
            (0x00A, &[0x02, 0x00]),             // mcu_id STM32F401xE
            (0x00C, &[0x00, 0x00, 0x00, 0x00]), // slot A, inline, reserved
            (0x010, &le32(0x0802_0200)),        // load_addr
            (0x014, &le32(1024)),               // payload_size
            (0x018, &[0x00, 0x00, 0x01, 0x01]), // version 1.1.0
            (0x01C, &le32(8)),                  // build_flags TEST_KEY
            (0x020, &le32(1)),                  // release_seq
            (0x024, &[0x00, 0x00, 0x00, 0x01]), // min_bootcore_version 1.0.0
            (0x028, &sha),
            (0x048, &kid),
            (0x050, b"seg-f401-7seg\0\0\0"),
            (0x060, &le32(1)), // proto_version
            (0x064, &le32(1)), // capabilities CONFIRM_BY_HUB
            (0x068, &[0; 0x1C0 - 0x068]),
        ],
    );

    for bit_at in [0x018, 0x050, 0x1C0] {
        let mut bad = raw;
        bad[bit_at] ^= 1;
        assert_eq!(
            verify_header(&bad, &TEST_PUBKEY),
            Err(FormatError::BadSignature)
        );
    }
    let mut bad = raw;
    bad[0x048] ^= 1;
    assert_eq!(
        verify_header(&bad, &TEST_PUBKEY),
        Err(FormatError::UnknownKey)
    );
}

#[test]
fn manifest_vector() {
    let file = golden("manifest.bin", &manifest_bytes(&[image(0), image(1)]));
    let m = ManifestView::parse(&file).unwrap();
    m.verify(&TEST_PUBKEY).unwrap();
    assert_eq!(m.entry_count(), 2);
    assert_eq!(m.bytes().len(), manifest::total_size(2, 0));
    let e = m.entry(1);
    assert_eq!(e.slot_binding, 1);
    let hdr_a = header_digest(&image(0)[..HEADER_SIZE].try_into().unwrap());
    expect_at(
        &file,
        &[
            (0x00, b"DGMF"),
            (0x04, &[1, 0]),      // mf_version
            (0x06, &[2, 0]),      // entry_count, constraint_count
            (0x08, &le32(1)),     // release_seq
            (0x0C, &le32(0x110)), // total_size 0x50 + 2*64 + 64
            (0x10, &sha256(&topology())),
            (0x30, b"release-1.1\0\0\0\0\0"),
            (0x40, &key_id(&TEST_PUBKEY)),
            (0x48, &[0; 8]),
            // entry 0: node 1, APP, slot A, REQUIRED, 1.1.0, 1024 B, proto 1
            (0x50, &[1, 1, 0, 1, 0x00, 0x00, 0x01, 0x01]),
            (0x58, &le32(1024)),
            (0x5C, &le32(1)),
            (0x60, &hdr_a),
            (0x80, &[0; 16]),      // order_rank 0, reserved
            (0x90, &[1, 1, 1, 1]), // entry 1 slot B
            (0xC0, &[1]),          // entry 1 order_rank
        ],
    );
    let mut out = vec![0u8; file.len()];
    manifest::encode(&m.fields(), &[m.entry(0), e], &[], &mut out).unwrap();
    assert_eq!(
        out[..file.len() - 64],
        file[..file.len() - 64],
        "round trip"
    );

    let mut bad = file.clone();
    bad[0x08] ^= 1;
    assert_eq!(
        ManifestView::parse(&bad).unwrap().verify(&TEST_PUBKEY),
        Err(FormatError::BadSignature)
    );
}

#[test]
fn release_bundle_vector() {
    let file = golden("release.dgb", &release());
    let b = BundleView::parse(&file).unwrap();
    b.verify(&TEST_PUBKEY).unwrap();
    assert_eq!(b.item_count(), 3);
    let t = b.topology_item().unwrap();
    let it = b.item(t);
    assert_eq!((it.dest, it.entry_index), (dest::TOPOLOGY, NO_ENTRY));
    assert_eq!(it.length as usize, topology().len());
    assert_eq!(b.item_bytes(t), &topology()[..]);
    let topo_len = topology().len() as u32;
    expect_at(
        &file,
        &[
            (0x00, b"DGBN"),
            (0x04, &[1, 0, 3, 0]), // bn_version, item_count, reserved
            (0x08, &le32(0x40)),   // manifest_off = 16 + 3*16
            (0x0C, &le32(0x110)),  // manifest_len
            // items: entry_index, dest, reserved u16, offset, length, crc32
            (0x10, &[0, 2, 0, 0]),
            (0x14, &le32(0x150)),
            (0x18, &le32(0x600)),
            (0x1C, &le32(crc32(&image(0)))),
            (0x20, &[1, 2, 0, 0]),
            (0x24, &le32(0x750)),
            (0x30, &[0xFF, 3, 0, 0]), // topology: NO_ENTRY, dest 3
            (0x34, &le32(0xD50)),
            (0x38, &le32(topo_len)),
            (0x3C, &le32(crc32(&topology()))),
        ],
    );

    // Tampered body: CRC caught first; with the CRC patched, the SHA-256 chain.
    let body_at = b.item(0).offset as usize + HEADER_SIZE;
    let mut bad = file.clone();
    bad[body_at] ^= 1;
    assert_eq!(
        BundleView::parse(&bad).unwrap().verify(&TEST_PUBKEY),
        Err(FormatError::BadCrc)
    );
    let new_crc = crc32(BundleView::parse(&bad).unwrap().item_bytes(0));
    bad[bundle::HEAD_SIZE + 12..][..4].copy_from_slice(&new_crc.to_le_bytes());
    assert_eq!(
        BundleView::parse(&bad).unwrap().verify(&TEST_PUBKEY),
        Err(FormatError::DigestMismatch)
    );

    // Topology that no longer matches the manifest.
    let mut bad = file.clone();
    bad[it.offset as usize] ^= 1;
    let new_crc = crc32(BundleView::parse(&bad).unwrap().item_bytes(t));
    bad[bundle::HEAD_SIZE + bundle::ITEM_SIZE * t + 12..][..4]
        .copy_from_slice(&new_crc.to_le_bytes());
    assert_eq!(
        BundleView::parse(&bad).unwrap().verify(&TEST_PUBKEY),
        Err(FormatError::DigestMismatch)
    );
}

fn state_log() -> Vec<u8> {
    let img = image(0);
    let d16 = header_digest16(&img[..HEADER_SIZE].try_into().unwrap());
    let v = Version::new(1, 1, 0);
    let rec = |kind, slot, seq| Record {
        kind,
        target: statelog::target::APP,
        slot,
        reason: reason::NONE,
        seq,
        version: v,
        digest16: d16,
    };
    let mut out = FaceHeader { generation: 1 }.to_bytes().to_vec();
    for r in [
        rec(kind::PROVISION, 0, 1),
        rec(kind::CONFIRMED, 0, 2),
        rec(kind::SLOT_PENDING, 1, 3),
        rec(kind::BOOT_ATTEMPT, 1, 4),
        Record {
            reason: reason::TRIAL_EXHAUSTED,
            ..rec(kind::ROLLBACK, 0, 5)
        },
    ] {
        out.extend_from_slice(&r.to_bytes());
    }
    out.extend_from_slice(&[0xFF; statelog::RECORD_SIZE]);
    out
}

#[test]
fn state_log_vector() {
    let file = golden("statelog_face.bin", &state_log());
    let fh = FaceHeader::parse(&file[..32].try_into().unwrap()).unwrap();
    assert_eq!(fh.generation, 1);
    let d16 = &header_digest(&image(0)[..HEADER_SIZE].try_into().unwrap())[..16];
    expect_at(
        &file,
        &[
            (0x00, b"DGSL"),
            (0x04, &[1, 0, 32, 0]), // format_version, record_size
            (0x08, &le32(1)),       // generation
            (0x0C, &[0xFF; 16]),    // reserved stays erased
            (0x1C, &le32(crc32(&file[..0x1C]))),
            // record 0: PROVISION, APP, slot A, no reason, seq 1, 1.1.0
            (0x20, &[0x08, 0, 0, 0]),
            (0x24, &le32(1)),
            (0x28, &[0x00, 0x00, 0x01, 0x01]),
            (0x2C, d16),
            (0x3C, &le32(crc32(&file[0x20..0x3C]))),
            (0xA0, &[0x04, 0, 0, 0x03]), // record 4: ROLLBACK to A, TRIAL_EXHAUSTED
            (0xC0, &[0xFF; 32]),         // end of log
        ],
    );
    let recs: Vec<&[u8; 32]> = file[32..]
        .chunks(32)
        .map(|c| c.try_into().unwrap())
        .collect();
    let parsed: Vec<Record> = recs
        .iter()
        .take_while(|r| !Record::is_erased(r))
        .map(|r| Record::parse(r).unwrap())
        .collect();
    assert_eq!(parsed.len(), 5);
    assert_eq!(parsed[4].kind, kind::ROLLBACK);
    assert_eq!(parsed[4].reason, reason::TRIAL_EXHAUSTED);
    for (r, raw) in parsed.iter().zip(&recs) {
        assert_eq!(&r.to_bytes(), *raw);
    }
    let mut torn = *recs[3];
    torn[8] ^= 1;
    assert_eq!(Record::parse(&torn), Err(FormatError::BadCrc));
}

#[test]
fn topology_desk_demo_vector() {
    use dagon_format::topology::{self as tp, TopologyView};
    let file = golden("topology_desk_demo.bin", &topology());
    let t = TopologyView::parse(&file).unwrap();
    assert_eq!(
        (t.node_count(), t.link_count(), t.constraint_count()),
        (3, 2, 0)
    );
    assert_eq!(t.link(0).i2c_addr, 0x42);
    assert_eq!(t.node(2).target_tag, target_tag("watch-s3"));
    expect_at(
        &file,
        &[
            (0x00, b"DGTP"),
            (0x04, &[1, 0, 3, 2]), // tp_version, node_count, link_count
            (0x08, &le32(0xA0)),   // body_size = whole binary
            (0x0C, &[0; 20]),      // unused key_id, constraint_count 0, reserved
            // nodes: index, role, mcu_id, port_kind, max_attempts,
            // listen_window_ms, map_id, recovery_tiers[3], order_rank, reserved[3], tag
            (
                0x20,
                &[0, 0, 0x01, 0x00, 1, 3, 0, 0, 0, 1, 0, 0, 2, 0, 0, 0],
            ),
            (0x30, b"hub-rp2040-lcd\0\0"),
            (
                0x40,
                &[1, 1, 0x02, 0x00, 1, 3, 50, 0, 0, 1, 2, 3, 0, 0, 0, 0],
            ),
            (0x50, b"seg-f401-7seg\0\0\0"),
            (
                0x60,
                &[2, 1, 0x03, 0x00, 2, 1, 0, 0, 0, 1, 2, 4, 1, 0, 0, 0],
            ),
            (0x70, b"watch-s3\0\0\0\0\0\0\0\0"),
            // links: from, to, iface, control, tx/SDA, rx/SCL, nrst, boot,
            // baud u32, rom_entry, i2c_addr, bus_id, reserved
            (
                0x80,
                &[
                    0, 1, 3, 1, 16, 17, 18, 28, 0x80, 0x1A, 0x06, 0x00, 3, 0x42, 0, 0,
                ],
            ),
            (
                0x90,
                &[
                    0, 2, 2, 2, 26, 27, 0xFF, 0xFF, 0x00, 0xC2, 0x01, 0x00, 1, 0xFF, 0xFF, 0,
                ],
            ),
        ],
    );
    assert_eq!(file.len(), 0xA0);

    // Non-canonical inputs are rejected both ways.
    let mut bad = file.clone();
    bad[0x0D] = 1; // reserved head byte
    assert_eq!(TopologyView::parse(&bad).err(), Some(FormatError::BadField));
    let mut bad = file.clone();
    bad[0x2D] = 1; // reserved node byte
    assert_eq!(TopologyView::parse(&bad).err(), Some(FormatError::BadField));
    let (n0, n1) = (t.node(0), t.node(1));
    let mut out = [0u8; tp::MAX_SIZE];
    assert_eq!(
        tp::encode(&[n1, n0], &[], &[], &mut out),
        Err(FormatError::BadField)
    );
    assert_eq!(
        tp::encode(&[n0, n1, t.node(2)], &[t.link(1), t.link(0)], &[], &mut out),
        Err(FormatError::BadField)
    );
}
