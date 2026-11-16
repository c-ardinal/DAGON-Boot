//! The round LCD task: it draws the home screen
//! (`rp2040_round_lcd_screen::home`) from the node's events and the nodes
//! wiring.rs says its links reach, and sends whole frames to the panel.

use dagon_agent::snapshot::{AppSnapshot, Phase};
use dagon_board_waveshare_rp2040_touch_lcd_1_28::DisplayPins;
use dagon_mcu_rp2040::arm::step;
use dagon_mcu_rp2040::node::AppEvents;
use dagon_mcu_rp2040::watchdog::Lease;
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{Async, Spi};
use embassy_time::Timer;
use heapless::Vec;
use rp2040_round_lcd_screen::frame::Frame;
use rp2040_round_lcd_screen::gc9a01;
use rp2040_round_lcd_screen::home::{self, Home, Peer, Status};
use static_cell::ConstStaticCell;

use crate::wiring;

/// Peers the home screen has rings for.
const PEERS: usize = 3;

static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());

/// The LCD task (lowest priority, thread mode). It alone owns SPI1 and the
/// panel pins. It draws into the RAM frame and sends whole frames by DMA,
/// so it never holds the CPU for long, and redraws when the node's state
/// changes. The backlight goes on after the first full frame. `lease`: its
/// guard task (thread mode, `Background`).
#[embassy_executor::task]
pub async fn task(pins: DisplayPins, events: AppEvents, lease: Lease) {
    // A whole piece of drawing must end within the default work limit.
    let busy = || lease.watch.busy(lease.task);
    busy();
    let frame = FRAME.take();
    let DisplayPins {
        spi,
        cs,
        dc,
        rst,
        mut backlight,
    } = pins;
    let mut panel = Panel::new(spi, cs, dc, rst).await;
    let peers: Vec<Peer<'static>, PEERS> = wiring::REACHED
        .iter()
        .take(PEERS)
        .map(|r| Peer {
            name: r.name,
            link: r.link,
        })
        .collect();
    let mut rx = events.receiver();
    let mut state = events.latest().map(|l| l.state).unwrap_or_default();
    let mut shown = None;
    loop {
        if shown != Some(state) {
            step(15);
            let _ = home::draw(frame, &home_of(&state, &peers));
            panel.flush(frame).await;
            backlight.set_high();
            shown = Some(state);
        }
        lease.idle();
        match rx.as_mut() {
            Some(rx) => state = rx.changed().await.state,
            // No receiver left: the screen stays as drawn.
            None => core::future::pending().await,
        }
        busy();
    }
}

/// The home screen for `s`.
fn home_of<'a>(s: &AppSnapshot, peers: &'a [Peer<'a>]) -> Home<'a> {
    Home {
        name: wiring::NODE.id,
        version: s.version,
        status: status(s.phase),
        percent: s.percent.or_else(|| {
            let (done, total) = s.progress.filter(|&(_, t)| t > 0)?;
            Some((u64::from(done) * 100 / u64::from(total)) as u8)
        }),
        peers,
        test_key: dagon_mcu_rp2040::TEST_KEY,
    }
}

fn status(p: Phase) -> Status {
    match p {
        Phase::Starting => Status::Starting,
        Phase::Idle | Phase::Confirmed => Status::Confirmed,
        Phase::Transferring => Status::Writing,
        Phase::Verifying => Status::Verifying,
        Phase::Written => Status::Written,
        Phase::Pending => Status::Pending,
        Phase::RebootPending => Status::Rebooting,
        Phase::Trial => Status::Trial,
        Phase::RolledBack => Status::RolledBack,
        Phase::Failed => Status::Failed,
        // A phase this sample does not know yet.
        _ => Status::Starting,
    }
}

/// The GC9A01 over SPI1 with DMA: commands as `gc9a01` frames them, and
/// whole frames in one transfer.
struct Panel<'d> {
    spi: Spi<'d, SPI1, Async>,
    cs: Output<'d>,
    dc: Output<'d>,
    /// Held high for as long as the panel is used: dropping the pin would
    /// release it to the pad default (pull-down) and reset the panel.
    _rst: Output<'d>,
}

impl<'d> Panel<'d> {
    /// Hardware reset and the init sequence; the frame memory is left as
    /// it is (send a frame before turning the backlight on).
    async fn new(
        spi: Spi<'d, SPI1, Async>,
        mut cs: Output<'d>,
        dc: Output<'d>,
        mut rst: Output<'d>,
    ) -> Self {
        cs.set_high();
        rst.set_high();
        Timer::after_millis(10).await;
        rst.set_low();
        Timer::after_millis(10).await;
        rst.set_high();
        Timer::after_millis(gc9a01::RESET_RELEASE_MS.into()).await;
        let mut p = Panel {
            spi,
            cs,
            dc,
            _rst: rst,
        };
        for &(cmd, args) in gc9a01::INIT {
            p.command(cmd, args).await;
        }
        p.command(gc9a01::SLPOUT, &[]).await;
        Timer::after_millis(gc9a01::SLEEP_OUT_MS.into()).await;
        p.command(gc9a01::DISPON, &[]).await;
        Timer::after_millis(20).await;
        p
    }

    /// One command with its parameters under a single CS frame.
    async fn command(&mut self, cmd: u8, args: &[u8]) {
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.write(&[cmd]).await;
        self.dc.set_high();
        if !args.is_empty() {
            let _ = self.spi.write(args).await;
        }
        self.cs.set_high();
    }

    /// Sends a whole frame: the full window, RAMWR, then the pixels in one
    /// DMA transfer with CS low throughout.
    async fn flush(&mut self, frame: &Frame) {
        let [hi, lo] = (gc9a01::SIZE - 1).to_be_bytes();
        self.command(gc9a01::CASET, &[0, 0, hi, lo]).await;
        self.command(gc9a01::RASET, &[0, 0, hi, lo]).await;
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.write(&[gc9a01::RAMWR]).await;
        self.dc.set_high();
        let _ = self.spi.write(&frame.0[..]).await;
        self.cs.set_high();
    }
}
