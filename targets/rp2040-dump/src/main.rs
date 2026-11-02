//! Hub flash dumper (bring-up). Loaded into SRAM from a RAM UF2 by the
//! BOOTSEL bootrom, it never writes flash. Over USB CDC, after the host sends
//! any byte, it prints two flash ranges twice:
//!
//! - `X`: through the XIP window (set up by the ROM for plain 03h reads),
//! - `S`: by direct SSI 03h read commands, bypassing XIP and its cache,
//!
//! as lines `<X|S> <address> <32 bytes hex>`, each range followed by
//! `H <X|S> <start> <end> <sha256>`, and `END`. The backlight blinks while
//! it runs.
#![no_std]
#![no_main]

use core::fmt::Write as _;

use dagon_format::Sha256;
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{self, Driver};
use embassy_rp::{bind_interrupts, rom_data};
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use heapless::String;
use panic_halt as _;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});

// The bootrom calls the lowest address of a RAM UF2 as code: point VTOR,
// MSP and PC at the cortex-m-rt vector table right after this.
core::arch::global_asm!(
    ".section .entry, \"ax\"",
    ".global dump_entry",
    ".thumb_func",
    "dump_entry:",
    "ldr r0, =0x20000100",
    "ldr r1, =0xE000ED08",
    "str r0, [r1]",
    "ldr r1, [r0]",
    "msr msp, r1",
    "ldr r1, [r0, #4]",
    "bx r1",
    ".ltorg",
);

const RANGES: [(u32, u32); 2] = [(0x1000_0000, 0x1001_0000), (0x1004_8000, 0x1004_9000)];
const XIP_BASE: u32 = 0x1000_0000;
const LINE: usize = 32;
const PACKET: u16 = 64;

const SSI: u32 = 0x1800_0000;
const SSI_CTRLR0: *mut u32 = SSI as *mut u32;
const SSI_CTRLR1: *mut u32 = (SSI + 0x04) as *mut u32;
const SSI_SSIENR: *mut u32 = (SSI + 0x08) as *mut u32;
const SSI_SR: *const u32 = (SSI + 0x28) as *const u32;
const SSI_DR0: *mut u32 = (SSI + 0x60) as *mut u32;
const SR_RFNE: u32 = 1 << 3;
const TMOD_EEPROM_READ: u32 = 0x3 << 8;
const CMD_READ: u8 = 0x03;

type Cdc<'d> = CdcAcmClass<'d, Driver<'d, USB>>;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut led = Output::new(p.PIN_25, Level::High);

    // SAFETY: this program runs from SRAM, so flash may leave XIP mode;
    // the ROM routines set the interface up for plain 03h XIP reads.
    unsafe {
        rom_data::connect_internal_flash();
        rom_data::flash_exit_xip();
        rom_data::flash_flush_cache();
        rom_data::flash_enter_cmd_xip();
    }

    let mut cfg = embassy_usb::Config::new(0xc0de, 0xcafe);
    cfg.manufacturer = Some("DAGON");
    cfg.product = Some("DAGON hub flash dump");
    cfg.serial_number = Some("DUMP");
    let mut config_desc = [0; 256];
    let mut bos_desc = [0; 256];
    let mut control_buf = [0; 64];
    let mut state = State::new();
    let mut builder = embassy_usb::Builder::new(
        Driver::new(p.USB, Irqs),
        cfg,
        &mut config_desc,
        &mut bos_desc,
        &mut [],
        &mut control_buf,
    );
    let mut class = CdcAcmClass::new(&mut builder, &mut state, PACKET);
    let mut usb = builder.build();

    let blink = async {
        loop {
            Timer::after_millis(250).await;
            led.toggle();
        }
    };
    let serve = async {
        loop {
            class.wait_connection().await;
            let _ = session(&mut class).await;
        }
    };
    join3(usb.run(), blink, serve).await;
}

async fn session(class: &mut Cdc<'_>) -> Result<(), EndpointError> {
    send(class, b"DAGON hub flash dump: press Enter to start\r\n").await?;
    let mut buf = [0u8; PACKET as usize];
    class.read_packet(&mut buf).await?;
    for (start, end) in RANGES {
        dump(class, 'X', start, end).await?;
        dump(class, 'S', start, end).await?;
    }
    send(class, b"END\r\n").await
}

async fn dump(
    class: &mut Cdc<'_>,
    method: char,
    start: u32,
    end: u32,
) -> Result<(), EndpointError> {
    let mut sha = Sha256::new();
    let mut chunk = [0u8; LINE];
    let mut addr = start;
    while addr < end {
        match method {
            'X' => read_xip(addr, &mut chunk),
            _ => read_ssi(addr, &mut chunk),
        }
        sha.update(&chunk);
        let mut line: String<96> = String::new();
        let _ = write!(line, "{method} {addr:08X} ");
        for b in chunk {
            let _ = write!(line, "{b:02X}");
        }
        let _ = line.push_str("\r\n");
        send(class, line.as_bytes()).await?;
        addr += LINE as u32;
    }
    let mut line: String<128> = String::new();
    let _ = write!(line, "H {method} {start:08X} {end:08X} ");
    for b in sha.finalize() {
        let _ = write!(line, "{b:02x}");
    }
    let _ = line.push_str("\r\n");
    send(class, line.as_bytes()).await
}

fn read_xip(addr: u32, out: &mut [u8]) {
    debug_assert!(addr >= XIP_BASE);
    for (i, b) in out.iter_mut().enumerate() {
        // SAFETY: XIP is mapped (set up in main) and read-only here.
        *b = unsafe { ((addr + i as u32) as *const u8).read_volatile() };
    }
}

/// One 03h read command straight through the SSI (XIP off meanwhile), then
/// XIP back on. Runs with interrupts off; everything here is in SRAM.
fn read_ssi(addr: u32, out: &mut [u8]) {
    let off = addr - XIP_BASE;
    cortex_m::interrupt::free(|_| {
        // SAFETY: SSI register sequence for a standard read, as the ROM's
        // flash routines leave the interface after exiting XIP; nothing
        // executes from flash while XIP is off.
        unsafe {
            rom_data::connect_internal_flash();
            rom_data::flash_exit_xip();
            SSI_SSIENR.write_volatile(0);
            SSI_CTRLR0.write_volatile(SSI_CTRLR0.read_volatile() | TMOD_EEPROM_READ);
            SSI_CTRLR1.write_volatile(out.len() as u32 - 1);
            SSI_SSIENR.write_volatile(1);
            for b in [CMD_READ, (off >> 16) as u8, (off >> 8) as u8, off as u8] {
                SSI_DR0.write_volatile(b as u32);
            }
            for b in out.iter_mut() {
                while SSI_SR.read_volatile() & SR_RFNE == 0 {}
                *b = SSI_DR0.read_volatile() as u8;
            }
            SSI_SSIENR.write_volatile(0);
            SSI_CTRLR1.write_volatile(0);
            rom_data::flash_flush_cache();
            rom_data::flash_enter_cmd_xip();
        }
    });
}

async fn send(class: &mut Cdc<'_>, data: &[u8]) -> Result<(), EndpointError> {
    for chunk in data.chunks(PACKET as usize) {
        class.write_packet(chunk).await?;
    }
    if data.len().is_multiple_of(PACKET as usize) {
        class.write_packet(&[]).await?;
    }
    Ok(())
}
