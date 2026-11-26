//! The link kinds this package offers, by direction, for STM32F401RE (a node without downlinks):
//!
//! README.md, "Links", shows the matrix (generated from `LINK_KINDS`,
//! which the MCU definition's `[links]` gives).
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Link, LinkError, Reconnect, Support};

/// The MCU definition's `[links]`: the kinds it lists are `Yes`
/// (`dagon_mcu_common::link::kinds_of_definition`).
pub const LINK_KINDS: &[KindInfo] =
    &dagon_mcu_common::link::kinds_of_definition(crate::LINKS_UP, crate::LINKS_DOWN);

dagon_mcu_common::link_requirements!("stm32f401re", LINK_KINDS);

#[cfg(feature = "node")]
pub use target::I2c;

/// Up `i2c`: the I2C target with the DAGON mailbox.
/// The DAGON link: an I2C target (listening at the node's address).
#[cfg(feature = "node")]
pub type I2cTarget =
    embassy_stm32::i2c::I2c<'static, embassy_stm32::mode::Async, embassy_stm32::i2c::MultiMaster>;

#[cfg(feature = "node")]
mod target {
    use embassy_futures::select::{select, Either};
    use embassy_stm32::i2c::SlaveCommandKind;
    use embassy_stm32::pac;
    use embassy_time::{Duration, Timer};

    use super::I2cTarget;
    use super::LinkError;
    use crate::target_link::{Asked, MailboxLink, TargetBus};

    /// How often a read wakes with nothing (the serving loop then drops an
    /// abandoned transfer).
    const TICK: Duration = Duration::from_millis(500);

    /// The node's I2C target: the hub writes a request frame, polls the
    /// mailbox state and reads the response (`crate::target_link`, which
    /// holds the link and its host tests), over embassy-stm32's target.
    pub type I2c = MailboxLink<Hal>;

    impl I2c {
        /// `target`: listening at the node's address; `regs`: its
        /// registers (`BoardDef::open_link`).
        pub fn new(target: I2cTarget, regs: pac::i2c::I2c) -> Self {
            MailboxLink::over(Hal { target, regs })
        }
    }

    /// embassy-stm32's I2C target and its registers.
    pub struct Hal {
        target: I2cTarget,
        regs: pac::i2c::I2c,
    }

    impl TargetBus for Hal {
        async fn listen(&mut self) -> Result<Asked, LinkError> {
            match select(self.target.listen(), Timer::after(TICK)).await {
                Either::First(Ok(cmd)) => Ok(match cmd.kind {
                    SlaveCommandKind::Write => Asked::Write,
                    SlaveCommandKind::Read => Asked::Read,
                }),
                Either::First(Err(_)) => Err(LinkError::Corrupt),
                Either::Second(()) => Err(LinkError::Timeout),
            }
        }

        async fn take_write(&mut self, buf: &mut [u8]) -> Result<usize, LinkError> {
            let n = self
                .target
                .respond_to_write(buf)
                .await
                .map_err(|_| LinkError::Corrupt)?;
            defmt::debug!("i2c write: {} bytes", n);
            Ok(n)
        }

        async fn give_read(&mut self, data: &[u8]) -> Result<usize, LinkError> {
            let offered = data.len();
            let sent = self.target.respond_to_read(data).await.map_err(|e| {
                defmt::debug!("i2c read: offered {}: {}", offered, e);
                LinkError::Corrupt
            })?;
            defmt::debug!("i2c read: offered {} sent {}", offered, sent);
            Ok(sent)
        }

        /// ACK (true) or NACK (false) the own address and data.
        fn acknowledge(&mut self, on: bool) {
            self.regs.cr1().modify(|w| w.set_ack(on));
        }

        /// The own 7-bit address, as the board's link configured it.
        fn addr(&self) -> u8 {
            ((self.regs.oar1().read().add() >> 1) & 0x7F) as u8
        }
    }
}
