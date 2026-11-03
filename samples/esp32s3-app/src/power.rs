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

use esp_hal::{
    delay::Delay,
    i2c::master::{Config as I2cConfig, I2c},
    peripherals::{GPIO47, GPIO48, I2C0},
    time::Rate,
    Blocking,
};

type Bus<'a> = I2c<'a, Blocking>;

/// M5PM1 power manager.
const PM1: u8 = 0x6E;
/// M5IOE1 IO expander.
const IOE: u8 = 0x4F;

/// M5PM1 settings taken from the official demo: (register, bits to set,
/// bits to clear). Its charging and power-button settings are left out on
/// purpose: the sample does not change how the watch charges or powers off.
const PM1_SETTINGS: [(u8, u8, u8); 2] = [
    (0x09, 0x00, 0x0F), // I2C_CFG: no I2C sleep
    (0x07, 1 << 5, 0),  // HOLD_CFG: hold the 3.3 V LDO
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

fn pm1(bus: &mut Bus) -> bool {
    let settings = PM1_SETTINGS
        .iter()
        .all(|&(reg, set, clear)| update8(bus, PM1, reg, set, clear));
    settings && update8(bus, PM1, PM1_WDT_CNT, 0, 0xFF)
}

fn ioe(bus: &mut Bus, delay: &Delay) -> bool {
    wake_ioe(bus, delay);
    if !update8(bus, IOE, IOE_I2C_CFG, IOE_I2C_400K, 0xFF) {
        return false;
    }
    if bus
        .apply_config(&I2cConfig::default().with_frequency(Rate::from_khz(400)))
        .is_err()
    {
        return false;
    }
    delay.delay_millis(SPEED_SWITCH_MS);
    wake_ioe(bus, delay);
    // pinMode(OUTPUT) of the library: no pulls, push-pull, output.
    let outputs = update16(bus, IOE_PU, 0, IOE_OUTPUTS)
        && update16(bus, IOE_PD, 0, IOE_OUTPUTS)
        && update16(bus, IOE_DRV, 0, IOE_OUTPUTS)
        && update16(bus, IOE_MODE, IOE_OUTPUTS, 0);
    if !outputs {
        return false;
    }
    // The AMOLED reset (IO5) is only released, never pulsed.
    let _ = update16(bus, IOE_OUT, IOE_HIGH, IOE_LOW);
    // The pin itself must read high, not just the output latch.
    (0..L3B_CHECKS).any(|_| {
        delay.delay_millis(L3B_CHECK_MS);
        let high = r16(bus, IOE, IOE_IN).is_some_and(|v| v & L3B_EN != 0);
        if !high {
            let _ = update16(bus, IOE_OUT, L3B_EN, 0);
        }
        high
    })
}

/// Powers the AMOLED (M5PM1 settings, M5IOE1 outputs with L3B_EN high) and
/// releases the bus. `false`: the panel has no power, leave it alone.
pub fn panel_power_on(i2c0: I2C0<'_>, sda: GPIO47<'_>, scl: GPIO48<'_>) -> bool {
    let delay = Delay::new();
    let Ok(bus) = I2c::new(
        i2c0,
        I2cConfig::default().with_frequency(Rate::from_khz(100)),
    ) else {
        return false;
    };
    let mut bus = bus.with_sda(sda).with_scl(scl);
    // A PM1 setting that does not stick is not fatal for the panel.
    let _ = pm1(&mut bus);
    let powered = ioe(&mut bus, &delay);
    delay.delay_millis(PANEL_SETTLE_MS);
    powered
}
