//! Bytes off a link: the frame decoder at every link's payload limit, and
//! the request decoder of the frame's command. Nothing panics; a decoded
//! frame encodes back to the bytes it came from, and a frame built from the
//! bytes decodes to what it was built from.
#![no_main]

use dagon_proto::ceiling::Ceiling;
use dagon_proto::handler::Handler;
use dagon_proto::payload::Req;
use dagon_proto::{frame_len, max_payload, Cmd, Frame, Status};
use libfuzzer_sys::fuzz_target;

const LIMITS: [usize; 4] = [
    max_payload::SMALLEST,
    max_payload::I2C,
    max_payload::UART,
    max_payload::USB,
];

/// The request `payload` is for `cmd` on a link of `limit`, as a node's
/// table decodes it; a request that decodes names its longest answer.
fn request(cmd: Cmd, payload: &[u8], limit: usize) {
    fn nothing(_: &mut (), _: Req<'_>) -> Result<(), Status> {
        Ok(())
    }
    let row = Handler::<(), ()>::new(cmd, nothing);
    if let Ok(request) = row.request(payload, Ceiling::link(limit)) {
        let _ = request.response_max();
    }
}

fuzz_target!(|data: &[u8]| {
    for limit in LIMITS {
        let Ok((frame, used)) = Frame::decode(data, limit) else {
            continue;
        };
        assert!(frame.payload.len() <= limit);
        assert_eq!(used, frame_len(frame.payload.len()));
        let mut again = vec![0u8; used];
        assert_eq!(frame.encode(&mut again), Ok(used));
        assert_eq!(again, data[..used]);
        if let Some(cmd) = Cmd::from_u8(frame.cmd) {
            request(cmd, frame.payload, limit);
        }
    }
    // The same bytes as a frame's fields (flags, seq, cmd, then the
    // payload): the frame they encode to decodes to them, and its payload
    // goes to the request decoder with no checksum in the way.
    if let [flags, seq, cmd, payload @ ..] = data {
        let frame = Frame {
            flags: *flags,
            seq: *seq,
            cmd: *cmd,
            payload,
        };
        if payload.len() <= max_payload::USB {
            let mut wire = vec![0u8; frame_len(payload.len())];
            let n = frame.encode(&mut wire).unwrap();
            assert_eq!(Frame::decode(&wire, max_payload::USB), Ok((frame, n)));
        }
        if let Some(cmd) = Cmd::from_u8(*cmd) {
            for limit in LIMITS {
                request(cmd, payload, limit);
            }
        }
    }
});
