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

    let sub = SubCtl {
        nrst: nrst::PULSE,
        boot0: boot0::HIGH,
    };
    let file = golden(
        "frame_subctl.bin",
        &frame(0, 0x12, Cmd::SubCtl, |b| sub.encode(b).unwrap()),
    );
    assert_eq!(SubCtl::parse(decode(&file).payload).unwrap(), sub);

    let raw = I2cRaw {
        read_len: 1,
        data: &[0x00, 0xFF],
    };
    let file = golden(
        "frame_i2c_raw_get.bin",
        &frame(0, 0x13, Cmd::I2cRaw, |b| raw.encode(b).unwrap()),
    );
    assert_eq!(I2cRaw::parse(decode(&file).payload).unwrap(), raw);
}
