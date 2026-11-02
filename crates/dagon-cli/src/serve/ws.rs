//! WebSocket (RFC 6455) on a served connection: the handshake and text
//! frames. Server frames are unmasked; client frames must be masked.

use std::io::{Read, Write};

use base64::Engine;
use sha1::{Digest, Sha1};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Largest client frame accepted (the client only sends the passphrase and
/// control frames).
pub const MAX_FRAME: u64 = 64 * 1024;

pub mod op {
    pub const TEXT: u8 = 0x1;
    pub const CLOSE: u8 = 0x8;
    pub const PING: u8 = 0x9;
    pub const PONG: u8 = 0xA;
}

/// `Sec-WebSocket-Accept` for a client key.
pub fn accept_key(key: &str) -> String {
    let digest = Sha1::digest(format!("{}{GUID}", key.trim()).as_bytes());
    base64::engine::general_purpose::STANDARD.encode(digest)
}

pub fn handshake_response(key: &str) -> String {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key)
    )
}

pub fn write_frame<W: Write>(w: &mut W, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut head = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => head.push(n as u8),
        n if n <= u16::MAX as usize => {
            head.push(126);
            head.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            head.push(127);
            head.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    w.write_all(&head)?;
    w.write_all(payload)?;
    w.flush()
}

/// Reads one complete client frame: (opcode, unmasked payload).
/// Fragmented messages are not used by the page and are refused.
pub fn read_frame<R: Read>(r: &mut R) -> std::io::Result<(u8, Vec<u8>)> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    if h[0] & 0x80 == 0 {
        return Err(bad("fragmented frame"));
    }
    if h[1] & 0x80 == 0 {
        return Err(bad("unmasked client frame"));
    }
    let len = match h[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            u16::from_be_bytes(b) as u64
        }
        127 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => n as u64,
    };
    if len > MAX_FRAME {
        return Err(bad("frame too large"));
    }
    let mut mask = [0u8; 4];
    r.read_exact(&mut mask)?;
    let mut data = vec![0u8; len as usize];
    r.read_exact(&mut data)?;
    data.iter_mut()
        .enumerate()
        .for_each(|(i, b)| *b ^= mask[i % 4]);
    Ok((h[0] & 0x0F, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6455_sample_accept_key() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frames_round_trip() {
        let mut out = Vec::new();
        write_frame(&mut out, op::TEXT, &[b'x'; 300]).unwrap();
        assert_eq!(&out[..4], &[0x81, 126, 0x01, 0x2C]);
        // A masked client frame "Hello" (RFC 6455 section 5.7).
        let client = [
            0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
        ];
        let (o, p) = read_frame(&mut &client[..]).unwrap();
        assert_eq!((o, p.as_slice()), (op::TEXT, &b"Hello"[..]));
        let unmasked = [0x81, 0x05, b'H', b'e', b'l', b'l', b'o'];
        assert!(read_frame(&mut &unmasked[..]).is_err());
    }
}
