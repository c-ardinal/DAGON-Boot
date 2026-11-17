//! A request frame of a wire version this crate does not read is answered
//! with `Status::Frame` and the version it does read; a response or event
//! of such a version is never answered.

use dagon_proto::field::read;
use dagon_proto::{
    finish, flags, frame_len, table, version_refusal, Cmd, Frame, FrameError, Status, HEADER_LEN,
    PROTO_VERSION,
};

fn other_version(fl: u8) -> Vec<u8> {
    let mut b = vec![0u8; frame_len(2)];
    b[HEADER_LEN..HEADER_LEN + 2].copy_from_slice(&[1, 2]);
    let n = finish(&mut b, fl, 0x33, Cmd::Hello as u8, 2).unwrap();
    assert_eq!(n, b.len());
    b[2] = PROTO_VERSION + 1;
    b
}

#[test]
fn unknown_version_is_refused_explicitly() {
    let req = other_version(0);
    assert_eq!(Frame::decode(&req, 256), Err(FrameError::BadVersion));
    let mut out = [0u8; 32];
    let n = version_refusal(&req, &mut out).expect("an answer");
    let (f, used) = Frame::decode(&out[..n], 256).unwrap();
    assert_eq!(used, n);
    assert_eq!(
        (f.flags, f.seq, f.cmd),
        (flags::RESP, 0x33, Cmd::Hello as u8)
    );
    assert_eq!(f.payload[0], Status::Frame as u8);
    let body = read(&f.payload[1..], &table::ERROR).unwrap();
    assert_eq!(body.field(&table::error::WIRE), Some(&[PROTO_VERSION][..]));
    // The stream cutters hand over the first HEADER_LEN bytes: enough.
    assert_eq!(version_refusal(&req[..HEADER_LEN], &mut out), Some(n));
    assert!(dagon_proto::is_refused_start(&req));
    // Noise that looks like a start right before a frame is not handed over.
    assert!(!dagon_proto::is_refused_start(b"\xDA\x6F junk\xDA\x6F"));
    assert!(!dagon_proto::is_refused_start(b"\xDA\x6F j"));
}

#[test]
fn only_a_request_of_another_version_is_answered() {
    let mut out = [0u8; 32];
    assert_eq!(version_refusal(&other_version(flags::RESP), &mut out), None);
    assert_eq!(
        version_refusal(&other_version(flags::EVENT), &mut out),
        None
    );
    let mut ours = other_version(0);
    ours[2] = PROTO_VERSION;
    assert_eq!(version_refusal(&ours, &mut out), None);
    assert_eq!(
        version_refusal(b"\xDA\x6F\x02", &mut out),
        None,
        "too short"
    );
    assert_eq!(
        version_refusal(b"xx\x02\x00\x00\x00\x00\x00", &mut out),
        None
    );
    assert_eq!(version_refusal(&other_version(0), &mut out[..4]), None);
}
