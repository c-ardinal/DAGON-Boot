//! SimRom: the STM32 system-memory boot loader on I2C (ST AN4221), as seen by
//! an I2C controller at 7-bit address [`ROM_ADDR`].
//!
//! Every host frame is one I2C transaction. A write is a "send command" or
//! "send data" frame; reads drain the bytes the loader has queued (ACK/NACK,
//! data, BUSY while a No-Stretch erase or write is still running).
//!
//! - Get `00 FF` -> ACK, `N`, version, `N` command codes, ACK
//! - Get Version `01 FE` -> ACK, version, ACK
//! - Get ID `02 FD` -> ACK, `01`, PID high, PID low, ACK
//! - Read Memory `11 EE` -> ACK; address (4 B big-endian + XOR) -> ACK;
//!   `N-1` + complement -> ACK, `N` bytes
//! - Write Memory `31 CE` / No-Stretch `32 CD` -> ACK; address -> ACK;
//!   `N-1`, `N` data bytes, XOR of all -> ACK (No-Stretch: BUSY until written)
//! - Erase `44 BB` / No-Stretch `45 BA` -> ACK; count `N-1` (2 B big-endian)
//!   and XOR -> ACK, then `N` sector numbers (2 B each) and XOR -> ACK when
//!   erased; count `FFFF` with XOR `00` erases everything in one step
//!
//! Reads and writes carry at most 256 bytes. More than 1000 ms between two
//! frames while a command is open resets the loader (the command is lost).

use crate::flash::SimFlash;
use dagon_port::Flash;
use std::collections::VecDeque;

pub const ROM_ADDR: u8 = 0x39;
pub const ACK: u8 = 0x79;
pub const NACK: u8 = 0x1F;
pub const BUSY: u8 = 0x76;
pub const FRAME_TIMEOUT_MS: u64 = 1000;
pub const MAX_RW: usize = 256;

pub mod cmd {
    pub const GET: u8 = 0x00;
    pub const GET_VERSION: u8 = 0x01;
    pub const GET_ID: u8 = 0x02;
    pub const READ_MEMORY: u8 = 0x11;
    pub const WRITE_MEMORY: u8 = 0x31;
    pub const NS_WRITE_MEMORY: u8 = 0x32;
    pub const ERASE: u8 = 0x44;
    pub const NS_ERASE: u8 = 0x45;
}

const COMMANDS: [u8; 8] = [
    cmd::GET,
    cmd::GET_VERSION,
    cmd::GET_ID,
    cmd::READ_MEMORY,
    cmd::WRITE_MEMORY,
    cmd::NS_WRITE_MEMORY,
    cmd::ERASE,
    cmd::NS_ERASE,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    Command,
    ReadAddr,
    ReadLen(u32),
    WriteAddr { stretch: bool },
    WriteData { addr: u32, stretch: bool },
    EraseCount { stretch: bool },
    ErasePages { stretch: bool, n: usize },
}

#[derive(Clone, Copy, Debug)]
enum Out {
    Byte(u8),
    /// ACK once the clock reaches this time; BUSY before.
    AckAt(u64),
}

pub struct SimRom {
    /// I2C protocol version reported by Get (F401xD/E: V1.1).
    pub version: u8,
    pub product_id: u16,
    pub erase_ms_per_kib: u64,
    /// Time to program one Write Memory frame.
    pub write_ms: u64,
    /// Self-resets caused by the frame timeout.
    pub timeouts: u32,
    expect: Expect,
    out: VecDeque<Out>,
    last_ms: u64,
}

impl Default for SimRom {
    /// STM32F401xD/E.
    fn default() -> Self {
        SimRom {
            version: 0x11,
            product_id: 0x0433,
            erase_ms_per_kib: 16,
            write_ms: 4,
            timeouts: 0,
            expect: Expect::Command,
            out: VecDeque::new(),
            last_ms: 0,
        }
    }
}

fn xor(b: &[u8]) -> u8 {
    b.iter().fold(0, |a, &x| a ^ x)
}

impl SimRom {
    /// Loader start after reset: no command open.
    pub fn restart(&mut self, now_ms: u64) {
        self.expect = Expect::Command;
        self.out.clear();
        self.last_ms = now_ms;
    }

    fn frame_start(&mut self, now_ms: u64) {
        let open = self.expect != Expect::Command || !self.out.is_empty();
        if open && now_ms.saturating_sub(self.last_ms) > FRAME_TIMEOUT_MS {
            self.timeouts += 1;
            self.restart(now_ms);
        }
        self.last_ms = now_ms;
    }

    /// Host write transaction. `now_ms` advances while a stretching command
    /// holds the bus.
    pub fn write(&mut self, flash: &mut SimFlash, data: &[u8], now_ms: &mut u64) {
        self.frame_start(*now_ms);
        self.out.clear();
        let (next, reply) = self.step(flash, data, now_ms);
        self.expect = next;
        if !reply {
            self.out.push_back(Out::Byte(NACK));
        }
    }

    /// Host read transaction.
    pub fn read(&mut self, buf: &mut [u8], now_ms: u64) {
        self.frame_start(now_ms);
        for b in buf {
            *b = match self.out.front().copied() {
                Some(Out::Byte(x)) => {
                    self.out.pop_front();
                    x
                }
                Some(Out::AckAt(t)) if now_ms >= t => {
                    self.out.pop_front();
                    ACK
                }
                Some(Out::AckAt(_)) => BUSY,
                None => NACK,
            };
        }
    }

    fn ack(&mut self, bytes: &[u8]) {
        self.out.push_back(Out::Byte(ACK));
        self.out.extend(bytes.iter().map(|&b| Out::Byte(b)));
    }

    /// Finishes a long operation: No-Stretch answers BUSY until done,
    /// the stretching variant holds the clock for the duration.
    fn finish(&mut self, stretch: bool, ms: u64, now_ms: &mut u64) {
        if stretch {
            *now_ms += ms;
            self.ack(&[]);
        } else {
            self.out.push_back(Out::AckAt(*now_ms + ms));
        }
    }

    /// Returns the next expectation and whether a reply was queued (`false`
    /// queues NACK and returns to command).
    fn step(&mut self, flash: &mut SimFlash, d: &[u8], now_ms: &mut u64) -> (Expect, bool) {
        match self.expect {
            Expect::Command => self.command(d),
            Expect::ReadAddr | Expect::WriteAddr { .. } => match parse_addr(flash, d) {
                Some(addr) => {
                    self.ack(&[]);
                    let next = match self.expect {
                        Expect::WriteAddr { stretch } => Expect::WriteData { addr, stretch },
                        _ => Expect::ReadLen(addr),
                    };
                    (next, true)
                }
                None => (Expect::Command, false),
            },
            Expect::ReadLen(addr) => {
                let mut buf = vec![0u8; d.first().map_or(0, |&n| n as usize + 1)];
                let ok = d.len() == 2 && d[1] == !d[0] && flash.read(addr, &mut buf).is_ok();
                if ok {
                    self.ack(&buf);
                }
                (Expect::Command, ok)
            }
            Expect::WriteData { addr, stretch } => {
                let n = d.first().map_or(0, |&n| n as usize + 1);
                let ok = d.len() == n + 2
                    && xor(&d[..n + 1]) == d[n + 1]
                    && flash.write(addr, &d[1..n + 1]).is_ok();
                if ok {
                    self.finish(stretch, self.write_ms, now_ms);
                }
                (Expect::Command, ok)
            }
            Expect::EraseCount { stretch } => {
                if d.len() != 3 || d[0] ^ d[1] != d[2] {
                    return (Expect::Command, false);
                }
                match u16::from_be_bytes([d[0], d[1]]) {
                    0xFFFF => {
                        let all: Vec<u32> = (0..)
                            .map_while(|i| flash.sector_addr(i).map(|_| i))
                            .collect();
                        let ok = self.erase(flash, &all, stretch, now_ms);
                        (Expect::Command, ok)
                    }
                    0xFFF0..=0xFFFE => (Expect::Command, false),
                    c => {
                        self.ack(&[]);
                        let n = c as usize + 1;
                        (Expect::ErasePages { stretch, n }, true)
                    }
                }
            }
            Expect::ErasePages { stretch, n } => {
                if d.len() != 2 * n + 1 || xor(&d[..2 * n]) != d[2 * n] {
                    return (Expect::Command, false);
                }
                let sectors: Vec<u32> = d[..2 * n]
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]) as u32)
                    .collect();
                (
                    Expect::Command,
                    self.erase(flash, &sectors, stretch, now_ms),
                )
            }
        }
    }

    fn command(&mut self, d: &[u8]) -> (Expect, bool) {
        if d.len() != 2 || d[1] != !d[0] || !COMMANDS.contains(&d[0]) {
            return (Expect::Command, false);
        }
        let next = match d[0] {
            cmd::GET => {
                let mut b = vec![COMMANDS.len() as u8, self.version];
                b.extend_from_slice(&COMMANDS);
                b.push(ACK);
                self.ack(&b);
                Expect::Command
            }
            cmd::GET_VERSION => {
                self.ack(&[self.version, ACK]);
                Expect::Command
            }
            cmd::GET_ID => {
                let [hi, lo] = self.product_id.to_be_bytes();
                self.ack(&[0x01, hi, lo, ACK]);
                Expect::Command
            }
            cmd::READ_MEMORY => {
                self.ack(&[]);
                Expect::ReadAddr
            }
            c @ (cmd::WRITE_MEMORY | cmd::NS_WRITE_MEMORY) => {
                self.ack(&[]);
                Expect::WriteAddr {
                    stretch: c == cmd::WRITE_MEMORY,
                }
            }
            c => {
                self.ack(&[]);
                Expect::EraseCount {
                    stretch: c == cmd::ERASE,
                }
            }
        };
        (next, true)
    }

    fn erase(
        &mut self,
        flash: &mut SimFlash,
        sectors: &[u32],
        stretch: bool,
        now_ms: &mut u64,
    ) -> bool {
        let mut kib = 0u64;
        for &s in sectors {
            let Some((a, len)) = flash.sector_addr(s) else {
                return false;
            };
            if flash.erase(a, len).is_err() {
                return false;
            }
            kib += len as u64 / 1024;
        }
        self.finish(stretch, kib * self.erase_ms_per_kib, now_ms);
        true
    }
}

fn parse_addr(flash: &SimFlash, d: &[u8]) -> Option<u32> {
    if d.len() != 5 || xor(&d[..4]) != d[4] {
        return None;
    }
    let addr = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    flash.unit_at(addr).map(|_| addr)
}
