//! Link frames for the page: encoding (header and CRC) and cutting frames
//! out of the serial byte stream, with dagon-proto's codec, so the page has
//! no frame or CRC code of its own.

use dagon_proto::field::{read, FieldWriter, MsgSpec};
use dagon_proto::{find_sof, frame_len, Cmd, Frame, FrameError};
use wasm_bindgen::prelude::*;

fn js_error(e: impl core::fmt::Debug) -> JsError {
    JsError::new(&format!("{e:?}"))
}

/// The table's row for a request or response of `cmd`.
fn spec(cmd: u8, response: bool) -> Result<&'static MsgSpec, JsError> {
    let row = Cmd::from_u8(cmd).ok_or_else(|| JsError::new("unknown command"))?;
    Ok(if response {
        &row.spec().resp
    } else {
        &row.spec().req
    })
}

/// A payload as dagon-proto's reader took it apart (a response's after
/// its status byte), for the page's in-memory hub: the page has no field
/// code of its own.
#[wasm_bindgen]
pub struct Payload {
    core: Vec<u8>,
    bulk: Vec<u8>,
    fields: Vec<(u16, Vec<u8>)>,
}

#[wasm_bindgen]
impl Payload {
    #[wasm_bindgen(getter)]
    pub fn core(&self) -> Vec<u8> {
        self.core.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn bulk(&self) -> Vec<u8> {
        self.bulk.clone()
    }

    /// The value of field `num`, when it was sent.
    pub fn field(&self, num: u16) -> Option<Vec<u8>> {
        let hit = self.fields.iter().find(|(n, _)| *n == num)?;
        Some(hit.1.clone())
    }
}

/// Reads the payload of a `cmd` request or response.
#[wasm_bindgen]
pub fn payload_read(cmd: u8, response: bool, payload: &[u8]) -> Result<Payload, JsError> {
    let spec = spec(cmd, response)?;
    let p = read(payload, spec).map_err(js_error)?;
    let sent = |f| p.field(f).map(|v| (f.num, v.to_vec()));
    Ok(Payload {
        core: p.core.to_vec(),
        bulk: p.bulk.to_vec(),
        fields: spec.fields.iter().filter_map(sent).collect(),
    })
}

/// Writes the payload of a `cmd` request or response with dagon-proto's
/// writer: the core, then fields in rising number order, then the bulk of
/// a message that carries one.
#[wasm_bindgen]
pub struct PayloadWriter {
    spec: &'static MsgSpec,
    core: Vec<u8>,
    fields: Vec<(u16, Vec<u8>)>,
    bulk: Vec<u8>,
}

#[wasm_bindgen]
impl PayloadWriter {
    #[wasm_bindgen(constructor)]
    pub fn new(cmd: u8, response: bool) -> Result<PayloadWriter, JsError> {
        Ok(PayloadWriter {
            spec: spec(cmd, response)?,
            core: Vec::new(),
            fields: Vec::new(),
            bulk: Vec::new(),
        })
    }

    pub fn core(&mut self, bytes: &[u8]) {
        self.core = bytes.to_vec();
    }

    pub fn field(&mut self, num: u16, value: &[u8]) {
        self.fields.push((num, value.to_vec()));
    }

    pub fn bulk(&mut self, bytes: &[u8]) {
        self.bulk = bytes.to_vec();
    }

    /// The payload's bytes; an error for a core of another length, a
    /// field the table does not list or lists otherwise, or a wrong order.
    pub fn finish(&self) -> Result<Vec<u8>, JsError> {
        if self.core.len() != usize::from(self.spec.core) {
            return Err(JsError::new("core length"));
        }
        let room = self.fields.iter().map(|f| 8 + f.1.len()).sum::<usize>();
        let mut out = vec![0u8; self.core.len() + room + 1 + self.bulk.len()];
        let mut w = FieldWriter::new(&mut out);
        w.put(&self.core).map_err(js_error)?;
        for (num, value) in &self.fields {
            let known = self.spec.fields.iter().find(|f| f.num == *num);
            let f = known.ok_or_else(|| JsError::new("unknown field"))?;
            w.field(f, value).map_err(js_error)?;
        }
        if self.spec.bulk {
            w.bulk(&self.bulk).map_err(js_error)?;
        }
        let n = w.len();
        out.truncate(n);
        Ok(out)
    }
}

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
    .map_err(js_error)?;
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
