//! The hardware run's measuring build (feature `measure`):
//! what the definition's stage0 layout waits for, kept in RTC slow memory
//! for the test boot core and app (../stage0-qemu) to write on the ROM's
//! text output (samples/hardware-test/esp_stage0.py reads it):
//!
//! - the CPU cycles (CCOUNT), the system timer's ticks (16 MHz from the
//!   crystal, the reference) and the RTC main timer's ticks (the RTC slow
//!   clock) over the whole decision, so the real CPU clock and the slow
//!   clock's rate follow from the reference;
//! - the cycles of the signature checks and of the body hashes, and the
//!   bytes hashed;
//! - the RTC STORE registers and the RTC slow memory below the record as
//!   the reset left them: stage0 fills that memory with a pattern after
//!   each decision, so at the next start the words an app changed show.
//!
//! The record is written before stage0 starts an image, the one of the
//! start before kept beside it (an app that tells nothing has its start's
//! record told by the boot core at the next start); it changes no
//! decision. Registers as the ESP32-S3 PAC 0.36 (`RTC_CNTL`, `SYSTIMER`).

use core::cell::Cell;

use dagon_format::SigVerifier;
use dagon_port::{Flash, FlashError, FlashRegion};

use crate::rom::{reg_read, reg_write};

/// The record's place: 32 words of RTC slow memory, then 32 for the
/// record of the start before, below the retained cell
/// (`qemu_layout::CELL`, 0x5000_1FF8); load plans keep off both.
pub const RECORD: u32 = 0x5000_1E00;
const BEFORE: u32 = RECORD + 32 * 4;
/// The words of a record.
const WORDS: u32 = 23;
/// The record's first word ("DGM1"), when the other words are whole.
pub const MAGIC: u32 = 0x314D_4744;
/// The memory stage0 fills with the pattern: RTC slow memory below the
/// records.
const FILL: (u32, u32) = (0x5000_0000, RECORD);

/// The path stage0 took (record word 22).
pub const PATH_CORE: u32 = 1;
pub const PATH_APP: u32 = 2;

const RTC_CNTL: u32 = 0x6000_8000;
const TIME_UPDATE: u32 = RTC_CNTL + 0x0C;
const TIME_LOW0: u32 = RTC_CNTL + 0x10;
const STORE0_3: u32 = RTC_CNTL + 0x50;
const STORE4_7: u32 = RTC_CNTL + 0xC0;
const SYSTIMER: u32 = 0x6002_3000;
const UNIT0_OP: u32 = SYSTIMER + 0x04;
const UNIT0_VALUE_LO: u32 = SYSTIMER + 0x44;
const UPDATE: u32 = 1 << 30;
const VALUE_VALID: u32 = 1 << 29;

extern "C" {
    fn ets_get_cpu_frequency() -> u32;
    fn rtc_get_reset_reason(cpu: u32) -> u32;
}

fn ccount() -> u32 {
    let c: u32;
    // SAFETY: reads the cycle counter special register.
    unsafe { core::arch::asm!("rsr.ccount {0}", out(reg) c) };
    c
}

/// The system timer's unit 0, low word (16 MHz); 0 when it does not latch.
fn systimer() -> u32 {
    reg_write(UNIT0_OP, UPDATE);
    for _ in 0..1000 {
        if reg_read(UNIT0_OP) & VALUE_VALID != 0 {
            return reg_read(UNIT0_VALUE_LO);
        }
    }
    0
}

/// The RTC main timer, low word (RTC slow clock ticks).
fn rtc_time() -> u32 {
    // The ESP32-S3 has no valid flag for this latch (ESP-IDF's
    // `rtc_cntl_ll_get_rtc_time` reads at once), and the latch takes up
    // to a few slow clock cycles: wait until the word moves past the
    // sample latched before this request.
    let before = reg_read(TIME_LOW0);
    reg_write(TIME_UPDATE, 1 << 31);
    for _ in 0..10_000 {
        let now = reg_read(TIME_LOW0);
        if now != before {
            return now;
        }
    }
    reg_read(TIME_LOW0)
}

fn word(at: u32) -> u32 {
    // SAFETY: RTC slow memory or a register, read as a word.
    unsafe { core::ptr::read_volatile(at as *const u32) }
}

fn set(at: u32, w: u32) {
    // SAFETY: RTC slow memory, written as a word.
    unsafe { core::ptr::write_volatile(at as *mut u32, w) }
}

fn pattern(at: u32) -> u32 {
    at ^ 0x5A5A_A5A5
}

/// The times the checks take, summed over the decision.
#[derive(Default)]
pub struct Spent {
    verify: Cell<(u32, u32)>,
    hash: Cell<(u32, u32)>,
}

/// What the reset left, and the counters at stage0's start.
pub struct Begun {
    at: (u32, u32, u32),
    stores: [u32; 8],
    /// Whether the record and the pattern of an earlier start are there.
    earlier: bool,
    changed: (u32, u32, u32),
}

pub fn begin() -> Begun {
    let at = (ccount(), systimer(), rtc_time());
    let mut stores = [0; 8];
    for i in 0..4 {
        stores[i] = word(STORE0_3 + 4 * i as u32);
        stores[4 + i] = word(STORE4_7 + 4 * i as u32);
    }
    let earlier = word(RECORD) == MAGIC;
    let (mut n, mut first, mut last) = (0, 0, 0);
    let mut a = FILL.0;
    while a < FILL.1 {
        if word(a) != pattern(a) {
            if n == 0 {
                first = a;
            }
            n += 1;
            last = a;
        }
        a += 4;
    }
    Begun {
        at,
        stores,
        earlier,
        changed: (n, first, last),
    }
}

/// Writes the record of this start, then the pattern for the next.
pub fn end(b: &Begun, s: &Spent, path: u32) {
    let (c, t, r) = (ccount(), systimer(), rtc_time());
    let (vc, vn) = s.verify.get();
    let (hc, hb) = s.hash.get();
    // SAFETY: ROM functions without arguments to check.
    let (mhz, reset) = unsafe { (ets_get_cpu_frequency(), rtc_get_reset_reason(0)) };
    let words = [
        mhz,
        reset,
        c.wrapping_sub(b.at.0),
        t.wrapping_sub(b.at.1),
        r.wrapping_sub(b.at.2),
        vc,
        vn,
        hc,
        hb,
        u32::from(b.earlier),
        b.changed.0,
        b.changed.1,
        b.changed.2,
    ];
    set(BEFORE, 0);
    if word(RECORD) == MAGIC {
        for i in 1..WORDS {
            set(BEFORE + 4 * i, word(RECORD + 4 * i));
        }
        set(BEFORE, MAGIC);
    }
    set(RECORD, 0);
    for (i, w) in words.iter().chain(b.stores.iter()).enumerate() {
        set(RECORD + 4 * (1 + i as u32), *w);
    }
    set(RECORD + 4 * 22, path);
    set(RECORD, MAGIC);
    let mut a = FILL.0;
    while a < FILL.1 {
        set(a, pattern(a));
        a += 4;
    }
}

/// The verifier, its checks timed.
pub struct TimedVerifier<'a, V> {
    pub inner: &'a V,
    pub spent: &'a Spent,
}

impl<V: SigVerifier> SigVerifier for TimedVerifier<'_, V> {
    fn key_id(&self) -> [u8; 8] {
        self.inner.key_id()
    }
    fn verify(&self, domain: &[u8], msg: &[&[u8]], sig: &[u8; 64]) -> bool {
        let at = ccount();
        let ok = self.inner.verify(domain, msg, sig);
        let (c, n) = self.spent.verify.get();
        self.spent
            .verify
            .set((c.wrapping_add(ccount().wrapping_sub(at)), n + 1));
        ok
    }
}

/// The flash, its body hashes timed.
pub struct TimedFlash<'a, F> {
    pub inner: &'a mut F,
    pub spent: &'a Spent,
}

impl<F: Flash> Flash for TimedFlash<'_, F> {
    fn geometry(&self) -> &[FlashRegion] {
        self.inner.geometry()
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.inner.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.inner.erase(addr, len)
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.inner.write(addr, data)
    }
    fn sha256(&mut self, addr: u32, len: u32) -> Result<[u8; 32], FlashError> {
        let at = ccount();
        let r = self.inner.sha256(addr, len);
        let (c, b) = self.spent.hash.get();
        self.spent.hash.set((
            c.wrapping_add(ccount().wrapping_sub(at)),
            b.wrapping_add(len),
        ));
        r
    }
}
