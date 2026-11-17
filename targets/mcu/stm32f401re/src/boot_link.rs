//! The boot core's I2C target link: the board's DAGON bus as a blocking
//! I2C target with the app's mailbox ([`crate::mailbox`]), polled by the
//! boot core's listen window and update mode (`dagon_agent::update_mode`).
//! The boot core enables no interrupts, so [`Port::poll`] serves one bus
//! event only when the own address has matched (SR1.ADDR); the own
//! address is NACKed while a request is handled, so the hub retries
//! instead of meeting a stretched clock during a flash operation.

use dagon_agent::update_mode::Port;
use dagon_proto::{frame_len, max_payload};
use embassy_stm32::i2c::{self, I2c, Instance, SclPin, SdaPin, SlaveAddrConfig, SlaveCommandKind};
use embassy_stm32::mode::Blocking;
use embassy_stm32::{pac, Peri};

use crate::mailbox::Mailbox;

const RX: usize = frame_len(max_payload::I2C);

pub struct I2cPort {
    target: I2c<'static, Blocking, i2c::MultiMaster>,
    regs: pac::i2c::I2c,
    mailbox: Mailbox,
    rx: [u8; RX],
    delivered: bool,
}

impl I2cPort {
    /// The I2C target `peri` on `scl` and `sda` at the 7-bit `addr`;
    /// `regs` are `peri`'s registers.
    pub fn new<T: Instance>(
        peri: Peri<'static, T>,
        scl: Peri<'static, impl SclPin<T>>,
        sda: Peri<'static, impl SdaPin<T>>,
        addr: u8,
        regs: pac::i2c::I2c,
    ) -> Self {
        let mut cfg = i2c::Config::default();
        cfg.sda_pullup = true;
        cfg.scl_pullup = true;
        let target = I2c::new_blocking(peri, scl, sda, cfg)
            .into_slave_multimaster(SlaveAddrConfig::basic(addr));
        I2cPort {
            target,
            regs,
            mailbox: Mailbox::default(),
            rx: [0; RX],
            delivered: false,
        }
    }

    fn acknowledge(&self, on: bool) {
        self.regs.cr1().modify(|w| w.set_ack(on));
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
}
