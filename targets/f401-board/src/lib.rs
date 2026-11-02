//! NUCLEO-F401RE board support shared by the boot core and the apps: the
//! flash layout, the verifying key and the internal flash behind the
//! `dagon_port::Flash` port.
#![no_std]
#![forbid(unsafe_code)]

mod layout;

pub use layout::*;

use dagon_core::key;

/// Verifying key, embedded at build time from `DAGON_PUBKEY`: the path to
/// the public key file from `dagon key gen`, or its 64 hex digits (see
/// build.rs). Unset, it is the public RFC 8032 test key and the build prints
/// a warning.
pub const PUBKEY: [u8; 32] = match option_env!("DAGON_PUBKEY_HEX") {
    Some(hex) => key::pubkey_from_hex(hex),
    None => key::TEST_PUBKEY,
};

/// True when built with the public test key: anyone can sign images this
/// firmware accepts. HELLO and displays should show "TEST KEY".
pub const TEST_KEY: bool = key::is_test_key(&PUBKEY);

/// Core clock set by [`clocks`].
pub const SYSCLK_HZ: u32 = 84_000_000;

/// 84 MHz from the HSI through the PLL (16 MHz / 16 x 336 / 4; 48 MHz on Q),
/// APB1 42 MHz. HSI keeps it independent of whether the ST-LINK MCO feeds
/// HSE on this board. Safe to apply again after a jump: the HAL moves the
/// core to HSI before it reprograms the PLL.
pub fn clocks() -> embassy_stm32::Config {
    use embassy_stm32::rcc::{
        AHBPrescaler, APBPrescaler, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource, Sysclk,
    };
    let mut c = embassy_stm32::Config::default();
    c.rcc.hsi = true;
    c.rcc.pll_src = PllSource::HSI;
    c.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV16,
        mul: PllMul::MUL336,
        divp: Some(PllPDiv::DIV4),
        divq: Some(PllQDiv::DIV7),
        divr: None,
    });
    c.rcc.sys = Sysclk::PLL1_P;
    c.rcc.ahb_pre = AHBPrescaler::DIV1;
    c.rcc.apb1_pre = APBPrescaler::DIV2;
    c.rcc.apb2_pre = APBPrescaler::DIV1;
    c
}

use dagon_port::{Flash, FlashError, FlashRegion};
use embassy_stm32::flash::{Blocking, Flash as Hal, FLASH_BASE, FLASH_SIZE};
use embassy_stm32::peripherals::FLASH;
use embassy_stm32::Peri;

pub struct BoardFlash<'d>(Hal<'d, Blocking>);

impl<'d> BoardFlash<'d> {
    pub fn new(p: Peri<'d, FLASH>) -> Self {
        BoardFlash(Hal::new_blocking(p))
    }
}

fn region(addr: u32, len: u32) -> Result<&'static FlashRegion, FlashError> {
    FLASH_REGIONS
        .iter()
        .find(|r| addr >= r.start && addr.saturating_add(len) <= r.start + r.len)
        .ok_or(FlashError::Range)
}

fn offset(addr: u32, len: usize) -> Result<u32, FlashError> {
    let off = addr
        .checked_sub(FLASH_BASE as u32)
        .ok_or(FlashError::Range)?;
    (off as usize + len <= FLASH_SIZE)
        .then_some(off)
        .ok_or(FlashError::Range)
}

impl Flash for BoardFlash<'_> {
    fn geometry(&self) -> &[FlashRegion] {
        &FLASH_REGIONS
    }

    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let off = offset(addr, buf.len())?;
        self.0.blocking_read(off, buf).map_err(|_| FlashError::Hw)
    }

    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let r = region(addr, len)?;
        if !(addr - r.start).is_multiple_of(r.erase_size) || !len.is_multiple_of(r.erase_size) {
            return Err(FlashError::Align);
        }
        let off = offset(addr, len as usize)?;
        self.0
            .blocking_erase(off, off + len)
            .map_err(|_| FlashError::Hw)
    }

    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        let r = region(addr, data.len() as u32)?;
        if !addr.is_multiple_of(r.write_size) || !(data.len() as u32).is_multiple_of(r.write_size) {
            return Err(FlashError::Align);
        }
        let off = offset(addr, data.len())?;
        self.0.blocking_write(off, data).map_err(|_| FlashError::Hw)
    }
}
