//! STM32 system-memory boot loader on I2C (ST AN4221), driven from the host
//! through the hub's NRST/BOOT0 lines and raw I2C transactions.
//!
//! [`flash`] resets the node into the ROM loader, reads the command list
//! (Get), erases the listed sectors, writes the regions in order in frames
//! of at most 256 bytes, reads everything back, and resets the node into
//! its application. No-Stretch Erase / Write Memory (0x45 / 0x32) are used
//! when Get lists them, the clock-stretching 0x44 / 0x31 otherwise.
//!
//! The loader drops an open command when two frames are more than 1000 ms
//! apart; this side keeps at most [`FRAME_GAP_MS`] between a response and
//! the next frame. Any NACK, unexpected byte, timeout or late frame abandons
//! the session and starts again from the reset, up to [`ATTEMPTS`] times.
#![no_std]
#![forbid(unsafe_code)]

use dagon_port::{BusError, I2cBus, SubControl};

pub const ROM_ADDR: u8 = 0x39;
pub const ACK: u8 = 0x79;
pub const NACK: u8 = 0x1F;
pub const BUSY: u8 = 0x76;
/// Largest Write / Read Memory frame.
pub const MAX_RW: usize = 256;
/// Longest pause from a response to the next frame (the loader's own limit
/// is 1000 ms).
pub const FRAME_GAP_MS: u64 = 500;
/// Timeout of one raw I2C transaction.
pub const FRAME_TIMEOUT_MS: u32 = 500;
/// Timeout of a clock-stretching erase of one sector (128 KiB: at most 4 s).
pub const STRETCH_ERASE_MS: u32 = 5_000;
/// How long a No-Stretch operation may answer BUSY.
pub const BUSY_LIMIT_MS: u64 = 5_000;
/// Pause between BUSY polls.
pub const BUSY_POLL_MS: u32 = 5;
pub const ATTEMPTS: u8 = 3;
/// Time for the loader to start after the reset pulse.
pub const ROM_START_MS: u32 = 50;

pub mod cmd {
    pub const GET: u8 = 0x00;
    pub const READ_MEMORY: u8 = 0x11;
    pub const WRITE_MEMORY: u8 = 0x31;
    pub const NS_WRITE_MEMORY: u8 = 0x32;
    pub const ERASE: u8 = 0x44;
    pub const NS_ERASE: u8 = 0x45;
}

/// Monotonic time source (shared with dagon-host).
#[allow(async_fn_in_trait)]
pub trait Clock {
    fn now_ms(&self) -> u64;
    async fn sleep_ms(&mut self, ms: u32);
}

/// The hub as seen by the ROM procedure: its I2C bus (all transactions go
/// to [`ROM_ADDR`]), the node's NRST/BOOT0 lines and a clock.
#[allow(async_fn_in_trait)]
pub trait RomLink: I2cBus + SubControl + Clock {
    /// Timeout for the following I2C transactions (a stretching command
    /// holds the bus for seconds).
    fn set_timeout_ms(&mut self, _ms: u32) {}
    /// Write and read-back progress, 0..=100.
    async fn progress(&mut self, _percent: u8) {}
}

/// Bytes to place at `addr`.
#[derive(Clone, Copy, Debug)]
pub struct Region<'a> {
    pub addr: u32,
    pub data: &'a [u8],
}

/// What to write: sectors to erase, then regions in this order.
#[derive(Clone, Copy, Debug)]
pub struct Plan<'a> {
    pub erase: &'a [u16],
    pub regions: &'a [Region<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RomError {
    Bus(BusError),
    /// The loader answered NACK (or another byte) where ACK was due.
    Refused(u8),
    /// The loader does not list Read Memory, or neither erase/write variant.
    MissingCommand(u8),
    /// BUSY for longer than [`BUSY_LIMIT_MS`].
    Busy,
    /// More than [`FRAME_GAP_MS`] passed before the next frame could go out.
    Late,
    /// Read-back differs at this address.
    Mismatch(u32),
}

impl From<BusError> for RomError {
    fn from(e: BusError) -> Self {
        RomError::Bus(e)
    }
}

/// Run record for timing reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Sessions started (1 when the first one succeeded).
    pub attempts: u8,
    /// I2C transactions in the successful session.
    pub frames: u32,
    /// Longest round trip of one transaction.
    pub max_rtt_ms: u32,
    /// No-Stretch commands were used.
    pub no_stretch: bool,
    pub elapsed_ms: u64,
    /// Error that ended the previous session, if any.
    pub last_error: Option<RomError>,
}

/// Writes `plan` through the ROM loader and restarts the node into its
/// application (BOOT0 low). On failure BOOT0 is left low as well.
pub async fn flash<L: RomLink>(l: &mut L, plan: &Plan<'_>) -> Result<Stats, RomError> {
    let t0 = l.now_ms();
    let mut stats = Stats::default();
    let r = loop {
        stats.attempts += 1;
        let mut s = Session::new(l);
        match s.run(plan).await {
            Ok(()) => {
                stats.frames = s.frames;
                stats.max_rtt_ms = s.max_rtt;
                stats.no_stretch = s.no_stretch;
                break Ok(());
            }
            Err(e) if stats.attempts >= ATTEMPTS => break Err(e),
            Err(e) => stats.last_error = Some(e),
        }
    };
    l.set_timeout_ms(FRAME_TIMEOUT_MS);
    let back = l.pulse_reset(false).await;
    r?;
    back?;
    stats.elapsed_ms = l.now_ms() - t0;
    Ok(stats)
}

struct Session<'l, L: RomLink> {
    l: &'l mut L,
    last: u64,
    frames: u32,
    max_rtt: u32,
    no_stretch: bool,
}

fn xor(b: &[u8]) -> u8 {
    b.iter().fold(0, |a, &x| a ^ x)
}

impl<'l, L: RomLink> Session<'l, L> {
    fn new(l: &'l mut L) -> Self {
        Session {
            l,
            last: 0,
            frames: 0,
            max_rtt: 0,
            no_stretch: false,
        }
    }

    async fn run(&mut self, plan: &Plan<'_>) -> Result<(), RomError> {
        self.l.set_timeout_ms(FRAME_TIMEOUT_MS);
        self.l.pulse_reset(true).await?;
        self.l.sleep_ms(ROM_START_MS).await;
        self.last = self.l.now_ms();
        self.get().await?;
        for &s in plan.erase {
            self.erase(s).await?;
        }
        let total: usize = plan.regions.iter().map(|r| r.data.len()).sum::<usize>() * 2;
        let mut done = 0;
        for pass in [false, true] {
            for r in plan.regions {
                for (i, c) in r.data.chunks(MAX_RW).enumerate() {
                    let addr = r.addr + (i * MAX_RW) as u32;
                    if pass {
                        self.verify(addr, c).await?;
                    } else {
                        self.write(addr, c).await?;
                    }
                    done += c.len();
                    self.l.progress((done * 100 / total.max(1)) as u8).await;
                }
            }
        }
        Ok(())
    }

    /// Runs one transaction, timing it and enforcing the frame gap.
    async fn frame(&mut self, data: &[u8], buf: &mut [u8]) -> Result<(), RomError> {
        let start = self.l.now_ms();
        if start - self.last > FRAME_GAP_MS {
            return Err(RomError::Late);
        }
        let r = match (data.is_empty(), buf.is_empty()) {
            (false, false) => self.l.write_read(ROM_ADDR, data, buf).await,
            (false, true) => I2cBus::write(self.l, ROM_ADDR, data).await,
            _ => I2cBus::read(self.l, ROM_ADDR, buf).await,
        };
        self.last = self.l.now_ms();
        self.frames += 1;
        self.max_rtt = self.max_rtt.max((self.last - start) as u32);
        Ok(r?)
    }

    /// Sends `data`, expects ACK.
    async fn send(&mut self, data: &[u8]) -> Result<(), RomError> {
        let mut a = [0u8];
        self.frame(data, &mut a).await?;
        expect_ack(a[0])
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<(), RomError> {
        self.frame(&[], buf).await
    }

    /// Polls a No-Stretch operation's state until ACK.
    async fn send_busy(&mut self, data: &[u8]) -> Result<(), RomError> {
        let mut a = [0u8];
        self.frame(data, &mut a).await?;
        let until = self.l.now_ms() + BUSY_LIMIT_MS;
        while a[0] == BUSY {
            if self.l.now_ms() >= until {
                return Err(RomError::Busy);
            }
            self.l.sleep_ms(BUSY_POLL_MS).await;
            self.read(&mut a).await?;
        }
        expect_ack(a[0])
    }

    async fn command(&mut self, c: u8) -> Result<(), RomError> {
        self.send(&[c, !c]).await
    }

    async fn get(&mut self) -> Result<(), RomError> {
        self.command(cmd::GET).await?;
        let mut n = [0u8];
        self.read(&mut n).await?;
        // Version, the N command codes, ACK.
        let mut b = [0u8; 258];
        let b = &mut b[..n[0] as usize + 2];
        self.read(b).await?;
        let (&last, rest) = b.split_last().expect("at least 2 bytes");
        expect_ack(last)?;
        let cmds = &rest[1..];
        let has = |c| cmds.contains(&c);
        self.no_stretch = has(cmd::NS_ERASE) && has(cmd::NS_WRITE_MEMORY);
        let stretch = has(cmd::ERASE) && has(cmd::WRITE_MEMORY);
        if !self.no_stretch && !stretch {
            return Err(RomError::MissingCommand(cmd::ERASE));
        }
        if !has(cmd::READ_MEMORY) {
            return Err(RomError::MissingCommand(cmd::READ_MEMORY));
        }
        Ok(())
    }

    async fn erase(&mut self, sector: u16) -> Result<(), RomError> {
        let c = if self.no_stretch {
            cmd::NS_ERASE
        } else {
            cmd::ERASE
        };
        self.command(c).await?;
        self.send(&[0, 0, 0]).await?;
        let [hi, lo] = sector.to_be_bytes();
        let f = [hi, lo, hi ^ lo];
        if self.no_stretch {
            self.send_busy(&f).await
        } else {
            self.l.set_timeout_ms(STRETCH_ERASE_MS);
            let r = self.send(&f).await;
            self.l.set_timeout_ms(FRAME_TIMEOUT_MS);
            r
        }
    }

    async fn address(&mut self, addr: u32) -> Result<(), RomError> {
        let a = addr.to_be_bytes();
        self.send(&[a[0], a[1], a[2], a[3], xor(&a)]).await
    }

    async fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), RomError> {
        let c = if self.no_stretch {
            cmd::NS_WRITE_MEMORY
        } else {
            cmd::WRITE_MEMORY
        };
        self.command(c).await?;
        self.address(addr).await?;
        let mut f = [0u8; MAX_RW + 2];
        f[0] = (data.len() - 1) as u8;
        f[1..=data.len()].copy_from_slice(data);
        f[data.len() + 1] = xor(&f[..=data.len()]);
        let f = &f[..data.len() + 2];
        if self.no_stretch {
            self.send_busy(f).await
        } else {
            self.send(f).await
        }
    }

    async fn verify(&mut self, addr: u32, data: &[u8]) -> Result<(), RomError> {
        self.command(cmd::READ_MEMORY).await?;
        self.address(addr).await?;
        let n = (data.len() - 1) as u8;
        self.send(&[n, !n]).await?;
        let mut b = [0u8; MAX_RW];
        let b = &mut b[..data.len()];
        self.read(b).await?;
        match b.iter().zip(data).position(|(x, y)| x != y) {
            Some(i) => Err(RomError::Mismatch(addr + i as u32)),
            None => Ok(()),
        }
    }
}

fn expect_ack(b: u8) -> Result<(), RomError> {
    if b == ACK {
        Ok(())
    } else {
        Err(RomError::Refused(b))
    }
}
