//! The wire's numbers and the typed payloads of dagon-proto for the page's
//! in-memory hub, so the page names commands and statuses and holds no
//! number, field or layout of its own: a request is read by dagon-proto's
//! reader before the hub acts on it, and a response is written by its
//! writer.

use dagon_format::Version;
use dagon_proto::payload::{
    encode_bulk_resp, encode_digest, encode_next_offset, parse_confirm, parse_empty, parse_reboot,
    parse_status_req, put_refusal, HelloReq, HelloResp, Relay, SetPending, Show, SlotInfo,
    SlotState, StatusResp, XferBegin, XferChunk, REFUSAL_MAX,
};
use dagon_proto::{flags, max_payload, Cmd, FrameError, Status, CRC_LEN, HEADER_LEN};
use wasm_bindgen::prelude::*;

/// The numbers of the wire the page names: commands, statuses and the
/// frame's sizes, read from dagon-proto.
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct Wire {
    pub cmd_hello: u8,
    pub cmd_status: u8,
    pub cmd_xfer_begin: u8,
    pub cmd_xfer_data: u8,
    pub cmd_xfer_end: u8,
    pub cmd_xfer_hdr: u8,
    pub cmd_set_pending: u8,
    pub cmd_reboot: u8,
    pub cmd_confirm: u8,
    pub cmd_show: u8,
    pub cmd_relay: u8,
    pub status_ok: u8,
    pub status_unsupported: u8,
    pub status_arg: u8,
    pub status_field: u8,
    pub status_state: u8,
    pub status_timeout: u8,
    pub flag_resp: u8,
    pub header_len: u32,
    pub crc_len: u32,
    pub max_payload_usb: u32,
    pub max_payload_uart: u32,
    pub max_payload_i2c: u32,
}

#[wasm_bindgen]
pub fn wire() -> Wire {
    Wire {
        cmd_hello: Cmd::Hello as u8,
        cmd_status: Cmd::Status as u8,
        cmd_xfer_begin: Cmd::XferBegin as u8,
        cmd_xfer_data: Cmd::XferData as u8,
        cmd_xfer_end: Cmd::XferEnd as u8,
        cmd_xfer_hdr: Cmd::XferHdr as u8,
        cmd_set_pending: Cmd::SetPending as u8,
        cmd_reboot: Cmd::Reboot as u8,
        cmd_confirm: Cmd::Confirm as u8,
        cmd_show: Cmd::Show as u8,
        cmd_relay: Cmd::Relay as u8,
        status_ok: Status::Ok as u8,
        status_unsupported: Status::Unsupported as u8,
        status_arg: Status::Arg as u8,
        status_field: Status::Field as u8,
        status_state: Status::State as u8,
        status_timeout: Status::Timeout as u8,
        flag_resp: flags::RESP,
        header_len: HEADER_LEN as u32,
        crc_len: CRC_LEN as u32,
        max_payload_usb: max_payload::USB as u32,
        max_payload_uart: max_payload::UART as u32,
        max_payload_i2c: max_payload::I2C as u32,
    }
}

/// A request as dagon-proto's reader took it; only the members of its
/// command mean something.
#[wasm_bindgen(getter_with_clone)]
#[derive(Default)]
pub struct Request {
    /// The response payload that refuses the request (its status, and the
    /// number of an unknown must field); empty when the request reads.
    pub refusal: Vec<u8>,
    pub target: u8,
    pub slot: u8,
    pub offset: u32,
    pub link: u8,
    pub addr: u8,
    /// A chunk's data, or a RELAY's inner frame.
    pub bulk: Vec<u8>,
}

fn read_request(cmd: u8, p: &[u8]) -> Result<Request, Status> {
    let mut r = Request::default();
    match Cmd::from_u8(cmd).ok_or(Status::Unsupported)? {
        Cmd::Hello => {
            HelloReq::parse(p)?;
        }
        Cmd::Status => r.target = parse_status_req(p)?,
        Cmd::XferBegin => {
            let b = XferBegin::parse(p)?;
            (r.target, r.slot) = (b.target, b.slot);
        }
        Cmd::XferHdr | Cmd::XferData => {
            let c = XferChunk::parse(p)?;
            (r.offset, r.bulk) = (c.offset, c.data.to_vec());
        }
        Cmd::XferEnd => parse_empty(&Cmd::XferEnd.spec().req, p)?,
        Cmd::SetPending => {
            let s = SetPending::parse(p)?;
            (r.target, r.slot) = (s.target, s.slot);
        }
        Cmd::Reboot => {
            parse_reboot(p)?;
        }
        Cmd::Confirm => r.target = parse_confirm(p)?,
        Cmd::Show => {
            Show::parse(p)?;
        }
        Cmd::Relay => {
            let x = Relay::parse(p)?;
            (r.link, r.addr, r.bulk) = (x.link, x.addr, x.inner.to_vec());
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(r)
}

/// Reads the payload of a `cmd` request. A command the hub does not serve
/// is refused as unsupported; a malformed payload and an unknown must
/// field are refused as a node refuses them.
#[wasm_bindgen]
pub fn request_read(cmd: u8, payload: &[u8]) -> Request {
    read_request(cmd, payload).unwrap_or_else(|status| {
        let mut out = [0u8; REFUSAL_MAX];
        let n = put_refusal(&mut out, status, cmd, payload);
        Request {
            refusal: out[..n].to_vec(),
            ..Request::default()
        }
    })
}

/// An OK response payload: the status, then the body `enc` writes.
fn ok(room: usize, enc: impl FnOnce(&mut [u8]) -> Result<usize, FrameError>) -> Vec<u8> {
    let mut out = vec![0u8; 1 + room];
    out[0] = Status::Ok as u8;
    let n = enc(&mut out[1..]).expect("sized for the body");
    out.truncate(1 + n);
    out
}

/// A response payload that is its status alone.
#[wasm_bindgen]
pub fn response_status(status: u8) -> Vec<u8> {
    vec![status]
}

/// The body of an OK HELLO response (after the status byte).
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn hello_response_body(
    proto_ver: u32,
    role: u8,
    port_kind: u8,
    mcu_id: u16,
    mode: u8,
    active_version: u32,
    trial: bool,
    key_id: &[u8],
    max_payload: u16,
) -> Result<Vec<u8>, JsError> {
    let h = HelloResp {
        proto_ver,
        role,
        port_kind,
        mcu_id,
        mode,
        active_version: Version::from_raw(active_version),
        trial,
        key_id: key_id
            .try_into()
            .map_err(|_| JsError::new("a key id is 8 bytes"))?,
        max_payload,
    };
    Ok(ok(HelloResp::MAX_LEN, |b| h.encode(b)).split_off(1))
}

/// An OK STATUS response: per slot (A, B, factory) its state, version and
/// the 16 header digest bytes in `digests`.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn response_status_slots(
    states: &[u8],
    versions: &[u32],
    digests: &[u8],
    active: u8,
    confirmed: u8,
    attempts: u8,
    last_reason: u8,
    log_generation: u32,
) -> Result<Vec<u8>, JsError> {
    let mut st = StatusResp {
        active,
        confirmed,
        attempts,
        last_reason,
        log_generation,
        ..StatusResp::default()
    };
    let digests = digests.chunks_exact(16);
    for (slot, ((state, version), digest)) in st
        .slots
        .iter_mut()
        .zip(states.iter().zip(versions).zip(digests))
    {
        *slot = SlotInfo {
            state: SlotState::from_u8(*state).ok_or_else(|| JsError::new("slot state"))?,
            version: Version::from_raw(*version),
            digest16: digest.try_into().unwrap_or_default(),
        };
    }
    Ok(ok(StatusResp::MAX_LEN, |b| st.encode(b)))
}

/// An OK XFER_HDR / XFER_DATA response: the next offset.
#[wasm_bindgen]
pub fn response_next_offset(next: u32) -> Vec<u8> {
    ok(8, |b| encode_next_offset(next, b))
}

/// An OK XFER_END response: the SHA-256 of the body received.
#[wasm_bindgen]
pub fn response_digest(sha256: &[u8]) -> Result<Vec<u8>, JsError> {
    let d: [u8; 32] = sha256
        .try_into()
        .map_err(|_| JsError::new("a SHA-256 is 32 bytes"))?;
    Ok(ok(40, |b| encode_digest(&d, b)))
}

/// An OK RELAY response carrying the node's response `frame`.
#[wasm_bindgen]
pub fn response_relay(frame: &[u8]) -> Vec<u8> {
    ok(1 + frame.len(), |b| encode_bulk_resp(frame, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::payload::{parse_bulk_resp, parse_digest, parse_next_offset, refused_field};
    use dagon_proto::split_status;

    #[test]
    fn requests_are_read_or_refused_as_a_node_does() {
        let begin = XferBegin {
            target: 0,
            slot: 1,
            total_len: 600,
            hdr_len: 512,
        };
        let mut b = [0u8; 16];
        let n = begin.encode(&mut b).unwrap();
        let r = request_read(Cmd::XferBegin as u8, &b[..n]);
        assert!(r.refusal.is_empty());
        assert_eq!((r.target, r.slot), (0, 1));
        // Cut short: E_ARG. An unknown must field (300): E_FIELD naming it.
        let r = request_read(Cmd::XferBegin as u8, &b[..3]);
        assert_eq!(r.refusal, [Status::Arg as u8]);
        let r = request_read(Cmd::XferEnd as u8, &[0xD9, 0x04, 0]);
        let (status, body) = split_status(&r.refusal).unwrap();
        assert_eq!((status, refused_field(body)), (Status::Field, Some(300)));
        let r = request_read(Cmd::SubCtl as u8, &[]);
        assert_eq!(r.refusal, [Status::Unsupported as u8]);
        assert_eq!(request_read(0xEE, &[]).refusal, [Status::Unsupported as u8]);
    }

    #[test]
    fn responses_are_the_typed_payloads() {
        assert_eq!(parse_next_offset(&response_next_offset(7)[1..]), Ok(7));
        let d = response_digest(&[9; 32]).ok().unwrap();
        assert_eq!(parse_digest(&d[1..]), Ok([9; 32]));
        let r = response_relay(&[1, 2, 3]);
        assert_eq!(parse_bulk_resp(Cmd::Relay, &r[1..]), Ok(&[1, 2, 3][..]));
        let w = wire();
        assert_eq!((w.cmd_relay, w.status_field), (0x50, 0x04));
    }
}
