//! ESP32-S3 ROM serial loader (Espressif's serial protocol, ROM side, no
//! stub), driven over the byte stream of the node's UART link once the node
//! has switched itself to the loader (ENTER_ROM).
//!
//! [`flash`] synchronises with the loader, attaches the SPI flash and tells
//! it the flash size, then for each region in order: FLASH_BEGIN (the
//! loader erases the region), FLASH_DATA in blocks of [`BLOCK`] bytes (the
//! last padded with 0xFF), and SPI_FLASH_MD5 over the region compared with
//! the MD5 of what was sent. FLASH_END with the reboot flag ends it.
//!
//! Every packet is one SLIP frame (0xC0 at both ends, 0xC0 as 0xDB 0xDC,
//! 0xDB as 0xDB 0xDD). A request is `00 cmd size(u16) checksum(u32) data`,
//! a response `01 cmd size(u16) value(u32) data`, little-endian; the last
//! four bytes of a ROM response's data are its status (0 success) and its
//! error code. Bytes that are no frame of the expected command (the ROM's
//! boot log, further SYNC responses) are read past.

use dagon_port::{BusError, StreamPort};

use crate::{HostClock, Region};

/// Bytes of flash data in one FLASH_DATA.
pub const BLOCK: usize = 0x400;
/// How long SYNC is sent to a node that said it enters the loader
/// ([`Plan::sync_ms`]).
pub const SYNC_MS: u32 = 3_000;
/// How long one SYNC waits for its response.
pub const SYNC_WAIT_MS: u32 = 100;
/// Timeout of a command that neither erases nor hashes.
pub const REPLY_MS: u32 = 3_000;
/// FLASH_BEGIN: the loader erases the region before it answers.
pub const ERASE_MS_PER_MIB: u32 = 30_000;
/// SPI_FLASH_MD5: the loader reads the region back.
pub const MD5_MS_PER_MIB: u32 = 8_000;

const END: u8 = 0xC0;
const ESC: u8 = 0xDB;
const ESC_END: u8 = 0xDC;
const ESC_ESC: u8 = 0xDD;
const HEADER: usize = 8;
const TRAILER: usize = 4;
/// Largest response read whole: SPI_FLASH_MD5's 32 characters and status.
const MAX_RESPONSE: usize = HEADER + 32 + TRAILER;
const CHECKSUM_SEED: u8 = 0xEF;

pub mod cmd {
    pub const FLASH_BEGIN: u8 = 0x02;
    pub const FLASH_DATA: u8 = 0x03;
    pub const FLASH_END: u8 = 0x04;
    pub const SYNC: u8 = 0x08;
    pub const SPI_SET_PARAMS: u8 = 0x0B;
    pub const SPI_ATTACH: u8 = 0x0D;
    pub const SPI_FLASH_MD5: u8 = 0x13;
}

/// The loader's stream and a clock.
#[allow(async_fn_in_trait)]
pub trait EspLink: StreamPort + HostClock {
    /// Write progress, 0..=100.
    async fn progress(&mut self, _percent: u8) {}
}

/// What to write: the flash size the loader is told, then the regions in
/// this order.
#[derive(Clone, Copy, Debug)]
pub struct Plan<'a> {
    pub flash_bytes: u32,
    pub regions: &'a [Region<'a>],
    /// FLASH_END asks the loader to reboot the chip.
    pub reboot: bool,
    /// How long SYNC is sent, one every [`SYNC_WAIT_MS`], before the
    /// loader counts as absent: [`SYNC_MS`] after ENTER_ROM, longer while
    /// a person brings the chip into the loader by hand.
    pub sync_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EspError {
    Bus(BusError),
    /// No response to SYNC within [`Plan::sync_ms`]: the loader is not
    /// running on this stream.
    NoSync,
    /// No response to this command within its time.
    Timeout(u8),
    /// The loader refused this command with this error code.
    Failed {
        cmd: u8,
        code: u8,
    },
    /// A response of the right command that is no response of its kind.
    Malformed(u8),
    /// The loader's MD5 of the region at this address differs.
    Mismatch(u32),
}

impl From<BusError> for EspError {
    fn from(e: BusError) -> Self {
        EspError::Bus(e)
    }
}

/// Run record for timing reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// SYNC packets sent until the loader answered.
    pub syncs: u32,
    /// FLASH_DATA blocks written.
    pub blocks: u32,
    pub elapsed_ms: u64,
}

/// The XOR checksum of a FLASH_DATA's data.
pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(CHECKSUM_SEED, |x, b| x ^ b)
}

/// Appends `bytes` SLIP-escaped to `out[at..]`; the new length.
fn escape(out: &mut [u8], mut at: usize, bytes: &[u8]) -> usize {
    for &b in bytes {
        match b {
            END => {
                out[at] = ESC;
                out[at + 1] = ESC_END;
                at += 2;
            }
            ESC => {
                out[at] = ESC;
                out[at + 1] = ESC_ESC;
                at += 2;
            }
            b => {
                out[at] = b;
                at += 1;
            }
        }
    }
    at
}

/// The SLIP frame of one request in `out`; its length. `out` holds
/// `2 * (8 + head.len() + body.len()) + 2` bytes at most.
pub fn request(out: &mut [u8], cmd: u8, check: u8, head: &[u8], body: &[u8]) -> usize {
    let size = (head.len() + body.len()) as u16;
    let mut header = [0u8; HEADER];
    header[1] = cmd;
    header[2..4].copy_from_slice(&size.to_le_bytes());
    header[4] = check;
    out[0] = END;
    let at = escape(out, 1, &header);
    let at = escape(out, at, head);
    let at = escape(out, at, body);
    out[at] = END;
    at + 1
}

/// Cuts SLIP frames out of a byte stream. A frame longer than the largest
/// response is dropped whole.
#[derive(Debug)]
struct Unslip {
    frame: [u8; MAX_RESPONSE],
    len: usize,
    open: bool,
    escaped: bool,
    over: bool,
}

impl Unslip {
    fn new() -> Self {
        Unslip {
            frame: [0; MAX_RESPONSE],
            len: 0,
            open: false,
            escaped: false,
            over: false,
        }
    }

    /// Takes one byte; a whole frame's length when this byte closed one.
    fn push(&mut self, b: u8) -> Option<usize> {
        if b == END {
            let done = self.open && self.len > 0 && !self.over;
            let len = self.len;
            self.open = true;
            self.len = 0;
            self.escaped = false;
            self.over = false;
            return done.then_some(len);
        }
        if !self.open {
            return None;
        }
        let b = match (self.escaped, b) {
            (false, ESC) => {
                self.escaped = true;
                return None;
            }
            (true, ESC_END) => END,
            (true, ESC_ESC) => ESC,
            (_, b) => b,
        };
        self.escaped = false;
        if self.len == self.frame.len() {
            self.over = true;
        } else {
            self.frame[self.len] = b;
            self.len += 1;
        }
        None
    }
}

/// A response's data before its status.
struct Answer {
    data: [u8; 32],
    len: usize,
}

struct Session<'l, L: EspLink> {
    link: &'l mut L,
    cut: Unslip,
    /// One request frame: a block and its headers, every byte escaped.
    out: [u8; 2 * (HEADER + 16 + BLOCK) + 2],
}

impl<L: EspLink> Session<'_, L> {
    /// Sends one request and waits `wait_ms` for the response of `cmd`.
    async fn command(
        &mut self,
        cmd: u8,
        check: u8,
        head: &[u8],
        body: &[u8],
        wait_ms: u32,
    ) -> Result<Answer, EspError> {
        let n = request(&mut self.out, cmd, check, head, body);
        self.link.write(&self.out[..n]).await?;
        let until = self.link.now_ms() + u64::from(wait_ms);
        loop {
            let left = until.saturating_sub(self.link.now_ms());
            if left == 0 {
                return Err(EspError::Timeout(cmd));
            }
            let mut buf = [0u8; 64];
            let got = match self.link.read(&mut buf, left as u32).await {
                Ok(n) => n,
                Err(BusError::Timeout) => return Err(EspError::Timeout(cmd)),
                Err(e) => return Err(e.into()),
            };
            for &b in &buf[..got] {
                let Some(len) = self.cut.push(b) else {
                    continue;
                };
                let f = &self.cut.frame[..len];
                if len < HEADER || f[0] != 0x01 || f[1] != cmd {
                    continue;
                }
                let size = usize::from(u16::from_le_bytes([f[2], f[3]]));
                if size < TRAILER || HEADER + size != len {
                    return Err(EspError::Malformed(cmd));
                }
                let data = &f[HEADER..len - TRAILER];
                let status = &f[len - TRAILER..];
                if status[0] != 0 {
                    return Err(EspError::Failed {
                        cmd,
                        code: status[1],
                    });
                }
                let mut answer = Answer {
                    data: [0; 32],
                    len: data.len(),
                };
                answer.data[..data.len()].copy_from_slice(data);
                return Ok(answer);
            }
        }
    }

    async fn sync(&mut self, attempts: u32) -> Result<u32, EspError> {
        let mut payload = [0x55u8; 36];
        payload[..4].copy_from_slice(&[0x07, 0x07, 0x12, 0x20]);
        for sent in 1..=attempts {
            match self
                .command(cmd::SYNC, 0, &payload, &[], SYNC_WAIT_MS)
                .await
            {
                Ok(_) => return Ok(sent),
                Err(EspError::Timeout(_) | EspError::Failed { .. } | EspError::Malformed(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Err(EspError::NoSync)
    }
}

fn words<const N: usize>(w: [u32; N]) -> [[u8; 4]; N] {
    w.map(u32::to_le_bytes)
}

fn per_mib(bytes: u32, ms_per_mib: u32) -> u32 {
    let ms = u64::from(bytes) * u64::from(ms_per_mib) / (1 << 20);
    REPLY_MS.max(ms.min(u64::from(u32::MAX)) as u32)
}

/// Writes `plan` through the loader on `link` and checks every region by
/// the loader's MD5 of it.
pub async fn flash<L: EspLink>(link: &mut L, plan: &Plan<'_>) -> Result<Stats, EspError> {
    let started = link.now_ms();
    let mut s = Session {
        link,
        cut: Unslip::new(),
        out: [0; 2 * (HEADER + 16 + BLOCK) + 2],
    };
    let mut stats = Stats {
        syncs: s.sync(plan.sync_ms.div_ceil(SYNC_WAIT_MS).max(1)).await?,
        ..Stats::default()
    };
    s.command(
        cmd::SPI_ATTACH,
        0,
        words([0, 0]).as_flattened(),
        &[],
        REPLY_MS,
    )
    .await?;
    let params = words([0, plan.flash_bytes, 0x1_0000, 0x1000, 0x100, 0xFFFF]);
    s.command(cmd::SPI_SET_PARAMS, 0, params.as_flattened(), &[], REPLY_MS)
        .await?;
    let total: u64 = plan.regions.iter().map(|r| r.data.len() as u64).sum();
    let mut written = 0u64;
    for r in plan.regions {
        let len = r.data.len() as u32;
        let blocks = len.div_ceil(BLOCK as u32);
        let begin = words([len, blocks, BLOCK as u32, r.addr, 0]);
        let wait = per_mib(len, ERASE_MS_PER_MIB);
        s.command(cmd::FLASH_BEGIN, 0, begin.as_flattened(), &[], wait)
            .await?;
        for (seq, part) in r.data.chunks(BLOCK).enumerate() {
            let mut block = [0xFFu8; BLOCK];
            block[..part.len()].copy_from_slice(part);
            let head = words([BLOCK as u32, seq as u32, 0, 0]);
            let check = checksum(&block);
            s.command(
                cmd::FLASH_DATA,
                check,
                head.as_flattened(),
                &block,
                REPLY_MS,
            )
            .await?;
            stats.blocks += 1;
            written += part.len() as u64;
            s.link.progress((written * 100 / total.max(1)) as u8).await;
        }
        let ask = words([r.addr, len, 0, 0]);
        let wait = per_mib(len, MD5_MS_PER_MIB);
        let answer = s
            .command(cmd::SPI_FLASH_MD5, 0, ask.as_flattened(), &[], wait)
            .await?;
        if answer.len != 32 {
            return Err(EspError::Malformed(cmd::SPI_FLASH_MD5));
        }
        let mut hex = [0u8; 32];
        for (i, b) in md5(r.data).iter().enumerate() {
            hex[2 * i] = b"0123456789abcdef"[usize::from(b >> 4)];
            hex[2 * i + 1] = b"0123456789abcdef"[usize::from(b & 15)];
        }
        if !answer.data.eq_ignore_ascii_case(&hex) {
            return Err(EspError::Mismatch(r.addr));
        }
    }
    // 0 reboots the chip, 1 leaves the loader running.
    let stay = words([u32::from(!plan.reboot)]);
    s.command(cmd::FLASH_END, 0, stay.as_flattened(), &[], REPLY_MS)
        .await?;
    stats.elapsed_ms = s.link.now_ms() - started;
    Ok(stats)
}

/// MD5 (RFC 1321) of `data`: what SPI_FLASH_MD5 answers with.
pub fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // floor(2^32 * abs(sin(i + 1))), RFC 1321 section 3.4.
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    let mut block = |chunk: &[u8; 64]| {
        let mut m = [0u32; 16];
        for (w, b) in m.iter_mut().zip(chunk.chunks_exact(4)) {
            *w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let x = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            (a, d, c) = (d, c, b);
            b = b.wrapping_add(x.rotate_left(S[i]));
        }
        for (s, v) in h.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    };
    let mut chunks = data.chunks_exact(64);
    for c in &mut chunks {
        block(c.try_into().expect("64 bytes"));
    }
    // The rest, 0x80, zeros, and the length in bits in the last 8 bytes.
    let rest = chunks.remainder();
    let mut tail = [0u8; 128];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let end = if rest.len() < 56 { 64 } else { 128 };
    tail[end - 8..end].copy_from_slice(&((data.len() as u64) * 8).to_le_bytes());
    for c in tail[..end].chunks_exact(64) {
        block(c.try_into().expect("64 bytes"));
    }
    let mut out = [0u8; 16];
    for (o, v) in out.chunks_exact_mut(4).zip(h) {
        o.copy_from_slice(&v.to_le_bytes());
    }
    out
}
