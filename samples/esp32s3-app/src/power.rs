//! Panel power on the M5StopWatch, run by `main` before `board::start` hands
//! the internal I2C bus (I2C0, SDA G47, SCL G48) to the DAGON link task.
//!
//! The order and values follow M5Stack's M5StopWatch-UserDemo V0.5 (its
//! `hal_pmic.cpp` / `hal_ioe.cpp`); this exact sequence lights the AMOLED on
//! the watch both from power-on and after a chip reset. Which of its steps
//! the panel strictly needs is not known; only the official demo's charging
//! and power-button settings are left out.
//!
//! After an update the chip only resets: the M5PM1 and M5IOE1 keep their
//! state and nothing powers the panel down, so the app redoes the whole
//! sequence on every start.
//!
//! Every start logs the register reads and write results on the USB
//! console and keeps a snapshot in the M5PM1's RTC RAM ([`crate::bootlog`]).

use esp_hal::{
    delay::Delay,
    i2c::master::{Config as I2cConfig, I2c},
    peripherals::{GPIO47, GPIO48, I2C0, USB_DEVICE},
    time::Rate,
    Blocking,
};

use crate::bootlog::{self, at, Console, Hex, PM1_REGS, SNAPSHOT_LEN};

type Bus<'a> = I2c<'a, Blocking>;

/// M5PM1 power manager.
const PM1: u8 = 0x6E;
/// M5IOE1 IO expander.
const IOE: u8 = 0x4F;

/// M5PM1 settings taken from the official demo: (register, bits to set,
/// bits to clear). Its charging and power-button settings are left out on
/// purpose: the sample does not change how the watch charges or powers off
/// (nor PMG3, the charger's CHG_PROG pin, which the demo drives low).
const PM1_SETTINGS: [(u8, u8, u8); 5] = [
    (0x09, 0x00, 0x0F), // I2C_CFG: no I2C sleep
    (0x07, 1 << 5, 0),  // HOLD_CFG: hold the 3.3 V LDO
    // PMG2 (the charger's CHG_STAT) a plain input: GPIO function, no pull.
    (0x16, 0x00, 0b11 << 4), // FUNC0: PMG2 GPIO
    (0x10, 0x00, 1 << 2),    // MODE: PMG2 input
    (0x14, 0x00, 0b11 << 4), // PUPD0: PMG2 no pull
];
/// M5PM1 WDT_CNT: 0 stops its watchdog.
const PM1_WDT_CNT: u8 = 0x0A;

// M5IOE1 registers (16-bit pairs: low byte IO1-IO8, high byte IO9-IO14).
const IOE_MODE: u8 = 0x03;
const IOE_OUT: u8 = 0x05;
const IOE_IN: u8 = 0x07;
const IOE_PU: u8 = 0x09;
const IOE_PD: u8 = 0x0B;
const IOE_DRV: u8 = 0x13;
const IOE_I2C_CFG: u8 = 0x23;
const IOE_REV: u8 = 0x02;
/// I2C_CFG: no sleep, 400 kHz.
const IOE_I2C_400K: u8 = 0x10;

const fn io(n: u8) -> u16 {
    1 << (n - 1)
}
/// IO1 rear-pin MUX, IO3 audio enable, IO4 touch reset, IO5 AMOLED reset,
/// IO8 L3B enable (the AMOLED's LDO), IO9 motor, IO10 speaker PA.
const IOE_OUTPUTS: u16 = io(1) | io(3) | io(4) | io(5) | io(8) | io(9) | io(10);
const IOE_HIGH: u16 = io(3) | io(4) | io(5) | io(8);
const IOE_LOW: u16 = io(1) | io(10);
const L3B_EN: u16 = io(8);

/// Attempts per register access: at 400 kHz the M5IOE1 now and then answers
/// garbage, so every write is read back.
const TRIES: usize = 5;
/// The official demo re-checks L3B_EN at this period.
const L3B_CHECK_MS: u32 = 80;
const L3B_CHECKS: usize = 10;
/// Settling time after the expander switches to 400 kHz.
const SPEED_SWITCH_MS: u32 = 50;
const WAKE_MS: u32 = 10;
/// Right after power-on the M5IOE1 (its own MCU) does not answer yet; the
/// official `M5IOE1::begin` then waits this long, wakes it and tries once
/// more. The whole power sequence stays well inside the 4 s start watchdog.
const IOE_RETRY_MS: u32 = 800;
/// Wait before the panel is first addressed (official `Hal::init`).
const PANEL_SETTLE_MS: u32 = 50;

fn r8(bus: &mut Bus, addr: u8, reg: u8) -> Option<u8> {
    let mut b = [0u8];
    bus.write_read(addr, &[reg], &mut b).ok().map(|()| b[0])
}

fn r16(bus: &mut Bus, addr: u8, reg: u8) -> Option<u16> {
    let mut b = [0u8; 2];
    bus.write_read(addr, &[reg], &mut b)
        .ok()
        .map(|()| u16::from_le_bytes(b))
}

/// Read-modify-write of one 8-bit register, verified by reading back.
fn update8(bus: &mut Bus, addr: u8, reg: u8, set: u8, clear: u8) -> bool {
    (0..TRIES).any(|_| {
        r8(bus, addr, reg).is_some_and(|v| {
            let want = (v & !clear) | set;
            bus.write(addr, &[reg, want]).is_ok() && r8(bus, addr, reg) == Some(want)
        })
    })
}

/// Read-modify-write of one 16-bit register pair, verified by reading back.
fn update16(bus: &mut Bus, reg: u8, set: u16, clear: u16) -> bool {
    (0..TRIES).any(|_| {
        r16(bus, IOE, reg).is_some_and(|v| {
            let want = (v & !clear) | set;
            let [l, h] = want.to_le_bytes();
            bus.write(IOE, &[reg, l, h]).is_ok() && r16(bus, IOE, reg) == Some(want)
        })
    })
}

/// A dummy access that wakes the expander's I2C.
fn wake_ioe(bus: &mut Bus, delay: &Delay) {
    let _ = bus.write(IOE, &[IOE_REV]);
    delay.delay_millis(WAKE_MS);
}

/// Every setting is tried; true if all of them stuck.
fn pm1(bus: &mut Bus, con: &mut Console) -> bool {
    let mut all = true;
    for &(reg, set, clear) in &PM1_SETTINGS {
        let ok = update8(bus, PM1, reg, set, clear);
        con.line(format_args!(
            "PM1 {reg:02X} set {set:02X} clear {clear:02X}: ok={ok}"
        ));
        all &= ok;
    }
    let ok = update8(bus, PM1, PM1_WDT_CNT, 0, 0xFF);
    con.line(format_args!("PM1 {PM1_WDT_CNT:02X} = 00: ok={ok}"));
    all && ok
}

/// Whether L3B_EN reads high, and at which check (0: never).
fn ioe(bus: &mut Bus, delay: &Delay, con: &mut Console) -> (bool, u8) {
    wake_ioe(bus, delay);
    let mut answered = update8(bus, IOE, IOE_I2C_CFG, IOE_I2C_400K, 0xFF);
    if !answered {
        con.line(format_args!(
            "IOE 23 = 10: no answer, again in {IOE_RETRY_MS} ms"
        ));
        delay.delay_millis(IOE_RETRY_MS);
        wake_ioe(bus, delay);
        answered = update8(bus, IOE, IOE_I2C_CFG, IOE_I2C_400K, 0xFF);
    }
    if !answered {
        con.line(format_args!("IOE 23 = 10: failed"));
        return (false, 0);
    }
    if bus
        .apply_config(&I2cConfig::default().with_frequency(Rate::from_khz(400)))
        .is_err()
    {
        con.line(format_args!("I2C 400 kHz: failed"));
        return (false, 0);
    }
    delay.delay_millis(SPEED_SWITCH_MS);
    wake_ioe(bus, delay);
    // pinMode(OUTPUT) of the library: no pulls, push-pull, output.
    let outputs = update16(bus, IOE_PU, 0, IOE_OUTPUTS)
        && update16(bus, IOE_PD, 0, IOE_OUTPUTS)
        && update16(bus, IOE_DRV, 0, IOE_OUTPUTS)
        && update16(bus, IOE_MODE, IOE_OUTPUTS, 0);
    con.line(format_args!("IOE outputs (09, 0B, 13, 03): ok={outputs}"));
    if !outputs {
        return (false, 0);
    }
    // The AMOLED reset (IO5) is only released, never pulsed.
    let levels = update16(bus, IOE_OUT, IOE_HIGH, IOE_LOW);
    con.line(format_args!("IOE 05 levels: ok={levels}"));
    // The pin itself must read high, not just the output latch.
    let check = (1..=L3B_CHECKS as u8).find(|_| {
        delay.delay_millis(L3B_CHECK_MS);
        let high = r16(bus, IOE, IOE_IN).is_some_and(|v| v & L3B_EN != 0);
        if !high {
            let _ = update16(bus, IOE_OUT, L3B_EN, 0);
        }
        high
    });
    con.line(format_args!("L3B_EN high at check {check:?}"));
    (check.is_some(), check.unwrap_or(0))
}

/// `regs` of `addr`, each read once.
fn read_all<const N: usize>(bus: &mut Bus, addr: u8, regs: &[u8; N]) -> [Option<u8>; N] {
    regs.map(|reg| r8(bus, addr, reg))
}

/// The previous start's snapshot (M5PM1 RTC RAM).
fn previous(bus: &mut Bus) -> Option<[u8; SNAPSHOT_LEN]> {
    let mut b = [0u8; SNAPSHOT_LEN];
    (0..TRIES)
        .any(|_| bus.write_read(PM1, &[bootlog::RTC_RAM], &mut b).is_ok())
        .then_some(b)
}

/// Writes this start's snapshot and reads it back.
fn keep(bus: &mut Bus, snap: &[u8; SNAPSHOT_LEN]) -> bool {
    let mut frame = [0u8; SNAPSHOT_LEN + 1];
    frame[0] = bootlog::RTC_RAM;
    frame[1..].copy_from_slice(snap);
    (0..TRIES).any(|_| bus.write(PM1, &frame).is_ok() && previous(bus) == Some(*snap))
}

/// Powers the AMOLED (M5PM1 settings, M5IOE1 outputs with L3B_EN high),
/// logs it on the USB console and keeps a snapshot, then releases the bus
/// and the console. `false`: the panel has no power, leave it alone.
pub fn panel_power_on(
    i2c0: I2C0<'_>,
    sda: GPIO47<'_>,
    scl: GPIO48<'_>,
    usb: USB_DEVICE<'_>,
) -> bool {
    let delay = Delay::new();
    let mut con = Console::new(usb);
    let reset = esp_hal::system::reset_reason();
    con.line(format_args!("=== watch start: ESP reset {reset:?}"));
    let Ok(bus) = I2c::new(
        i2c0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    ) else {
        con.line(format_args!("I2C0: cannot open"));
        return false;
    };
    let mut bus = bus.with_sda(sda).with_scl(scl);
    let prev = previous(&mut bus);
    con.line(format_args!("previous start snapshot: {prev:02X?}"));
    let before = read_all(&mut bus, PM1, &PM1_REGS);
    con.line(format_args!("PM1 before {PM1_REGS:02X?}: {}", Hex(&before)));
    // A PM1 setting that does not stick is not fatal for the panel.
    let pm1_ok = pm1(&mut bus, &mut con);
    const PM1_AFTER: [u8; 3] = [0x06, 0x07, 0x09];
    let pm1_after = read_all(&mut bus, PM1, &PM1_AFTER);
    con.line(format_args!(
        "PM1 after {PM1_AFTER:02X?}: {}",
        Hex(&pm1_after)
    ));
    let (powered, check) = ioe(&mut bus, &delay, &mut con);
    const IOE_AFTER: [u8; 5] = [IOE_MODE, IOE_OUT, IOE_IN, IOE_DRV, IOE_I2C_CFG];
    let ioe_after = read_all(&mut bus, IOE, &IOE_AFTER);
    con.line(format_args!(
        "IOE after {IOE_AFTER:02X?}: {}",
        Hex(&ioe_after)
    ));

    let starts = prev
        .filter(|p| p[at::MAGIC] == bootlog::MAGIC)
        .map_or(0, |p| p[at::STARTS]);
    let mut snap = [0u8; SNAPSHOT_LEN];
    snap[at::MAGIC] = bootlog::MAGIC;
    snap[at::STARTS] = starts.wrapping_add(1);
    snap[at::RESET] = reset.map_or(0xFF, |r| r as u8);
    snap[at::RESULT] = u8::from(pm1_ok) | u8::from(powered) << 1 | check << 4;
    let fields = [
        (at::PM1_BEFORE, &before[..]),
        (at::PM1_AFTER, &pm1_after[..]),
        (at::IOE_AFTER, &ioe_after[..]),
    ];
    for (start, regs) in fields {
        for (i, b) in regs.iter().enumerate() {
            // An unreadable register is kept as FF.
            snap[start + i] = b.unwrap_or(0xFF);
        }
    }
    // The M5PM1 stays at 100 kHz (its I2C_CFG speed bit is left at 0).
    let kept = bus
        .apply_config(&I2cConfig::default().with_frequency(Rate::from_khz(100)))
        .is_ok()
        && keep(&mut bus, &snap);
    con.line(format_args!("snapshot {snap:02X?}: kept={kept}"));
    con.line(format_args!("=== panel powered={powered}"));
    delay.delay_millis(PANEL_SETTLE_MS);
    powered
}
