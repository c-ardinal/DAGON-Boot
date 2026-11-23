//! The ESP32-S3 stage0 in the emulator: stage0's ROM image at
//! flash 0x0, a test boot core and a test app in their slots with signed
//! detached headers, and the boot core and app logs, booted in Espressif's
//! QEMU. What runs tells itself on the ROM's text output, which is
//! compared with what each case must show.
//!
//! The images come from the CI job `stage0-qemu`, which builds them and
//! names them here: `DAGON_QEMU` (qemu-system-xtensa), `DAGON_STAGE0_ELF`
//! (targets/mcu/esp32s3/stage0 with `qemu-layout`, the RFC 8032 test key
//! and the target tag [`TAG`]), `DAGON_QEMU_CORE_ELF` and
//! `DAGON_QEMU_APP_ELF` (targets/mcu/esp32s3/stage0-qemu). The layout is
//! stage0's provisional `qemu_layout`.

use super::*;
use dagon_format::header::{
    build_flags, capabilities, header_digest16, image_kind, mcu_id, placement, port_kind,
    ImageHeader, HEADER_SIZE, SIGNED_LEN, SIGN_DOMAIN,
};
use dagon_format::statelog::{kind, reason, target, FaceHeader, Record};
use dagon_format::{key_id, Version};
use ed25519_compact::{KeyPair, Seed};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The node tag stage0 is built with (`DAGON_TARGET_TAG`).
const TAG: &str = "qemu-node";
/// RFC 8032 section 7.1 TEST 1 secret key: stage0 is built with its
/// public key (`DAGON_ALLOW_TEST_KEY`).
const TEST_SECRET: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const FLASH_BYTES: usize = 16 << 20;
/// Stage0's `qemu_layout`: the boot core (factory) and APP slot A (ota_0),
/// each a detached header and a body, and the two logs' first faces.
const CORE: (u32, u32) = (0x71_2000, 0x1_0000);
const APP: (u32, u32) = (0x71_0000, 0x11_0000);
const CORE_LOG: u32 = 0x71_4000;
const APP_LOG: u32 = 0x71_8000;
const PAGE: usize = 0x1_0000;

/// What the test boot core and app say.
const ASKS: &str = "core: asks for the app";
const APP_RUNS: &str = "app: running from flash";
const RECOVERY: &str = "core: recovery mode, refusal";

fn s3() -> &'static ChipImage {
    chip("esp32s3").unwrap()
}

fn env_file(name: &str) -> Vec<u8> {
    let path = std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{name}={path}: {e}"))
}

/// The ESP image of an app as the image tool lays one out: RAM segments
/// as they come, each segment mapped from flash (IROM, DROM) placed so its
/// offset agrees with its address below the MMU page, a PADDING segment
/// (load address 0) filling the gap before it.
fn app_image(elf: &[u8]) -> Vec<u8> {
    let (entry, segs) = load_segments(elf, s3()).unwrap();
    let mapped =
        |a: u32| (0x3C00_0000..0x3E00_0000).contains(&a) || (0x4200_0000..0x4400_0000).contains(&a);
    let mut body: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut at = HEADER_LEN;
    for (addr, data) in segs {
        let mut data = data.to_vec();
        data.resize((data.len() + 3) & !3, 0);
        if mapped(addr) {
            let want = addr as usize % PAGE;
            if (at + SEGMENT_HEADER_LEN) % PAGE != want {
                let pad = (want + 2 * PAGE - (at + 2 * SEGMENT_HEADER_LEN) % PAGE) % PAGE;
                body.push((0, vec![0; pad]));
                at += SEGMENT_HEADER_LEN + pad;
            }
        }
        at += SEGMENT_HEADER_LEN + data.len();
        body.push((addr, data));
    }
    let mut out = vec![MAGIC, body.len() as u8, 2, 0x4F];
    out.extend(entry.to_le_bytes());
    out.extend([
        WP_PIN_DISABLED,
        0,
        0,
        0,
        9,
        0,
        0,
        0,
        0,
        99,
        0,
        0,
        0,
        0,
        0,
        1,
    ]);
    let mut checksum = CHECKSUM_SEED;
    for (addr, data) in &body {
        out.extend(addr.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        checksum = data.iter().fold(checksum, |c, b| c ^ b);
    }
    out.resize(out.len() + (15 - out.len() % 16), 0);
    out.push(checksum);
    let digest = sha256(&out);
    out.extend(digest);
    out
}

/// A signed detached header for `body` of kind `kind` at `load_addr`.
fn header(kind: u8, load_addr: u32, body: &[u8], tag: &str) -> [u8; HEADER_SIZE] {
    let kp = KeyPair::from_seed(Seed::new(TEST_SECRET));
    let mut h = ImageHeader {
        image_kind: kind,
        port_kind: port_kind::SELF_BOOT,
        mcu_id: mcu_id::ESP32S3,
        slot_binding: 0,
        placement: placement::DETACHED,
        load_addr,
        payload_size: body.len() as u32,
        version: Version::new(1, 0, 0),
        build_flags: build_flags::TEST_KEY,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        payload_sha256: sha256(body),
        key_id: key_id(&kp.pk),
        target_tag: dagon_format::header::target_tag(tag),
        proto_version: 1,
        capabilities: if kind == image_kind::BOOTCORE {
            capabilities::SIG_VERIFIER
        } else {
            capabilities::CONFIRM_BY_HUB
        },
        signature: [0; 64],
    };
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    h.to_bytes()
}

/// A log face naming `hdr` PROVISIONed for `target`.
fn face(t: u8, hdr: &[u8; HEADER_SIZE]) -> Vec<u8> {
    let rec = Record {
        kind: kind::PROVISION,
        target: t,
        slot: 0,
        reason: reason::NONE,
        seq: 1,
        version: Version::new(1, 0, 0),
        digest16: header_digest16(hdr),
    };
    let mut out = FaceHeader { generation: 1 }.to_bytes().to_vec();
    out.extend(rec.to_bytes());
    out
}

/// One node's flash: what each case changes of the good one.
struct Node {
    stage0: Vec<u8>,
    core: Vec<u8>,
    core_hdr: [u8; HEADER_SIZE],
    app: Vec<u8>,
    app_hdr: [u8; HEADER_SIZE],
    /// The app log names the app.
    app_named: bool,
}

impl Node {
    fn good() -> Node {
        let stage0 = build_image(&env_file("DAGON_STAGE0_ELF"), s3(), "16mb").unwrap();
        let core = build_image(&env_file("DAGON_QEMU_CORE_ELF"), s3(), "16mb").unwrap();
        let app = app_image(&env_file("DAGON_QEMU_APP_ELF"));
        Node {
            core_hdr: header(image_kind::BOOTCORE, CORE.1, &core, ""),
            app_hdr: header(image_kind::APP, APP.1, &app, TAG),
            stage0,
            core,
            app,
            app_named: true,
        }
    }

    fn flash(&self) -> Vec<u8> {
        let mut f = vec![0xFF; FLASH_BYTES];
        let mut put = |at: u32, b: &[u8]| f[at as usize..][..b.len()].copy_from_slice(b);
        put(0, &self.stage0);
        put(CORE.0, &self.core_hdr);
        put(CORE.1, &self.core);
        put(APP.0, &self.app_hdr);
        put(APP.1, &self.app);
        put(CORE_LOG, &face(target::BOOTCORE, &self.core_hdr));
        if self.app_named {
            put(APP_LOG, &face(target::APP, &self.app_hdr));
        } else {
            put(APP_LOG, &FaceHeader { generation: 1 }.to_bytes());
        }
        f
    }

    /// Boots the node in QEMU until `done` is seen in the output, or 30 s
    /// pass; the output lines from stage0's images (the ROM's own aside).
    fn boot(&self, case: &str, done: &str) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!("dagon-stage0-qemu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rel = crate::genfs::Rel::new(format!("{case}.bin")).unwrap();
        crate::genfs::write(&dir, &rel, &self.flash()).unwrap();
        let path = dir.join(rel.as_str());
        let qemu = std::env::var("DAGON_QEMU").expect("DAGON_QEMU is not set");
        let mut child = Command::new(qemu)
            .args(["-nographic", "-machine", "esp32s3", "-drive"])
            .arg(format!("file={},if=mtd,format=raw", path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("qemu starts");
        let (tx, rx) = mpsc::channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let until = Instant::now() + Duration::from_secs(30);
        let mut lines = Vec::new();
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(l) => {
                    let end = l.contains(done);
                    if l.starts_with("core:") || l.starts_with("app:") {
                        lines.push(l);
                    }
                    // A boot loop: stop early.
                    if end || lines.len() > 20 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = crate::genfs::remove(&dir, &rel);
        lines
    }
}

/// The ESP image's first segment at load address 0 (PADDING), if any.
fn has_padding(img: &[u8]) -> bool {
    let mut at = HEADER_LEN;
    (0..img[1]).any(|_| {
        let (addr, len) = (word(img, at).unwrap(), word(img, at + 4).unwrap());
        at += SEGMENT_HEADER_LEN + len as usize;
        addr == 0
    })
}

fn recovery(refusal: u8) -> String {
    format!("{RECOVERY} {refusal}")
}

#[test]
#[ignore = "needs QEMU and the images the CI job stage0-qemu builds"]
fn esp32s3_stage0_in_qemu() {
    let good = Node::good();
    assert!(
        has_padding(&good.app),
        "the app image has a PADDING segment"
    );
    let mut failed = Vec::new();
    let mut case = |name: &str, node: &Node, want: &[&str]| {
        let done = want.last().unwrap();
        let got = node.boot(name, done);
        let ok = got.len() == want.len() && got.iter().zip(want).all(|(g, w)| g.starts_with(w));
        eprintln!("{name}: {}", if ok { "ok" } else { "FAILED" });
        for l in &got {
            eprintln!("  {l}");
        }
        if !ok {
            failed.push(format!("{name}: want {want:?}, got {got:?}"));
        }
    };
    let started = "core: started, cell 00000000 00000000";
    // The boot core starts, asks; stage0 checks the app, maps it (through
    // the PADDING segment) and starts it.
    case("good", &good, &[started, ASKS, APP_RUNS]);
    // The app's signature does not verify: refused, the boot core stays.
    let mut n = Node::good();
    n.app_hdr[HEADER_SIZE - 1] ^= 1;
    let rejected = recovery(2);
    let result = "core: started, cell b95aa52d b95aa52d";
    case("bad-signature", &n, &[started, ASKS, result, &rejected]);
    // A byte of the app's code changed after signing.
    let mut n = Node::good();
    let last = n.app.len() - 40;
    n.app[last] ^= 0x40;
    case("tampered-body", &n, &[started, ASKS, result, &rejected]);
    // A signed app with a segment in stage0's window: planned, refused.
    let mut n = Node::good();
    n.app = with_segment(&n.app, 0x3FCE_0000, &[0; 16]);
    n.app_hdr = header(image_kind::APP, APP.1, &n.app, TAG);
    case(
        "out-of-range-segment",
        &n,
        &[started, ASKS, result, &rejected],
    );
    // A signed app with a RAM segment elsewhere in RTC slow memory: copied,
    // and the app starts.
    let mut n = Node::good();
    n.app = with_segment(&n.app, 0x5000_1000, &[0; 8]);
    n.app_hdr = header(image_kind::APP, APP.1, &n.app, TAG);
    case("segment-in-rtc-memory", &n, &[started, ASKS, APP_RUNS]);
    // A signed app with a RAM segment on the cell stage0 keeps across
    // resets (RTC slow memory): planned, refused, the cell unchanged.
    let mut n = Node::good();
    n.app = with_segment(&n.app, 0x5000_1FF8, &[0; 8]);
    n.app_hdr = header(image_kind::APP, APP.1, &n.app, TAG);
    case(
        "segment-on-the-cell",
        &n,
        &[started, ASKS, result, &rejected],
    );
    // A signed body shorter than the image: its last segment runs past
    // what the signature covers, so it is refused, not loaded.
    let mut n = Node::good();
    n.app_hdr = header(image_kind::APP, APP.1, &n.app[..n.app.len() - 64], TAG);
    case(
        "past-the-signed-body",
        &n,
        &[started, ASKS, result, &rejected],
    );
    // No app record: the result says so.
    let mut n = Node::good();
    n.app_named = false;
    let no_image = recovery(1);
    let none = "core: started, cell b95aa51e b95aa51e";
    case("no-app", &n, &[started, ASKS, none, &no_image]);
    // No boot core passes (its signature): the confirmed app starts.
    let mut n = Node::good();
    n.core_hdr[HEADER_SIZE - 1] ^= 1;
    case("bad-core", &n, &[APP_RUNS]);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// `img` with one more segment (`addr`, `data`) first, then padding to a
/// whole MMU page, resealed.
fn with_segment(img: &[u8], addr: u32, data: &[u8]) -> Vec<u8> {
    let body_end = {
        let mut at = HEADER_LEN;
        for _ in 0..img[1] {
            at += SEGMENT_HEADER_LEN + word(img, at + 4).unwrap() as usize;
        }
        at
    };
    let mut out = img[..HEADER_LEN].to_vec();
    out[1] += 2;
    out.extend(addr.to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    // Then a PADDING segment, so the two shift the rest by a whole MMU
    // page, which keeps every mapped segment's offset.
    let pad = PAGE - 2 * SEGMENT_HEADER_LEN - data.len();
    out.extend(0u32.to_le_bytes());
    out.extend((pad as u32).to_le_bytes());
    out.resize(out.len() + pad, 0);
    out.extend(&img[HEADER_LEN..body_end]);
    let checksum = out[HEADER_LEN..].iter().fold(CHECKSUM_SEED, |c, b| c ^ b);
    out.resize(out.len() + (15 - out.len() % 16), 0);
    out.push(checksum);
    let digest = sha256(&out);
    out.extend(digest);
    out
}
