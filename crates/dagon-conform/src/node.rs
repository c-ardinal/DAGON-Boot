//! Node side: the CONF_RUN handler of a conformance build.
//!
//! Serve it in front of the agent: pass every request frame to
//! [`handle_frame`] first and hand it to `dagon_agent::Agent::handle` when it
//! returns `None`.
//!
//! | test_id | Test | Passes when |
//! |---|---|---|
//! | 1 | CT-01 flash geometry | `geometry()` equals the declared layout; regions are sane and do not overlap |
//! | 2 | CT-02 erase | after a write and an erase of the scratch erase unit, every byte reads the erased value |
//! | 3 | CT-03 aligned write | aligned writes into the erased scratch unit read back unchanged |
//! | 7 | CT-07 ed25519 | RFC 8032 TEST 1 and a signed image header verify; 1-bit flips of the header and the signature are all rejected |
//! | 8 | CT-08 sha-256 | FIPS 180-2 vectors (one-shot and incremental) match |
//!
//! `param` is unused (send 0). Other test ids answer E_UNSUPPORTED.

use crate::{detail, Detail};
use dagon_format::header::{self, HEADER_SIZE};
use dagon_format::{sha256, verify_signed, Sha256};
use dagon_port::{Flash, FlashRegion};
use dagon_proto::payload::{verdict, ConfRunReq, ConfRunResp, CONF_DETAIL_MAX};
use dagon_proto::{flags, frame_len, Cmd, Frame, Status, HEADER_LEN};

/// Response frame size `handle_frame` may need.
pub const RESPONSE_MAX: usize = frame_len(1 + 5 + CONF_DETAIL_MAX);

const CHUNK: usize = 256;

/// What the board tells the node-side tests.
#[derive(Clone, Copy, Debug)]
pub struct NodeSetup<'a> {
    /// The flash layout the port declares (its documented memory map).
    pub declared: &'a [FlashRegion],
    /// Start of one erase unit the tests may destroy: the first erase unit of
    /// the slot that is not running.
    pub scratch: u32,
}

/// Result of one node-side test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub pass: bool,
    pub measured: u32,
    pub detail: Detail,
}

type TestResult = Result<u32, Detail>;

impl From<TestResult> for Outcome {
    fn from(r: TestResult) -> Self {
        match r {
            Ok(measured) => Outcome {
                pass: true,
                measured,
                detail: Detail::new(),
            },
            Err(detail) => Outcome {
                pass: false,
                measured: 0,
                detail,
            },
        }
    }
}

/// Runs one node-side test. `None` for a test id that does not run in the
/// node.
pub fn run<F: Flash>(flash: &mut F, setup: &NodeSetup<'_>, req: ConfRunReq) -> Option<Outcome> {
    let r = match req.test_id {
        1 => ct01_geometry(flash, setup),
        2 => ct02_erase(flash, setup),
        3 => ct03_write(flash, setup),
        7 => ct07_ed25519(),
        8 => ct08_sha256(),
        _ => return None,
    };
    Some(r.into())
}

/// Answers a CONF_RUN request frame; writes the response frame to `out`
/// (at least [`RESPONSE_MAX`] bytes) and returns its length. `None` when the
/// frame is not a CONF_RUN request: pass it on to the agent.
pub fn handle_frame<F: Flash>(
    request: &[u8],
    max_payload: usize,
    flash: &mut F,
    setup: &NodeSetup<'_>,
    out: &mut [u8],
) -> Option<usize> {
    let (frame, _) = Frame::decode(request, max_payload).ok()?;
    if frame.cmd != Cmd::ConfRun as u8 || frame.flags & (flags::RESP | flags::EVENT) != 0 {
        return None;
    }
    let body = &mut out[HEADER_LEN + 1..];
    let (status, len) = match ConfRunReq::parse(frame.payload) {
        Err(s) => (s, 0),
        Ok(req) => match run(flash, setup, req) {
            None => (Status::Unsupported, 0),
            Some(o) => {
                let resp = ConfRunResp {
                    verdict: if o.pass { verdict::PASS } else { verdict::FAIL },
                    measured: o.measured,
                    detail: o.detail.as_bytes(),
                };
                (Status::Ok, resp.encode(body).ok()?)
            }
        },
    };
    out[HEADER_LEN] = status as u8;
    dagon_proto::finish(out, flags::RESP, frame.seq, frame.cmd, 1 + len).ok()
}

fn ct01_geometry<F: Flash>(flash: &mut F, setup: &NodeSetup<'_>) -> TestResult {
    let geo = flash.geometry();
    if geo != setup.declared {
        return Err(detail!(
            "geometry differs from declared ({} vs {} regions)",
            geo.len(),
            setup.declared.len()
        ));
    }
    if geo.is_empty() {
        return Err(detail!("no regions"));
    }
    for (i, r) in geo.iter().enumerate() {
        let sane = r.len > 0
            && r.erase_size > 0
            && r.write_size > 0
            && r.erase_size % r.write_size == 0
            && r.start % r.erase_size == 0
            && r.len % r.erase_size == 0
            && end(r) <= 1 << 32;
        if !sane {
            return Err(detail!("region {} sizes not aligned", i));
        }
        if let Some(j) = geo[i + 1..].iter().position(|o| overlaps(r, o)) {
            return Err(detail!("regions {} and {} overlap", i, i + 1 + j));
        }
    }
    Ok(geo.len() as u32)
}

fn end(r: &FlashRegion) -> u64 {
    r.start as u64 + r.len as u64
}

fn overlaps(a: &FlashRegion, b: &FlashRegion) -> bool {
    (a.start as u64) < end(b) && (b.start as u64) < end(a)
}

/// Region of the scratch unit, checked to be erase-aligned.
fn scratch_region<F: Flash>(flash: &F, setup: &NodeSetup<'_>) -> Result<FlashRegion, Detail> {
    let at = setup.scratch as u64;
    let r = *flash
        .geometry()
        .iter()
        .find(|r| (r.start as u64) <= at && at < end(r))
        .ok_or_else(|| detail!("scratch {:#x} outside flash", setup.scratch))?;
    if r.erase_size == 0 || r.write_size == 0 {
        return Err(detail!("zero erase or write unit"));
    }
    if !(setup.scratch - r.start).is_multiple_of(r.erase_size) {
        return Err(detail!("scratch {:#x} not erase aligned", setup.scratch));
    }
    if r.write_size as usize > CHUNK {
        return Err(detail!("write unit {} above {}", r.write_size, CHUNK));
    }
    Ok(r)
}

fn pattern(i: u32) -> u8 {
    (i as u8).wrapping_mul(37) ^ 0x5A
}

fn erase<F: Flash>(flash: &mut F, addr: u32, len: u32) -> Result<(), Detail> {
    flash
        .erase(addr, len)
        .map_err(|e| detail!("erase {:#x}: {:?}", addr, e))
}

fn write_pattern<F: Flash>(flash: &mut F, addr: u32, len: u32, from: u32) -> Result<(), Detail> {
    let mut buf = [0u8; CHUNK];
    let b = &mut buf[..len as usize];
    for (i, x) in b.iter_mut().enumerate() {
        *x = pattern(from + i as u32);
    }
    flash
        .write(addr, b)
        .map_err(|e| detail!("write {:#x}: {:?}", addr, e))
}

/// Reads `len` bytes at `addr`; `expect(i)` is the byte wanted at offset `i`.
fn check<F: Flash>(
    flash: &mut F,
    addr: u32,
    len: u32,
    expect: impl Fn(u32) -> u8,
) -> Result<(), Detail> {
    let mut buf = [0u8; CHUNK];
    let mut off = 0u32;
    while off < len {
        let n = (len - off).min(CHUNK as u32);
        let b = &mut buf[..n as usize];
        flash
            .read(addr + off, b)
            .map_err(|e| detail!("read {:#x}: {:?}", addr + off, e))?;
        if let Some(i) = (0..n).find(|&i| b[i as usize] != expect(off + i)) {
            let at = addr + off + i;
            return Err(detail!(
                "{:#x} reads {:#04x}, want {:#04x}",
                at,
                b[i as usize],
                expect(off + i)
            ));
        }
        off += n;
    }
    Ok(())
}

fn ct02_erase<F: Flash>(flash: &mut F, setup: &NodeSetup<'_>) -> TestResult {
    let r = scratch_region(flash, setup)?;
    let at = setup.scratch;
    erase(flash, at, r.erase_size)?;
    write_pattern(flash, at, r.write_size, 0)?;
    erase(flash, at, r.erase_size)?;
    check(flash, at, r.erase_size, |_| r.erased_value)?;
    Ok(r.erase_size)
}

fn ct03_write<F: Flash>(flash: &mut F, setup: &NodeSetup<'_>) -> TestResult {
    let r = scratch_region(flash, setup)?;
    let at = setup.scratch;
    // One long aligned write, then one write unit right after it.
    let first = (CHUNK as u32 / r.write_size * r.write_size).min(r.erase_size);
    let second = r.write_size.min(r.erase_size - first);
    erase(flash, at, r.erase_size)?;
    write_pattern(flash, at, first, 0)?;
    if second > 0 {
        write_pattern(flash, at + first, second, first)?;
    }
    check(flash, at, first + second, pattern)?;
    erase(flash, at, r.erase_size)?;
    Ok(first + second)
}

/// RFC 8032 section 7.1 TEST 1 (empty message).
const RFC8032_PK: [u8; 32] =
    hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
const RFC8032_SIG: [u8; 64] = hex(concat!(
    "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555",
    "fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
));
/// Image header signed with the TEST 1 key.
const SIGNED_HEADER: &[u8; HEADER_SIZE] = include_bytes!("../vectors/ct07_header.bin");
/// Bits flipped one at a time: fields across the signed part, then the
/// signature (R and S).
const FLIP_BITS: [(usize, u8); 10] = [
    (0x008, 0),
    (0x00A, 1),
    (0x014, 2),
    (0x018, 3),
    (0x028, 4),
    (0x050, 5),
    (0x1BF, 7),
    (0x1C0, 0),
    (0x1E0, 6),
    (0x1FF, 1),
];

fn ct07_ed25519() -> TestResult {
    verify_signed(&RFC8032_PK, b"", b"", &RFC8032_SIG)
        .map_err(|e| detail!("RFC 8032 TEST 1: {:?}", e))?;
    header::verify_header(SIGNED_HEADER, &RFC8032_PK)
        .map_err(|e| detail!("signed header: {:?}", e))?;
    let mut rejected = 0;
    for (byte, bit) in FLIP_BITS {
        let mut h = *SIGNED_HEADER;
        h[byte] ^= 1 << bit;
        if header::verify_header(&h, &RFC8032_PK).is_ok() {
            return Err(detail!("flip {:#x}.{} accepted", byte, bit));
        }
        rejected += 1;
    }
    Ok(rejected)
}

const SHA_VECTORS: [(&[u8], [u8; 32]); 3] = [
    (
        b"",
        hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    ),
    (
        b"abc",
        hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
    ),
    (
        b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        hex("248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"),
    ),
];
const MILLION_A: [u8; 32] = hex("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");

fn ct08_sha256() -> TestResult {
    for (i, (msg, want)) in SHA_VECTORS.iter().enumerate() {
        if sha256(msg) != *want {
            return Err(detail!("vector {} one-shot mismatch", i));
        }
        let mut h = Sha256::new();
        msg.chunks(3).for_each(|c| h.update(c));
        if h.finalize() != *want {
            return Err(detail!("vector {} incremental mismatch", i));
        }
    }
    let mut h = Sha256::new();
    (0..1_000_000 / 64).for_each(|_| h.update(&[b'a'; 64]));
    if h.finalize() != MILLION_A {
        return Err(detail!("million 'a' mismatch"));
    }
    Ok(SHA_VECTORS.len() as u32 * 2 + 1)
}

const fn hex<const N: usize>(s: &str) -> [u8; N] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("bad hex"),
        }
    }
    let b = s.as_bytes();
    assert!(b.len() == N * 2);
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = nib(b[2 * i]) << 4 | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_tests_pass_on_host() {
        assert_eq!(ct07_ed25519(), Ok(FLIP_BITS.len() as u32));
        assert_eq!(ct08_sha256(), Ok(7));
    }
}
