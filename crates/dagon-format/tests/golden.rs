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

/// Opaque bytes standing in for a topology canonical binary.
fn topology() -> Vec<u8> {
    (0..96u8).map(|i| i ^ 0x5A).collect()
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
    assert_eq!(b.item_bytes(t), &golden("topology.bin", &topology())[..]);

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
