//! The boot core's I2C target link: the board's DAGON bus as a blocking
//! I2C target with the app's mailbox ([`crate::mailbox`]), served by the
//! boot core's listen window and update mode (`dagon_agent::update_mode`).
//! [`Port::poll`] serves one bus event only when the own address has
//! matched (SR1.ADDR); the own address is NACKed while a request is
//! handled, so the hub retries instead of meeting a stretched clock during
//! a flash operation.
//!
//! Between events the core sleeps in [`Port::wait`] (WFE): the I2C event
//! interrupt is raised in the peripheral but left disabled in the NVIC, so
//! with SEVONPEND its pending bit wakes the core without a handler; the
//! SysTick, one-shot for the time asked and with an empty handler in the
//! boot core, wakes it otherwise. DBGMCU DBG_SLEEP keeps HCLK running in
//! sleep, so the boot core's cycle counter keeps the time. Dropping the
//! port undoes all of it before the boot core jumps to an app.

use cortex_m::peripheral::{syst::SystClkSource, NVIC, SCB};
use dagon_agent::update_mode::Port;
use dagon_proto::{frame_len, max_payload};
use embassy_stm32::i2c::{self, I2c, Instance, SclPin, SdaPin, SlaveAddrConfig, SlaveCommandKind};
use embassy_stm32::interrupt::typelevel::Interrupt as _;
use embassy_stm32::interrupt::Interrupt;
use embassy_stm32::mode::Blocking;
use embassy_stm32::{pac, Peri};

use crate::mailbox::Mailbox;

const RX: usize = frame_len(max_payload::I2C);
/// The SysTick's 24-bit reload.
const SYST_MAX: u32 = 0x00FF_FFFF;

pub struct I2cPort {
    target: I2c<'static, Blocking, i2c::MultiMaster>,
    regs: pac::i2c::I2c,
    event: Interrupt,
    cycles_per_us: u32,
    mailbox: Mailbox,
    rx: [u8; RX],
    delivered: bool,
}

/// The core peripherals the wait uses. The boot core runs one thread and
/// takes them nowhere else while a port exists.
fn core() -> cortex_m::Peripherals {
    // SAFETY: see above; the boot core's own setup only enables the cycle
    // counter, which the wait does not touch.
    unsafe { cortex_m::Peripherals::steal() }
}

impl I2cPort {
    /// The I2C target `peri` on `scl` and `sda` at the 7-bit `addr`;
    /// `regs` are `peri`'s registers, `cycles_per_us` the core clock.
    pub fn new<T: Instance>(
        peri: Peri<'static, T>,
        scl: Peri<'static, impl SclPin<T>>,
        sda: Peri<'static, impl SdaPin<T>>,
        addr: u8,
        regs: pac::i2c::I2c,
        cycles_per_us: u32,
    ) -> Self {
        let mut cfg = i2c::Config::default();
        cfg.sda_pullup = true;
        cfg.scl_pullup = true;
        let target = I2c::new_blocking(peri, scl, sda, cfg)
            .into_slave_multimaster(SlaveAddrConfig::basic(addr));
        core().SCB.set_sevonpend();
        pac::DBGMCU.cr().modify(|w| w.set_dbg_sleep(true));
        I2cPort {
            target,
            regs,
            event: T::EventInterrupt::IRQ,
            cycles_per_us,
            mailbox: Mailbox::default(),
            rx: [0; RX],
            delivered: false,
        }
    }

    fn acknowledge(&self, on: bool) {
        self.regs.cr1().modify(|w| w.set_ack(on));
    }
}

impl Drop for I2cPort {
    fn drop(&mut self) {
        self.regs.cr2().modify(|w| w.set_itevten(false));
        NVIC::unpend(self.event);
        core().SCB.clear_sevonpend();
        pac::DBGMCU.cr().modify(|w| w.set_dbg_sleep(false));
    }
}

impl Port for I2cPort {
    fn poll(&mut self) -> Option<&[u8]> {
        if !self.regs.sr1().read().addr() {
            return None;
        }
        let cmd = self.target.blocking_listen().ok()?;
        match cmd.kind {
            SlaveCommandKind::Write => {
                let n = self.target.blocking_respond_to_write(&mut self.rx).ok()?;
                // Zero bytes: an address probe.
                if n == 0 {
                    return None;
                }
                self.mailbox.request();
                self.acknowledge(false);
                Some(&self.rx[..n])
            }
            SlaveCommandKind::Read => {
                let sent = self
                    .target
                    .blocking_respond_to_read(self.mailbox.read_data())
                    .ok()?;
                self.delivered |= self.mailbox.read_done(sent);
                None
            }
        }
    }

    fn reply(&mut self, frame: Option<&[u8]>, reboot: bool) {
        self.mailbox.respond(frame, reboot);
        self.delivered = false;
        self.acknowledge(true);
    }

    fn delivered(&mut self) -> bool {
        self.delivered
    }

    fn wait(&mut self, max_us: u32) {
        // The blocking driver turns the event interrupt off; on again, and
        // a pending bit left from the last event cleared, so only a new one
        // wakes the core (one that comes in between still does: WFE sees
        // the event it latched).
        self.regs.cr2().modify(|w| w.set_itevten(true));
        NVIC::unpend(self.event);
        if self.regs.sr1().read().addr() {
            return;
        }
        let mut syst = core().SYST;
        let ticks = max_us.saturating_mul(self.cycles_per_us).clamp(1, SYST_MAX);
        syst.set_clock_source(SystClkSource::Core);
        syst.set_reload(ticks);
        syst.clear_current();
        syst.enable_interrupt();
        syst.enable_counter();
        cortex_m::asm::wfe();
        syst.disable_counter();
        syst.disable_interrupt();
        SCB::clear_pendst();
    }
}
