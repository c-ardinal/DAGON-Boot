//! Link frames for the page: encoding (header and CRC) and cutting frames
//! out of the serial byte stream, with dagon-proto's codec, so the page has
//! no frame or CRC code of its own.

use dagon_proto::{find_sof, frame_len, is_refused_start, Frame, FrameError, REFUSED_LEN};
use wasm_bindgen::prelude::*;

/// A complete frame (header, payload, CRC) for `payload`.
#[wasm_bindgen]
pub fn frame_encode(flags: u8, seq: u8, cmd: u8, payload: &[u8]) -> Result<Vec<u8>, JsError> {
    let mut out = vec![0u8; frame_len(payload.len())];
    Frame {
        flags,
        seq,
        cmd,
        payload,
    }
    .encode(&mut out)
    .map_err(|e| JsError::new(&format!("{e:?}")))?;
    Ok(out)
}

/// The wire version of the peer when `frame` is its refusal, in another
/// wire version, of the request (`seq`, `cmd`)
/// (`dagon_proto::refusing_version`).
#[wasm_bindgen]
pub fn refusing_version(frame: &[u8], seq: u8, cmd: u8) -> Option<u8> {
    dagon_proto::refusing_version(frame, seq, cmd)
}

/// Cuts complete, CRC-checked frames out of a byte stream. Bytes before a
/// SOF and frames with a bad length or CRC are skipped. A frame of another
/// wire version is handed over as its first `REFUSED_LEN` bytes, as the
/// firmware's cutters do, so that its refusal of a request is read
/// ([`refusing_version`]) instead of waited out.
#[wasm_bindgen]
pub struct FrameCutter {
    buf: Vec<u8>,
    max_payload: usize,
}

#[wasm_bindgen]
impl FrameCutter {
    /// `max_payload`: the link's payload limit (USB 1088).
    #[wasm_bindgen(constructor)]
    pub fn new(max_payload: usize) -> FrameCutter {
        FrameCutter {
            buf: Vec::new(),
            max_payload,
        }
    }

    /// Adds received bytes; returns each complete frame as raw bytes.
    pub fn push(&mut self, chunk: &[u8]) -> js_sys::Array {
        self.cut(chunk)
            .iter()
            .map(|f| js_sys::Uint8Array::from(f.as_slice()))
            .collect()
    }
}

impl FrameCutter {
    fn cut(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(chunk);
        let mut frames = Vec::new();
        loop {
            let Some(start) = find_sof(&self.buf, 0) else {
                self.buf.clear();
                break;
            };
            self.buf.drain(..start);
            match Frame::decode(&self.buf, self.max_payload) {
                Ok((_, n)) => frames.push(self.buf.drain(..n).collect()),
                Err(FrameError::Incomplete) => break,
                Err(FrameError::BadVersion) if is_refused_start(&self.buf) => {
                    frames.push(self.buf.drain(..REFUSED_LEN).collect());
                }
                Err(_) => {
                    self.buf.drain(..1);
                }
            }
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_split_noisy_and_corrupt_input() {
        let a = frame_encode(0, 7, 1, &[1, 2, 3]).unwrap();
        let mut bad = a.clone();
        bad[9] ^= 1;
        let mut c = FrameCutter::new(1088);
        assert!(c.cut(b"boot log\xDA").is_empty());
        assert!(c.cut(&bad).is_empty());
        assert!(c.cut(&a[..5]).is_empty());
        assert_eq!(c.cut(&a[5..]), std::slice::from_ref(&a));
        assert_eq!(c.cut(&[a.as_slice(), &a].concat()), [a.clone(), a]);
    }

    #[test]
    fn a_frame_of_another_wire_version_is_handed_over_as_its_start() {
        let req = frame_encode(0, 9, 1, &[]).unwrap();
        let mut theirs = req.clone();
        theirs[2] = dagon_proto::PROTO_VERSION + 1;
        let mut refusal = [0u8; 32];
        let n = dagon_proto::version_refusal(&theirs, &mut refusal).unwrap();
        refusal[2] = dagon_proto::PROTO_VERSION + 1;
        let mut c = FrameCutter::new(1088);
        let got = c.cut(&refusal[..n]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0], refusal[..REFUSED_LEN]);
        assert_eq!(
            refusing_version(&got[0], 9, 1),
            Some(dagon_proto::PROTO_VERSION + 1)
        );
        // What follows is still cut.
        assert_eq!(c.cut(&req), [req]);
    }
}
