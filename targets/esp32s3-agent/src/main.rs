//! M5StopWatch smoke image: routes the rear MUX_IO_1/2 pins to UART0, echoes
//! every byte received on the rear UART, and pulses the vibration motor as a
//! heartbeat (the board has no user LED).
#![no_std]
#![no_main]

use esp_hal::{
    i2c::master::{Config as I2cConfig, I2c},
    time::{Duration, Instant},
    uart::{Config as UartConfig, Uart},
    Blocking,
};

esp_bootloader_esp_idf::esp_app_desc!();

/// M5IOE1 I/O expander on the internal SYS I2C bus (SDA G47, SCL G48).
const IOE_ADDR: u8 = 0x4F;
const IOE_REG_MODE_L: u8 = 0x03;
const IOE_REG_MODE_H: u8 = 0x04;
const IOE_REG_OUT_L: u8 = 0x05;
const IOE_REG_OUT_H: u8 = 0x06;
const IOE_REG_DRV_L: u8 = 0x13;
const IOE_REG_DRV_H: u8 = 0x14;
/// PYG1 (PYB_MUX_CTR): low = rear pins carry UART0 (G43 TX / G44 RX), high = USB.
const PYG1_BIT_L: u8 = 1 << 0;
/// PYG9 (PYB_MT_PWM): vibration motor, driven as a plain push-pull output.
const PYG9_BIT_H: u8 = 1 << 0;

const HEARTBEAT_PERIOD: Duration = Duration::from_millis(2000);
const HEARTBEAT_PULSE: Duration = Duration::from_millis(60);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // Reset instead of hanging so a failing image never stalls the boot flow.
    esp_hal::system::software_reset()
}

type Bus = I2c<'static, Blocking>;

fn ioe_update(
    i2c: &mut Bus,
    reg: u8,
    set: u8,
    clear: u8,
) -> Result<(), esp_hal::i2c::master::Error> {
    let mut cur = [0u8];
    i2c.write_read(IOE_ADDR, &[reg], &mut cur)?;
    i2c.write(IOE_ADDR, &[reg, (cur[0] & !clear) | set])
}

/// Drive a pin low first, then make it a push-pull output, so it never glitches high.
fn ioe_output_low(
    i2c: &mut Bus,
    out: u8,
    drv: u8,
    mode: u8,
    bit: u8,
) -> Result<(), esp_hal::i2c::master::Error> {
    ioe_update(i2c, out, 0, bit)?;
    ioe_update(i2c, drv, 0, bit)?;
    ioe_update(i2c, mode, bit, 0)
}

fn ioe_init(i2c: &mut Bus) -> Result<(), esp_hal::i2c::master::Error> {
    ioe_output_low(
        i2c,
        IOE_REG_OUT_L,
        IOE_REG_DRV_L,
        IOE_REG_MODE_L,
        PYG1_BIT_L,
    )?;
    ioe_output_low(
        i2c,
        IOE_REG_OUT_H,
        IOE_REG_DRV_H,
        IOE_REG_MODE_H,
        PYG9_BIT_H,
    )
}

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default());

    let mut i2c = I2c::new(p.I2C0, I2cConfig::default())
        .expect("i2c config")
        .with_sda(p.GPIO47)
        .with_scl(p.GPIO48);
    let ioe_ok = ioe_init(&mut i2c).is_ok();

    let mut uart = Uart::new(p.UART0, UartConfig::default())
        .expect("uart config")
        .with_tx(p.GPIO43)
        .with_rx(p.GPIO44);
    let banner: &[u8] = if ioe_ok {
        b"\r\ndagon smoke: rear UART0 ready, echoing\r\n"
    } else {
        b"\r\ndagon smoke: M5IOE1 not reachable, PYG1 left at default\r\n"
    };
    let _ = uart.write(banner);

    let mut motor_on = false;
    let mut beat = Instant::now();
    let mut buf = [0u8; 64];
    loop {
        if let Ok(n) = uart.read_buffered(&mut buf) {
            let mut sent = 0;
            while sent < n {
                sent += uart.write(&buf[sent..n]).unwrap_or(0);
            }
        }
        // Non-blocking so the 128-byte RX FIFO keeps being drained during a pulse.
        if !ioe_ok {
            continue;
        }
        let elapsed = beat.elapsed();
        if !motor_on && elapsed >= HEARTBEAT_PERIOD {
            beat = Instant::now();
            motor_on = ioe_update(&mut i2c, IOE_REG_OUT_H, PYG9_BIT_H, 0).is_ok();
        } else if motor_on && elapsed >= HEARTBEAT_PULSE {
            motor_on = ioe_update(&mut i2c, IOE_REG_OUT_H, 0, PYG9_BIT_H).is_err();
        }
    }
}
