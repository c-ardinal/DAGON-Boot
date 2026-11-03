//! The link kinds this package offers, by direction, for STM32F401RE (a 子機: no downlinks):
//!
//! | Kind | Up | Down |
//! |---|---|---|
//! | `usb-cdc` | NotYet | No |
//! | `uart` | NotYet | No |
//! | `i2c` | Yes | No |
//! | `ble` | NotYet | No |
//! | `wifi-tcp` | NotYet | No |
//! | `ethernet` | NotYet | No |
//! | `can` | NotYet | No |
//!
//! `NotYet`: the 構成 may name it, but a project asking for it does not
//! build yet (`require_uplink` / `require_downlinks` say which hop).

pub use dagon_mcu_common::link::{Dir, Kind, KindInfo, Link, LinkError, Reconnect, Support};

pub const LINK_KINDS: &[KindInfo] = &[
    KindInfo {
        kind: Kind::UsbCdc,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Uart,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::I2c,
        up: Support::Yes,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Ble,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::WifiTcp,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Ethernet,
        up: Support::NotYet,
        down: Support::No,
    },
    KindInfo {
        kind: Kind::Can,
        up: Support::NotYet,
        down: Support::No,
    },
];

dagon_mcu_common::link_requirements!("stm32f401re", LINK_KINDS);

#[cfg(feature = "node")]
pub use target::I2c;

/// Up `i2c`: the I2C target with the DAGON mailbox.
#[cfg(feature = "node")]
mod target {
    use dagon_proto::{frame_len, max_payload};
    use embassy_futures::select::{select, Either};
    use embassy_stm32::i2c::SlaveCommandKind;
    use embassy_stm32::pac;
    use embassy_time::{Duration, Timer};

    use super::{Link, LinkError, Reconnect};
    use crate::mailbox::Mailbox;
    use crate::node::I2cTarget;

    /// How often a read wakes with nothing (the serving loop then drops an
    /// abandoned transfer).
    const TICK: Duration = Duration::from_millis(500);
    const RX: usize = frame_len(max_payload::I2C);

    /// The node's I2C target: the hub writes a request frame, polls the
    /// mailbox state and reads the response. A request is handed out with
    /// the own address NACKed; [`Link::write_frame`] holds the response and
    /// ACKs again. [`Link::flush`] serves the hub's reads until it has the
    /// whole response.
    pub struct I2c {
        target: I2cTarget,
        regs: pac::i2c::I2c,
        mailbox: Mailbox,
        rx: [u8; RX],
        /// A request that arrived while flushing, handed out next.
        pending: Option<usize>,
    }

    impl I2c {
        /// `target`: listening at the node's address; `regs`: its
        /// registers (`BoardDef::open_link`).
        pub fn new(target: I2cTarget, regs: pac::i2c::I2c) -> Self {
            I2c {
                target,
                regs,
                mailbox: Mailbox::default(),
                rx: [0; RX],
                pending: None,
            }
        }

        /// ACK (true) or NACK (false) the own address and data.
        fn acknowledge(&self, on: bool) {
            self.regs.cr1().modify(|w| w.set_ack(on));
        }

        /// One bus event: `Ok(Some(n))` a request of `n` bytes, `Ok(None)`
        /// anything else (a served read reports whether the whole
        /// response left in `done`).
        async fn event(&mut self, done: &mut bool) -> Result<Option<usize>, LinkError> {
            let cmd = match select(self.target.listen(), Timer::after(TICK)).await {
                Either::First(Ok(cmd)) => cmd,
                Either::First(Err(_)) => return Err(LinkError::Corrupt),
                Either::Second(()) => return Err(LinkError::Timeout),
            };
            match cmd.kind {
                SlaveCommandKind::Write => {
                    let n = self
                        .target
                        .respond_to_write(&mut self.rx)
                        .await
                        .map_err(|_| LinkError::Corrupt)?;
                    // Zero bytes: an address probe.
                    if n == 0 {
                        return Ok(None);
                    }
                    self.mailbox.request();
                    self.acknowledge(false);
                    Ok(Some(n))
                }
                SlaveCommandKind::Read => {
                    let sent = self
                        .target
                        .respond_to_read(self.mailbox.read_data())
                        .await
                        .map_err(|_| LinkError::Corrupt)?;
                    *done = self.mailbox.read_done(sent);
                    Ok(None)
                }
            }
        }
    }

    impl Link for I2c {
        fn max_payload(&self) -> usize {
            max_payload::I2C
        }

        fn reconnect(&self) -> Reconnect {
            Reconnect::Persistent
        }

        async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
            if let Some(n) = self.pending.take() {
                return Ok(&self.rx[..n]);
            }
            loop {
                let mut done = false;
                if let Some(n) = self.event(&mut done).await? {
                    return Ok(&self.rx[..n]);
                }
            }
        }

        async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
            self.mailbox.respond(Some(frame), false);
            self.acknowledge(true);
            Ok(())
        }

        /// No answer: reads stay BUSY (the hub times out and resends) and
        /// the own address is ACKed again for the next request.
        async fn no_reply(&mut self) -> Result<(), LinkError> {
            self.mailbox.respond(None, false);
            self.acknowledge(true);
            Ok(())
        }

        /// Serves the hub's reads until it has the whole response. A new
        /// request first: kept for the next read, and this fails (the
        /// caller then does not reset).
        async fn flush(&mut self) -> Result<(), LinkError> {
            self.mailbox.reboot_after_read();
            loop {
                let mut done = false;
                match self.event(&mut done).await {
                    Ok(Some(n)) => {
                        self.pending = Some(n);
                        return Err(LinkError::Closed);
                    }
                    Ok(None) if done => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}
