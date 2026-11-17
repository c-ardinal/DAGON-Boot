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
        proto_ver: 2,
        role: role::NODE,
        port_kind: 1,
        mcu_id: 2,
        mode: mode::APP,
        active_version: Version::new(1, 1, 0),
        trial: true,
        key_id: KEY_ID,
        max_payload: max_payload::I2C as u16,
        cmds: CmdSet::EMPTY,
    }
}

fn hello_req_frame() -> Vec<u8> {
    let req = HelloReq {
        proto_ver: 2,
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
            &[0xDA, 0x6F, 2, 0, 1, 0x01, 6, 0, 2, 0, 0, 0, 0x40, 0x04],
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
            (0, &[0xDA, 0x6F, 2, flags::RESP, 1, 0x01, 39, 0]),
            (8, &[0x00]),                             // status OK
            (9, &[2, 0, 0, 0, 0x00, 0x01]),           // core: proto_ver, max_payload 256
            (15, &[0x02, 1, 1]),                      // field 1: role node
            (18, &[0x04, 1, 1]),                      // field 2: port_kind S
            (21, &[0x06, 2, 0x02, 0x00]),             // field 3: mcu_id F401
            (25, &[0x08, 1, 0]),                      // field 4: mode app
            (28, &[0x0A, 4, 0x00, 0x00, 0x01, 0x01]), // field 5: active_version 1.1.0
            (34, &[0x0C, 1, 1]),                      // field 6: trial
            (37, &[0x0E, 8]),                         // field 7: key id
            (39, &KEY_ID),
        ],
    );
    assert_crc(&file);
}

/// HELLO from a node that says what it serves, and CAPS about one of those
/// commands. `frame_hello_resp.bin` is the answer of a node that does not
/// say, and stays byte for byte what it was.
#[test]
fn capability_vectors() {
    let served = [Cmd::Hello, Cmd::Status, Cmd::Version, Cmd::Caps, Cmd::Relay];
    let resp = HelloResp {
        cmds: served.iter().fold(CmdSet::EMPTY, |s, c| s.with(*c as u8)),
        ..hello_resp()
    };
    let file = golden(
        "frame_hello_resp_cmds.bin",
        &frame(flags::RESP, 0x01, Cmd::Hello, |b| resp.encode(b).unwrap()),
    );
    let f = decode(&file);
    let (st, body) = split_status(f.payload).unwrap();
    assert_eq!(st, Status::Ok);
    assert_eq!(HelloResp::parse(body).unwrap(), resp);
    expect_at(
        &file,
        &[
            (0, &[0xDA, 0x6F, 2, flags::RESP, 1, 0x01, 52, 0]),
            (37, &[0x0E, 8]), // field 7: key id, as without `cmds`
            (39, &KEY_ID),
            // field 8: cmds 0x01 to 0x04 in byte 0, 0x50 in byte 10
            (47, &[0x10, 11, 0x1E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]),
        ],
    );
    assert_crc(&file);

    let file = golden(
        "frame_caps_req.bin",
        &frame(0, 0x16, Cmd::Caps, |b| {
            encode_caps_req(Cmd::Relay as u8, b).unwrap()
        }),
    );
    let f = decode(&file);
    assert_eq!(parse_caps_req(f.payload), Ok(Cmd::Relay as u8));
    expect_at(&file, &[(0, &[0xDA, 0x6F, 2, 0, 0x16, 0x04, 1, 0, 0x50])]);
    assert_crc(&file);

    let links = [(1u8, 268u16), (2, 1031)];
    let mut packed = [0u8; 6];
    let caps = CapsResp {
        tunnel_mtu: TunnelMtus::pack(
            links.map(|(link, mtu)| TunnelMtu { link, mtu }),
            &mut packed,
        )
        .unwrap(),
        ..CapsResp::of(Cmd::Relay)
    };
    let file = golden(
        "frame_caps_resp.bin",
        &frame(flags::RESP, 0x16, Cmd::Caps, |b| caps.encode(b).unwrap()),
    );
    let f = decode(&file);
    let (st, body) = split_status(f.payload).unwrap();
    assert_eq!(st, Status::Ok);
    assert_eq!(CapsResp::parse(body).unwrap(), caps);
    expect_at(
        &file,
        &[
            (0, &[0xDA, 0x6F, 2, flags::RESP, 0x16, 0x04, 12, 0]),
            (8, &[0x00]),           // status OK
            (9, &[0x02, 1, 0x06]),  // field 1: request fields 1 and 2
            (12, &[0x06, 6]),       // field 3: tunnel entries
            (14, &[1, 0x0C, 0x01]), // link 1: 268
            (17, &[2, 0x07, 0x04]), // link 2: 1031
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
    assert_eq!(body.len(), StatusResp::MAX_LEN);
    let mut a = vec![1, 0x00, 0x00, 0x00, 0x01]; // A: written, 1.0.0
    a.extend([0xA0; 16]);
    let mut b = vec![3, 0x00, 0x00, 0x01, 0x01]; // B: trying, 1.1.0
    b.extend([0xB1; 16]);
    expect_at(
        &file,
        &[
            (6, &[84, 0]), // len = status + 83
            (8, &[0x00]),
            (9, &[0x02, 63]), // field 1: three packed slots
            (11, &a),         // a slot is 21 bytes: state, version, digest16
            (32, &b),
            (53, &[0; 21]),               // factory empty
            (74, &[0x04, 1, 1]),          // field 2: active
            (77, &[0x06, 1, 0]),          // field 3: confirmed
            (80, &[0x08, 1, 1]),          // field 4: attempts
            (83, &[0x0A, 1, 0]),          // field 5: last_reason
            (86, &[0x0C, 4, 1, 0, 0, 0]), // field 6: log_generation
        ],
    );
    assert_eq!(file.len(), 96);
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
    // core: target APP, total_len 1536; must fields: 1 slot B, 2 hdr_len 512
    expect_at(
        &file,
        &[(
            5,
            &[
                0x10, 12, 0, 0, 0x00, 0x06, 0, 0, 0x03, 1, 1, 0x05, 2, 0x00, 0x02,
            ],
        )],
    );
    assert_crc(&file);

    let data: Vec<u8> = (0..251u32).map(|i| i as u8).collect();
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
    // len 256: offset 252, the delimiter, then the data
    expect_at(&file, &[(5, &[0x15, 0x00, 0x01, 252, 0, 0, 0, 0, 0, 1, 2])]);
}

/// The relay link id the built-in RP2040 definition gives its port `port`
/// (`[[relay_link]]` in `targets/mcu/rp2040/dagon-mcu.toml`).
fn rp2040_relay_link(port: &str) -> u8 {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "targets",
        "mcu",
        "rp2040",
        "dagon-mcu.toml",
    ]
    .iter()
    .collect();
    let text = std::fs::read_to_string(&path).unwrap();
    let field = |entry: &str, key: &str| {
        entry.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
    };
    text.split("[[relay_link]]")
        .skip(1)
        .find(|e| field(e, "port").as_deref() == Some(port))
        .and_then(|e| field(e, "id"))
        .expect("the RP2040 definition relays over the port")
        .parse()
        .unwrap()
}

#[test]
fn relay_vector() {
    let inner = hello_req_frame();
    let relay = Relay {
        link: rp2040_relay_link("i2c0"),
        addr: 0x42,
        timeout_ms: Some(300),
        inner: &inner,
    };
    let file = golden(
        "frame_relay_hello.bin",
        &frame(0, 0x10, Cmd::Relay, |b| relay.encode(b).unwrap()),
    );
    let r = Relay::parse(decode(&file).payload).unwrap();
    assert_eq!(r, relay);
    assert_eq!(decode(r.inner).cmd, Cmd::Hello as u8);
    // len 9 + 18; core: the link id of the RP2040's i2c0 entry; must field 1
    // addr 0x42, field 2 timeout 300 ms, the delimiter, then the inner frame verbatim
    expect_at(
        &file,
        &[
            (5, &[0x50, 27, 0, 1, 0x03, 1, 0x42, 0x04, 2, 0x2C, 0x01, 0]),
            (17, &inner),
        ],
    );
    assert_eq!(
        decode(&file).payload.len(),
        Relay::OVERHEAD_MAX + inner.len()
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
    // core: phase; fields: 1 node, 2 percent, 3 version u32, 4 text
    expect_at(
        &file,
        &[(
            5,
            &[
                0x41, 18, 0, 1, 0x02, 1, 1, 0x04, 1, 42, 0x06, 4, 0x00, 0x00, 0x01, 0x01, 0x08, 3,
                b's', b'e', b'g',
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
    // must fields: 1 nrst pulse, 2 boot0 high
    expect_at(&file, &[(5, &[0x60, 6, 0, 0x03, 1, 2, 0x05, 1, 1])]);

    let raw = I2cRaw {
        read_len: 1,
        data: &[0x00, 0xFF],
    };
    let file = golden(
        "frame_i2c_raw_get.bin",
        &frame(0, 0x13, Cmd::I2cRaw, |b| raw.encode(b).unwrap()),
    );
    assert_eq!(I2cRaw::parse(decode(&file).payload).unwrap(), raw);
    // must field 1 read_len 1, the delimiter, then the bytes to write
    expect_at(&file, &[(5, &[0x61, 7, 0, 0x03, 2, 1, 0, 0, 0x00, 0xFF])]);
}

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
    expect_at(&file, &[(0, &[0xDA, 0x6F, 2, 0, 0x14, 0x24, 1, 0, 1])]);
    assert_eq!(file.len(), 13);
    assert_crc(&file);
}

fn version_resp() -> VersionResp {
    let sv = |state, version| SlotVersion { state, version };
    VersionResp {
        wire: Some(2),
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

#[test]
fn version_vectors() {
    let file = golden(
        "frame_version_req.bin",
        &frame(0, 0x15, Cmd::Version, |_| 0),
    );
    let f = decode(&file);
    assert_eq!((f.cmd, f.payload), (0x03, &[][..]));
    expect_at(&file, &[(0, &[0xDA, 0x6F, 2, 0, 0x15, 0x03, 0, 0])]);
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
    // one field per value, rising by number
    expect_at(
        &file,
        &[
            (6, &[51, 0]),                            // len = status + 50
            (9, &[0x02, 4, 0x00, 0x00, 0x00, 0x01]),  // 1: running boot core 1.0.0
            (15, &[0x0C, 2, 0x02, 0x00]),             // 6: mcu_id F401
            (19, &[0x14, 4, 0x00, 0x00, 0x01, 0x00]), // 10: stage0 0.1.0
            (25, &[0x16, 11, 0]),                     // 11: boot core slots, A running
            (28, &[4, 0x00, 0x00, 0x00, 0x01]),       // core A confirmed 1.0.0
            (33, &[0; 5]),                            // core B empty
            (38, &[0x18, 16, 1]),                     // 12: app slots, B running
            (41, &[1, 0x00, 0x00, 0x00, 0x01]),       // A written 1.0.0
            (46, &[3, 0x00, 0x00, 0x01, 0x01]),       // B trying 1.1.0
            (51, &[0; 5]),                            // factory empty
            (56, &[0x1A, 1, 2]),                      // 13: wire 2
        ],
    );
    assert_eq!(file.len(), 63);
    assert_crc(&file);
}
