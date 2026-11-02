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
/// Per-sector erase budget: the datasheet maximum for a 128 KiB sector is
/// 4 s; the procedure allows 10 s.
pub const SECTOR_ERASE_MS: u32 = 10_000;
/// Timeout of a clock-stretching erase of one sector.
pub const STRETCH_ERASE_MS: u32 = SECTOR_ERASE_MS;
/// How long a No-Stretch operation may answer BUSY.
pub const BUSY_LIMIT_MS: u64 = SECTOR_ERASE_MS as u64;
/// N in the Get reply of an I2C protocol V1.1 loader (STM32F401: 17
/// command codes, No-Stretch variants included).
pub const GET_GUESS_N: u8 = 17;
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

    /// Get: ACK frame, data frame (N, version, N command codes), ACK frame,
    /// each one I2C read. The data frame's length is only known from its
    /// first byte, so it is read at the length of a V1.1 loader's list and,
    /// if N says otherwise, Get is sent again and read at the exact length
    /// (as stm32flash does on I2C).
    async fn get(&mut self) -> Result<(), RomError> {
        let mut b = [0u8; 258];
        let mut n = GET_GUESS_N;
        let len = loop {
            self.command(cmd::GET).await?;
            let frame = &mut b[..n as usize + 2];
            self.read(frame).await?;
            if frame[0] == n {
                break frame.len();
            }
            if n != GET_GUESS_N {
                return Err(RomError::Refused(frame[0]));
            }
            // The rest of this reply is lost with the frame; the loader
            // takes a new command after its trailing ACK.
            let mut a = [0u8];
            self.read(&mut a).await?;
            n = frame[0];
        };
        let mut a = [0u8];
        self.read(&mut a).await?;
        expect_ack(a[0])?;
        let cmds = &b[2..len];
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

#[cfg(test)]
mod tests {
    //! A strict loader model: every reply frame is taken by exactly one I2C
    //! read, and a read shorter than the frame is an error.
    extern crate std;

    use super::*;
    use dagon_port::Nrst;
    use std::collections::{BTreeMap, VecDeque};
    use std::vec;
    use std::vec::Vec;

    #[derive(Clone, Copy, PartialEq)]
    enum St {
        Command,
        ReadAddr,
        ReadLen(u32),
        WriteAddr,
        WriteData(u32),
        EraseCount,
        ErasePages,
    }

    struct Fake {
        cmds: Vec<u8>,
        mem: BTreeMap<u32, u8>,
        frames: VecDeque<Vec<u8>>,
        st: St,
        now: u64,
        gets: u32,
        split_reads: u32,
        fail_erase_once: bool,
        late_once: bool,
        boot0: bool,
        stretch: bool,
    }

    fn fake(cmds: &[u8]) -> Fake {
        Fake {
            cmds: cmds.to_vec(),
            mem: BTreeMap::new(),
            frames: VecDeque::new(),
            st: St::Command,
            now: 0,
            gets: 0,
            split_reads: 0,
            fail_erase_once: false,
            late_once: false,
            boot0: false,
            stretch: false,
        }
    }

    /// All 17 codes of a V1.1 loader.
    const V11: [u8; 17] = [
        0x00, 0x01, 0x02, 0x11, 0x21, 0x31, 0x32, 0x44, 0x45, 0x63, 0x64, 0x73, 0x74, 0x82, 0x83,
        0x92, 0x93,
    ];

    impl Fake {
        fn reply(&mut self, frames: &[&[u8]]) {
            self.frames.extend(frames.iter().map(|f| f.to_vec()));
        }

        fn step(&mut self, d: &[u8]) {
            self.frames.clear();
            let st = core::mem::replace(&mut self.st, St::Command);
            let addr = (d.len() == 5 && xor(&d[..4]) == d[4])
                .then(|| u32::from_be_bytes([d[0], d[1], d[2], d[3]]));
            // Stretching commands answer ACK when done, No-Stretch ones BUSY
            // first.
            let done: &[&[u8]] = if self.stretch {
                &[&[ACK]]
            } else {
                &[&[BUSY], &[BUSY], &[ACK]]
            };
            let ok = match st {
                St::Command if d.len() == 2 && d[1] == !d[0] && self.cmds.contains(&d[0]) => {
                    self.reply(&[&[ACK]]);
                    self.stretch = matches!(d[0], cmd::WRITE_MEMORY | cmd::ERASE);
                    self.st = match d[0] {
                        cmd::GET => {
                            self.gets += 1;
                            let mut data = vec![self.cmds.len() as u8, 0x11];
                            data.extend_from_slice(&self.cmds);
                            self.reply(&[&data, &[ACK]]);
                            St::Command
                        }
                        cmd::READ_MEMORY => St::ReadAddr,
                        cmd::WRITE_MEMORY | cmd::NS_WRITE_MEMORY => St::WriteAddr,
                        _ => St::EraseCount,
                    };
                    true
                }
                St::ReadAddr | St::WriteAddr if addr.is_some() => {
                    let a = addr.expect("checked");
                    self.reply(&[&[ACK]]);
                    self.st = if st == St::ReadAddr {
                        St::ReadLen(a)
                    } else {
                        St::WriteData(a)
                    };
                    true
                }
                St::ReadLen(a) if d.len() == 2 && d[1] == !d[0] => {
                    let data: Vec<u8> = (0..=d[0] as u32)
                        .map(|i| *self.mem.get(&(a + i)).unwrap_or(&0xFF))
                        .collect();
                    self.reply(&[&[ACK], &data]);
                    true
                }
                St::WriteData(a)
                    if d.len() == d[0] as usize + 3 && xor(&d[..d.len() - 1]) == d[d.len() - 1] =>
                {
                    for (i, &b) in d[1..d.len() - 1].iter().enumerate() {
                        self.mem.insert(a + i as u32, b);
                    }
                    self.reply(done);
                    true
                }
                St::EraseCount if d == [0, 0, 0] => {
                    self.reply(&[&[ACK]]);
                    self.st = St::ErasePages;
                    true
                }
                St::ErasePages if d.len() == 3 && d[0] ^ d[1] == d[2] => {
                    let ok = !core::mem::take(&mut self.fail_erase_once);
                    if ok {
                        self.reply(done);
                    }
                    ok
                }
                _ => false,
            };
            if !ok {
                self.frames.clear();
                self.reply(&[&[NACK]]);
                self.st = St::Command;
            }
        }
    }

    impl I2cBus for Fake {
        async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
            assert_eq!(addr, ROM_ADDR);
            self.now += 1;
            self.step(data);
            Ok(())
        }
        async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
            assert_eq!(addr, ROM_ADDR);
            self.now += 1;
            let f = self.frames.pop_front().ok_or(BusError::Nack)?;
            if buf.len() < f.len() {
                self.split_reads += 1;
                return Err(BusError::Io);
            }
            buf.fill(0xFF);
            buf[..f.len()].copy_from_slice(&f);
            Ok(())
        }
        async fn write_read(&mut self, a: u8, d: &[u8], buf: &mut [u8]) -> Result<(), BusError> {
            I2cBus::write(self, a, d).await?;
            I2cBus::read(self, a, buf).await
        }
    }

    impl SubControl for Fake {
        async fn set_lines(&mut self, _nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
            self.boot0 = boot0_high;
            self.st = St::Command;
            self.frames.clear();
            Ok(())
        }
    }

    impl Clock for Fake {
        fn now_ms(&self) -> u64 {
            self.now
        }
        async fn sleep_ms(&mut self, ms: u32) {
            self.now += ms as u64;
        }
    }

    impl RomLink for Fake {
        async fn progress(&mut self, _percent: u8) {
            if core::mem::take(&mut self.late_once) {
                self.now += FRAME_GAP_MS + 100;
            }
        }
    }

    fn block_on<F: core::future::Future>(f: F) -> F::Output {
        let mut f = core::pin::pin!(f);
        let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
        loop {
            if let core::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    const BODY: [u8; 600] = [0x5A; 600];
    const HDR: [u8; 16] = [0xC3; 16];
    const REGIONS: [Region<'static>; 2] = [
        Region {
            addr: 0x0802_0200,
            data: &BODY,
        },
        Region {
            addr: 0x0802_0000,
            data: &HDR,
        },
    ];
    const PLAN: Plan<'static> = Plan {
        erase: &[5],
        regions: &REGIONS,
    };

    fn written(f: &Fake, r: &Region<'_>) -> bool {
        r.data
            .iter()
            .enumerate()
            .all(|(i, b)| f.mem.get(&(r.addr + i as u32)) == Some(b))
    }

    #[test]
    fn writes_with_one_read_per_frame() {
        for cmds in [&V11[..], &[0x00, 0x01, 0x02, 0x11, 0x31, 0x44][..]] {
            let mut f = fake(cmds);
            let s = block_on(flash(&mut f, &PLAN)).unwrap();
            assert_eq!(f.split_reads, 0);
            assert!(REGIONS.iter().all(|r| written(&f, r)));
            assert_eq!((s.attempts, s.no_stretch), (1, cmds.len() == 17));
            // A V1.1 list is read in one go; another length takes a second Get.
            assert_eq!(f.gets, if cmds.len() == 17 { 1 } else { 2 });
            assert!(!f.boot0, "back to the application");
        }
    }

    #[test]
    fn a_failed_session_restarts_from_the_reset() {
        let mut f = fake(&V11);
        f.fail_erase_once = true;
        let s = block_on(flash(&mut f, &PLAN)).unwrap();
        assert_eq!(s.attempts, 2);
        assert_eq!(s.last_error, Some(RomError::Refused(NACK)));
        assert!(REGIONS.iter().all(|r| written(&f, r)));
    }

    #[test]
    fn a_late_frame_abandons_the_session() {
        let mut f = fake(&V11);
        f.late_once = true;
        let s = block_on(flash(&mut f, &PLAN)).unwrap();
        assert_eq!((s.attempts, s.last_error), (2, Some(RomError::Late)));
    }

    #[test]
    fn missing_read_memory_is_reported() {
        let mut f = fake(&[0x00, 0x31, 0x44]);
        let r = block_on(flash(&mut f, &PLAN));
        assert_eq!(r, Err(RomError::MissingCommand(cmd::READ_MEMORY)));
    }
}
