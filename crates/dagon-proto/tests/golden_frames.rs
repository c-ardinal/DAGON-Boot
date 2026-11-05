//! Frame golden vectors in `crates/dagon-format/golden/frame_*.bin`. Each is
//! rebuilt from fixed inputs, compared with the file, then decoded again.
//! Set `DAGON_REGEN_GOLDEN=1` to rewrite the files.

use dagon_format::Version;
use dagon_proto::payload::*;
use dagon_proto::{finish, flags, max_payload, split_status, Cmd, Frame, Status, HEADER_LEN};
use std::path::PathBuf;

fn golden(name: &str, built: &[u8]) -> Vec<u8> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "dagon-format",
        "golden",
        name,
    ]
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

/// Frame with `body` encoded by `enc`; responses get a leading OK status
/// (the zero byte left at `HEADER_LEN`).
fn frame(fl: u8, seq: u8, cmd: Cmd, enc: impl FnOnce(&mut [u8]) -> usize) -> Vec<u8> {
    let mut buf = vec![0u8; 1200];
    let at = if fl & flags::RESP != 0 {
        HEADER_LEN + 1
    } else {
        HEADER_LEN
    };
    let n = enc(&mut buf[at..]) + (at - HEADER_LEN);
    let len = finish(&mut buf, fl, seq, cmd as u8, n).unwrap();
    buf.truncate(len);
    buf
}

fn decode(file: &[u8]) -> Frame<'_> {
    let (f, used) = Frame::decode(file, max_payload::USB).unwrap();
    assert_eq!(used, file.len());
    f
}

const KEY_ID: [u8; 8] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];

fn hello_resp() -> HelloResp {
    HelloResp {
        proto_ver: 1,
        role: role::NODE,
        port_kind: 1,
        mcu_id: 2,
        mode: mode::APP,
        active_version: Version::new(1, 1, 0),
        trial: true,
        key_id: KEY_ID,
        max_payload: max_payload::I2C as u16,
    }
}

fn hello_req_frame() -> Vec<u8> {
    let req = HelloReq {
        proto_ver: 1,
        max_payload: max_payload::USB as u16,
    };
    frame(0, 0x01, Cmd::Hello, |b| req.encode(b).unwrap())
}

#[test]
fn hello_vectors() {
    let file = golden("frame_hello_req.bin", &hello_req_frame());
    let f = decode(&file);
    assert_eq!((f.seq, f.cmd), (0x01, Cmd::Hello as u8));
    assert_eq!(HelloReq::parse(f.payload).unwrap().max_payload, 1088);
    // SOF, ver, flags, seq, cmd, len u16; proto_ver u32, max_payload u16; CRC
    expect_at(
        &file,
        &[(
            0,
            &[0xDA, 0x6F, 1, 0, 1, 0x01, 6, 0, 1, 0, 0, 0, 0x40, 0x04],
        )],
    );
    assert_crc(&file);

    let file = golden(
        "frame_hello_resp.bin",
        &frame(flags::RESP, 0x01, Cmd::Hello, |b| {
            hello_resp().encode(b).unwrap()
        }),
    );
    let f = decode(&file);
    let (st, body) = split_status(f.payload).unwrap();
    assert_eq!(st, Status::Ok);
    assert_eq!(HelloResp::parse(body).unwrap(), hello_resp());
    expect_at(
        &file,
        &[
            (0, &[0xDA, 0x6F, 1, flags::RESP, 1, 0x01, 25, 0]),
            (8, &[0x00]),                    // status OK
            (9, &[1, 0, 0, 0]),              // proto_ver
            (13, &[1, 1, 0x02, 0x00]),       // role node, port_kind S, mcu_id F401
            (17, &[0]),                      // mode app
            (18, &[0x00, 0x00, 0x01, 0x01]), // active_version 1.1.0
            (22, &[1]),                      // trial
            (23, &KEY_ID),
            (31, &[0x00, 0x01]), // max_payload 256
        ],
    );
    assert_crc(&file);
}

fn expect_at(file: &[u8], table: &[(usize, &[u8])]) {
    for (off, bytes) in table {
        assert_eq!(&file[*off..off + bytes.len()], *bytes, "bytes at {off}");
    }
}

/// CRC-32 over bytes 2 .. end-4, stored little-endian in the last 4 bytes.
fn assert_crc(file: &[u8]) {
    let n = file.len();
    let crc = dagon_format::crc32(&file[2..n - 4]).to_le_bytes();
    assert_eq!(file[n - 4..], crc);
}

#[test]
fn status_vector() {
    let s = StatusResp {
        slots: [
            SlotInfo {
                state: SlotState::Written,
                version: Version::new(1, 0, 0),
                digest16: [0xA0; 16],
            },
            SlotInfo {
                state: SlotState::Trying,
                version: Version::new(1, 1, 0),
                digest16: [0xB1; 16],
            },
            SlotInfo::default(),
        ],
        active: 1,
        confirmed: 0,
        attempts: 1,
        last_reason: 0,
        log_generation: 1,
    };
    let file = golden(
        "frame_status_resp.bin",
        &frame(flags::RESP, 0x02, Cmd::Status, |b| s.encode(b).unwrap()),
    );
    let (_, body) = split_status(decode(&file).payload).unwrap();
    assert_eq!(StatusResp::parse(body).unwrap(), s);
    assert_eq!(body.len(), StatusResp::LEN);
    let mut a = vec![1, 0x00, 0x00, 0x00, 0x01]; // A: written, 1.0.0
    a.extend([0xA0; 16]);
    let mut b = vec![3, 0x00, 0x00, 0x01, 0x01]; // B: trying, 1.1.0
    b.extend([0xB1; 16]);
    expect_at(
        &file,
        &[
            (6, &[72, 0]), // len = status + 71
            (8, &[0x00]),
            (9, &a), // slots are 21 bytes: state, version, digest16
            (30, &b),
            (51, &[0; 21]),      // factory empty
            (72, &[1, 0, 1, 0]), // active, confirmed, attempts, last_reason
            (76, &[1, 0, 0, 0]), // log_generation
        ],
    );
    assert_eq!(file.len(), 84);
    assert_crc(&file);
}

#[test]
fn xfer_vectors() {
    let begin = XferBegin {
        target: target::APP,
        slot: 1,
        total_len: 512 + 1024,
        hdr_len: IMAGE_HDR_LEN,
    };
    let file = golden(
        "frame_xfer_begin.bin",
        &frame(0, 0x03, Cmd::XferBegin, |b| begin.encode(b).unwrap()),
    );
    assert_eq!(XferBegin::parse(decode(&file).payload).unwrap(), begin);
    // target APP, slot B, total_len 1536, hdr_len 512
    expect_at(
        &file,
        &[(5, &[0x10, 8, 0, 0, 1, 0x00, 0x06, 0, 0, 0x00, 0x02])],
    );
    assert_crc(&file);

    let data: Vec<u8> = (0..252u32).map(|i| i as u8).collect();
    let chunk = XferChunk {
        offset: 252,
        data: &data,
    };
    let file = golden(
        "frame_xfer_hdr.bin",
        &frame(0, 0x05, Cmd::XferHdr, |b| chunk.encode(b).unwrap()),
    );
    assert_eq!(XferChunk::parse(decode(&file).payload).unwrap(), chunk);
    assert_eq!(file.len(), 268, "max I2C frame");
    expect_at(&file, &[(5, &[0x15, 0x00, 0x01, 252, 0, 0, 0, 0, 1, 2])]); // len 256, offset 252
}

#[test]
fn relay_vector() {
    let inner = hello_req_frame();
    let relay = Relay {
        link: link::I2C0,
        addr: 0x42,
        timeout_ms: 300,
        inner: &inner,
    };
    let file = golden(
        "frame_relay_hello.bin",
        &frame(0, 0x10, Cmd::Relay, |b| relay.encode(b).unwrap()),
    );
    let r = Relay::parse(decode(&file).payload).unwrap();
    assert_eq!(r, relay);
    assert_eq!(decode(r.inner).cmd, Cmd::Hello as u8);
    // len 4 + 18; link I2C0, addr 0x42, timeout 300 ms, then the inner frame verbatim
    expect_at(
        &file,
        &[(5, &[0x50, 22, 0, 1, 0x42, 0x2C, 0x01]), (12, &inner)],
    );
}

#[test]
fn hub_command_vectors() {
    let show = Show {
        node: 1,
        phase: phase::TRANSFER,
        percent: 42,
        version: Version::new(1, 1, 0),
        text: "seg".as_bytes(),
    };
    let file = golden(
        "frame_show.bin",
        &frame(0, 0x11, Cmd::Show, |b| show.encode(b).unwrap()),
    );
    assert_eq!(Show::parse(decode(&file).payload).unwrap(), show);
    // node, phase, percent, version u32, text
    expect_at(
        &file,
        &[(
            5,
            &[
                0x41, 10, 0, 1, 1, 42, 0x00, 0x00, 0x01, 0x01, b's', b'e', b'g',
            ],
        )],
    );

    let sub = SubCtl {
        nrst: nrst::PULSE,
        boot0: boot0::HIGH,
    };
    let file = golden(
        "frame_subctl.bin",
        &frame(0, 0x12, Cmd::SubCtl, |b| sub.encode(b).unwrap()),
    );
    assert_eq!(SubCtl::parse(decode(&file).payload).unwrap(), sub);
    expect_at(&file, &[(5, &[0x60, 2, 0, 2, 1])]); // nrst pulse, boot0 high

    let raw = I2cRaw {
        read_len: 1,
        data: &[0x00, 0xFF],
    };
    let file = golden(
        "frame_i2c_raw_get.bin",
        &frame(0, 0x13, Cmd::I2cRaw, |b| raw.encode(b).unwrap()),
    );
    assert_eq!(I2cRaw::parse(decode(&file).payload).unwrap(), raw);
    // write_len 2, read_len 1, data
    expect_at(&file, &[(5, &[0x61, 6, 0, 2, 0, 1, 0, 0x00, 0xFF])]);
}

/// ENTER_ROM was added after G1 (owner-approved, additive).
#[test]
fn enter_rom_vector() {
    let req = EnterRom {
        method: rom_method::FORCE_DOWNLOAD_BOOT,
    };
    let file = golden(
        "frame_enter_rom.bin",
        &frame(0, 0x14, Cmd::EnterRom, |b| req.encode(b).unwrap()),
    );
    assert_eq!(EnterRom::parse(decode(&file).payload).unwrap(), req);
    // SOF, ver, flags, seq, cmd, len 1; method FORCE_DOWNLOAD_BOOT; CRC
    expect_at(&file, &[(0, &[0xDA, 0x6F, 1, 0, 0x14, 0x24, 1, 0, 1])]);
    assert_eq!(file.len(), 13);
    assert_crc(&file);
}

fn version_resp() -> VersionResp {
    let sv = |state, version| SlotVersion { state, version };
    VersionResp {
        wire: Some(1),
        mcu_id: Some(2),
        stage0: Some(Version::new(0, 1, 0)),
        boot_core: Some(Version::new(1, 0, 0)),
        core_slots: Some(VersionSlots {
            active: 0,
            slots: [
                sv(SlotState::Confirmed, Version::new(1, 0, 0)),
                SlotVersion::default(),
            ],
        }),
        app_slots: Some(VersionSlots {
            active: 1,
            slots: [
                sv(SlotState::Written, Version::new(1, 0, 0)),
                sv(SlotState::Trying, Version::new(1, 1, 0)),
                SlotVersion::default(),
            ],
        }),
    }
}

/// VERSION is an additive command of wire 1: the files above are
/// unchanged.
#[test]
fn version_vectors() {
    let file = golden(
        "frame_version_req.bin",
        &frame(0, 0x15, Cmd::Version, |_| 0),
    );
    let f = decode(&file);
    assert_eq!((f.cmd, f.payload), (0x03, &[][..]));
    expect_at(&file, &[(0, &[0xDA, 0x6F, 1, 0, 0x15, 0x03, 0, 0])]);
    assert_eq!(file.len(), 12);
    assert_crc(&file);

    let file = golden(
        "frame_version_resp.bin",
        &frame(flags::RESP, 0x15, Cmd::Version, |b| {
            version_resp().encode(b).unwrap()
        }),
    );
    let (st, body) = split_status(decode(&file).payload).unwrap();
    assert_eq!(st, Status::Ok);
    assert_eq!(body.len(), VersionResp::MAX_LEN);
    assert_eq!(VersionResp::parse(body), Ok(version_resp()));
    // count, then TLVs: id, len, value
    expect_at(
        &file,
        &[
            (6, &[52, 0]),                            // len = status + 51
            (9, &[6]),                                // six values
            (10, &[0x0D, 1, 1]),                      // wire 1
            (13, &[0x06, 2, 0x02, 0x00]),             // mcu_id F401
            (17, &[0x0A, 4, 0x00, 0x00, 0x01, 0x00]), // stage0 0.1.0
            (23, &[0x01, 4, 0x00, 0x00, 0x00, 0x01]), // running boot core 1.0.0
            (29, &[0x0B, 11, 0]),                     // boot core slots, A running
            (32, &[4, 0x00, 0x00, 0x00, 0x01]),       // core A confirmed 1.0.0
            (37, &[0; 5]),                            // core B empty
            (42, &[0x0C, 16, 1]),                     // app slots, B running
            (45, &[1, 0x00, 0x00, 0x00, 0x01]),       // A written 1.0.0
            (50, &[3, 0x00, 0x00, 0x01, 0x01]),       // B trying 1.1.0
            (55, &[0; 5]),                            // factory empty
        ],
    );
    assert_eq!(file.len(), 64);
    assert_crc(&file);
}
