//! SimEspRom: the ESP32-S3 ROM serial loader (Espressif's serial protocol,
//! no stub), as seen on the byte stream of the UART it listens on.
//!
//! Every packet is one SLIP frame (0xC0 at both ends, 0xC0 as 0xDB 0xDC,
//! 0xDB as 0xDB 0xDD). A request is `00 cmd size(u16) checksum(u32) data`,
//! a response `01 cmd size(u16) value(u32) data status error 00 00`,
//! little-endian. Bytes outside a frame and frames that are no request are
//! dropped without an answer.
//!
//! - SYNC `08`: answered eight times.
//! - SPI_ATTACH `0D`: makes the flash reachable.
//! - SPI_SET_PARAMS `0B`: the flash size the following commands may reach.
//! - FLASH_BEGIN `02` (bytes, blocks, block size, offset): erases every
//!   sector the bytes touch.
//! - FLASH_DATA `03` (size, sequence, 0, 0, data): programs one block; the
//!   header's checksum is 0xEF XORed with every data byte.
//! - SPI_FLASH_MD5 `13` (address, size, 0, 0): 32 hexadecimal characters.
//! - FLASH_END `04` (0 reboot, 1 stay).
//!
//! Error codes: 0x05 for a request that is malformed, 0x06 for one that
//! cannot be acted on now, 0x07 for a wrong checksum, 0x08 when the flash
//! refuses the write.

use crate::flash::SimFlash;
use dagon_port::Flash;
use std::collections::VecDeque;

pub const INVALID: u8 = 0x05;
pub const FAILED: u8 = 0x06;
pub const BAD_CHECKSUM: u8 = 0x07;
pub const FLASH_WRITE: u8 = 0x08;
/// What SYNC answers before the next request.
pub const SYNC_ANSWERS: usize = 8;
/// Time one 4 KiB sector erase takes.
pub const ERASE_SECTOR_MS: u64 = 45;
/// Time programming one block takes.
pub const WRITE_BLOCK_MS: u64 = 4;
/// Time hashing 1 MiB takes.
pub const MD5_MS_PER_MIB: u64 = 1_000;

const END: u8 = 0xC0;
const ESC: u8 = 0xDB;
const SECTOR: u32 = 0x1000;
/// Largest request taken: 16 KiB of data and its headers.
const MAX_REQUEST: usize = 8 + 16 + 0x4000;

pub mod cmd {
    pub const FLASH_BEGIN: u8 = 0x02;
    pub const FLASH_DATA: u8 = 0x03;
    pub const FLASH_END: u8 = 0x04;
    pub const SYNC: u8 = 0x08;
    pub const SPI_SET_PARAMS: u8 = 0x0B;
    pub const SPI_ATTACH: u8 = 0x0D;
    pub const SPI_FLASH_MD5: u8 = 0x13;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Download {
    offset: u32,
    blocks: u32,
    block: u32,
    next: u32,
}

#[derive(Clone, Debug, Default)]
pub struct SimEspRom {
    frame: Vec<u8>,
    open: bool,
    escaped: bool,
    out: VecDeque<u8>,
    attached: bool,
    /// Flash size from SPI_SET_PARAMS.
    size: Option<u32>,
    download: Option<Download>,
    /// FLASH_END asked for a reboot: the loader no longer answers.
    rebooted: bool,
    /// Requests answered, by command, in order.
    seen: Vec<u8>,
}

fn word(data: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(data[4 * i..4 * i + 4].try_into().expect("4 bytes"))
}

impl SimEspRom {
    /// The loader just after the chip reset into it: `log` is what it has
    /// printed on the stream so far.
    pub fn started(log: &[u8]) -> Self {
        SimEspRom {
            out: log.iter().copied().collect(),
            ..SimEspRom::default()
        }
    }

    pub fn rebooted(&self) -> bool {
        self.rebooted
    }

    /// Commands answered so far, in order.
    pub fn seen(&self) -> &[u8] {
        &self.seen
    }

    /// Bytes from the host. Erasing, programming and hashing advance
    /// `now_ms` by the time they take.
    pub fn write(&mut self, flash: &mut SimFlash, data: &[u8], now_ms: &mut u64) {
        for &b in data {
            if b == END {
                if self.open && !self.frame.is_empty() {
                    let frame = std::mem::take(&mut self.frame);
                    self.request(flash, &frame, now_ms);
                }
                self.open = true;
                self.escaped = false;
                self.frame.clear();
                continue;
            }
            if !self.open {
                continue;
            }
            let b = match (self.escaped, b) {
                (false, ESC) => {
                    self.escaped = true;
                    continue;
                }
                (true, 0xDC) => END,
                (true, 0xDD) => ESC,
                (_, b) => b,
            };
            self.escaped = false;
            if self.frame.len() < MAX_REQUEST {
                self.frame.push(b);
            }
        }
    }

    /// Bytes to the host; how many were waiting, `buf.len()` at most.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.out.len());
        for (b, o) in buf.iter_mut().zip(self.out.drain(..n)) {
            *b = o;
        }
        n
    }

    fn request(&mut self, flash: &mut SimFlash, f: &[u8], now_ms: &mut u64) {
        if self.rebooted || f.len() < 8 || f[0] != 0x00 {
            return;
        }
        let op = f[1];
        let size = usize::from(u16::from_le_bytes([f[2], f[3]]));
        let check = f[4];
        let data = &f[8..];
        let result = if size != data.len() {
            Err(INVALID)
        } else {
            self.act(flash, op, check, data, now_ms)
        };
        self.seen.push(op);
        let (body, status): (&[u8], [u8; 4]) = match &result {
            Ok(body) => (body, [0; 4]),
            Err(code) => (&[], [1, *code, 0, 0]),
        };
        let mut packet = vec![0x01, op];
        packet.extend(((body.len() + 4) as u16).to_le_bytes());
        packet.extend([0u8; 4]);
        packet.extend(body);
        packet.extend(status);
        let times = if op == cmd::SYNC { SYNC_ANSWERS } else { 1 };
        for _ in 0..times {
            self.out.push_back(END);
            for &b in &packet {
                match b {
                    END => self.out.extend([ESC, 0xDC]),
                    ESC => self.out.extend([ESC, 0xDD]),
                    b => self.out.push_back(b),
                }
            }
            self.out.push_back(END);
        }
    }

    fn act(
        &mut self,
        flash: &mut SimFlash,
        op: u8,
        check: u8,
        data: &[u8],
        now_ms: &mut u64,
    ) -> Result<Vec<u8>, u8> {
        let words = |n: usize| if data.len() == 4 * n { Ok(()) } else { Err(INVALID) };
        match op {
            cmd::SYNC => {
                if data.len() != 36 || data[..4] != [0x07, 0x07, 0x12, 0x20] {
                    return Err(INVALID);
                }
            }
            cmd::SPI_ATTACH => {
                words(2)?;
                self.attached = true;
            }
            cmd::SPI_SET_PARAMS => {
                words(6)?;
                self.size = Some(word(data, 1));
            }
            cmd::FLASH_BEGIN => {
                // The fifth word (encrypted write) is the ESP32-S3 ROM's.
                words(5)?;
                let (bytes, blocks, block, offset) =
                    (word(data, 0), word(data, 1), word(data, 2), word(data, 3));
                let end = self.reach(offset, bytes)?;
                if block == 0 || blocks != bytes.div_ceil(block) {
                    return Err(INVALID);
                }
                let first = offset / SECTOR * SECTOR;
                let len = end.div_ceil(SECTOR) * SECTOR - first;
                flash.erase(first, len).map_err(|_| FAILED)?;
                *now_ms += u64::from(len / SECTOR) * ERASE_SECTOR_MS;
                self.download = Some(Download {
                    offset,
                    blocks,
                    block,
                    next: 0,
                });
            }
            cmd::FLASH_DATA => {
                if data.len() < 16 {
                    return Err(INVALID);
                }
                let (len, seq) = (word(data, 0), word(data, 1));
                let body = &data[16..];
                let d = self.download.as_mut().ok_or(FAILED)?;
                if len as usize != body.len() || len != d.block {
                    return Err(INVALID);
                }
                if body.iter().fold(0xEF, |x, b| x ^ b) != check {
                    return Err(BAD_CHECKSUM);
                }
                if seq != d.next || seq >= d.blocks {
                    return Err(FAILED);
                }
                flash
                    .write(d.offset + seq * d.block, body)
                    .map_err(|_| FLASH_WRITE)?;
                d.next += 1;
                *now_ms += WRITE_BLOCK_MS;
            }
            cmd::SPI_FLASH_MD5 => {
                words(4)?;
                let (addr, len) = (word(data, 0), word(data, 1));
                self.reach(addr, len)?;
                let bytes = flash.peek(addr, len).ok_or(FAILED)?;
                *now_ms += u64::from(len) * MD5_MS_PER_MIB / (1 << 20);
                return Ok(dagon_romlink::esp::md5(bytes)
                    .iter()
                    .flat_map(|b| format!("{b:02x}").into_bytes())
                    .collect());
            }
            cmd::FLASH_END => {
                words(1)?;
                self.download = None;
                self.rebooted = word(data, 0) == 0;
            }
            _ => return Err(INVALID),
        }
        Ok(Vec::new())
    }

    /// The end of `len` bytes at `addr`, when the attached flash holds them.
    fn reach(&self, addr: u32, len: u32) -> Result<u32, u8> {
        let size = self.size.filter(|_| self.attached).ok_or(FAILED)?;
        addr.checked_add(len).filter(|&e| e <= size).ok_or(FAILED)
    }
}
