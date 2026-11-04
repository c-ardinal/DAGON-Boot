//! DAGON over USB CDC: the device (DAGON's VID:PID, so the host tools find
//! it), a frame link over its serial class, and [`serve_agent`], the whole
//! serving loop of an app whose only DAGON link is USB.
//!
//! Build the device and link inside the executor that serves them, run the
//! device with [`run`], then serve. Call [`new`] once. This module binds
//! the USB interrupt itself.

use dagon_agent::{Agent, AppChain, SlotStore};
use dagon_proto::{max_payload, MAX_FRAME};
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::UsbDevice;
use static_cell::StaticCell;

use crate::frames::FrameReader;
use crate::watchdog::Watch;
use dagon_mcu_common::link::{Link, LinkError, Reconnect};
use dagon_mcu_common::serve::{serve, Exit, ServeHost};

// This package owns the USB interrupt: apps must not bind USBCTRL_IRQ.
bind_interrupts!(struct UsbIrqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// DAGON's USB vendor and product ID (the host tools look for these).
pub const USB_VID: u16 = 0xc0de;
pub const USB_PID: u16 = 0xcafe;
pub const USB_PACKET: u16 = 64;

const USB_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// Lets the REBOOT reply leave before the reset drops USB.
pub const REBOOT_DELAY: Duration = Duration::from_millis(50);

pub type UsbDriver = Driver<'static, USB>;
type Cdc = CdcAcmClass<'static, UsbDriver>;

static USB_BUFS: StaticCell<([u8; 256], [u8; 256], [u8; 64])> = StaticCell::new();
static USB_STATE: StaticCell<State> = StaticCell::new();

/// The USB device (run it with [`run`]) and the DAGON frame link over its
/// CDC serial class. `product` and `serial` show on the host.
pub fn new(
    usb: Peri<'static, USB>,
    product: &'static str,
    serial: &'static str,
) -> (UsbDevice<'static, UsbDriver>, CdcLink) {
    let mut cfg = embassy_usb::Config::new(USB_VID, USB_PID);
    cfg.manufacturer = Some("DAGON");
    cfg.product = Some(product);
    cfg.serial_number = Some(serial);
    let (config_desc, bos_desc, control_buf) = USB_BUFS.init(([0; 256], [0; 256], [0; 64]));
    let mut builder = embassy_usb::Builder::new(
        Driver::new(usb, UsbIrqs),
        cfg,
        config_desc,
        bos_desc,
        &mut [],
        control_buf,
    );
    let class = CdcAcmClass::new(&mut builder, USB_STATE.init(State::new()), USB_PACKET);
    let link = CdcLink {
        class,
        rx: FrameReader::new(max_payload::USB),
        pkt: [0; USB_PACKET as usize],
        at: 0,
        len: 0,
        last: 0,
    };
    (builder.build(), link)
}

/// Runs the USB device.
#[embassy_executor::task]
pub async fn run(mut usb: UsbDevice<'static, UsbDriver>) -> ! {
    usb.run().await
}

/// DAGON frames over the CDC serial class.
pub struct CdcLink {
    class: Cdc,
    rx: FrameReader<{ MAX_FRAME + USB_PACKET as usize }>,
    pkt: [u8; USB_PACKET as usize],
    /// Unread part of `pkt`: `at..len`.
    at: usize,
    len: usize,
    /// Length of the frame handed out last (dropped on the next read).
    last: usize,
}

impl CdcLink {
    /// The USB device (run it with [`run`]) and the link; as [`new`].
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        usb: Peri<'static, USB>,
        product: &'static str,
        serial: &'static str,
    ) -> (UsbDevice<'static, UsbDriver>, CdcLink) {
        new(usb, product, serial)
    }
}

impl Link for CdcLink {
    fn max_payload(&self) -> usize {
        max_payload::USB
    }

    fn reconnect(&self) -> Reconnect {
        Reconnect::Reenumerates
    }

    /// Waits for the next complete frame from the host (never times out).
    async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
        if self.last > 0 {
            self.rx.consume(self.last);
            self.last = 0;
        }
        loop {
            if let Some(n) = self.rx.poll() {
                self.last = n;
                return Ok(self.rx.frame(n));
            }
            if self.at < self.len {
                self.at += self.rx.push(&self.pkt[self.at..self.len]);
                continue;
            }
            self.class.wait_connection().await;
            match self.class.read_packet(&mut self.pkt).await {
                Ok(n) => (self.at, self.len) = (0, n),
                Err(_) => {
                    self.rx.clear();
                    (self.at, self.len) = (0, 0);
                }
            }
        }
    }

    /// Writes one frame (a zero-length packet ends a frame that fills its
    /// last packet); gives up if the host stops reading.
    async fn write_frame(&mut self, data: &[u8]) -> Result<(), LinkError> {
        if data.is_empty() {
            return Ok(());
        }
        let class = &mut self.class;
        let write = async {
            for chunk in data.chunks(USB_PACKET as usize) {
                class.write_packet(chunk).await?;
            }
            if data.len().is_multiple_of(USB_PACKET as usize) {
                class.write_packet(&[]).await?;
            }
            Ok::<(), EndpointError>(())
        };
        match with_timeout(USB_WRITE_TIMEOUT, write).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(LinkError::Closed),
        }
    }

    /// Lets the last reply leave before a reset drops USB.
    async fn flush(&mut self) -> Result<(), LinkError> {
        Timer::after(REBOOT_DELAY).await;
        Ok(())
    }
}

/// The serving loop's view of the guard: guard task `task` is idle while
/// waiting for the host and busy while answering.
pub struct GuardHost<'a> {
    pub watch: &'a dyn Watch,
    pub task: usize,
}

impl<S: SlotStore, X: AppChain> ServeHost<S, X> for GuardHost<'_> {
    fn now_ms(&self) -> u64 {
        embassy_time::Instant::now().as_millis()
    }
    fn idle(&mut self) {
        self.watch.idle(self.task);
    }
    fn busy(&mut self) {
        self.watch.busy(self.task);
    }
}

/// Serves `agent` over `link` (any [`Link`]) forever; REBOOT resets the
/// chip after the reply has left.
pub async fn serve_agent<L: Link, S: SlotStore, X: AppChain>(
    mut link: L,
    mut agent: Agent<S, X>,
    guard: &dyn Watch,
    task: usize,
) -> ! {
    let mut host = GuardHost { watch: guard, task };
    loop {
        if serve(&mut link, &mut agent, &mut host).await == Exit::Reboot {
            guard.reboot();
        }
    }
}
