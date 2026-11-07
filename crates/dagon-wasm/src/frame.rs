//! Link frames for the page: encoding (header and CRC) and cutting frames
//! out of the serial byte stream, with dagon-proto's codec, so the page has
//! no frame or CRC code of its own.

use dagon_proto::{find_sof, frame_len, Frame, FrameError};
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

/// Cuts complete, CRC-checked frames out of a byte stream. Bytes before a
/// SOF and frames with a bad version, length or CRC are skipped.
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
}
