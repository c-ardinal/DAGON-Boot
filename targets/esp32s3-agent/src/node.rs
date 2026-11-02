//! Board bring-up and the serving loop: frames from the rear UART0 or USB
//! Serial/JTAG go to the agent (and, in a conformance build, CONF_RUN to the
//! node-side tests); the reply goes back on the link the request came in on.
//! The loop feeds the RTC WDT, bounded by [`TRIAL_FEED_MS`] while on trial.

use core::cell::RefCell;

use dagon_agent::{Agent, Hooks, SlotStore, StoreError, IDLE_TIMEOUT_MS};
use dagon_esp32s3_port::{
    board,
    boot::EspBootSelect,
    flash::EspFlash,
    framer::FrameCutter,
    otadata::ENTRY_LEN,
    sys::{EspClock, EspReset, RtcWatchdog},
    TRIAL_WDT_MS,
};
use dagon_port::{BootSelectV, Reset, Watchdog};
use dagon_proto::payload::SlotState;
use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::{
    rtc_cntl::Rtc,
    time::{Duration, Instant},
    uart::Uart,
    usb::usb_serial_jtag::UsbSerialJtag,
    Blocking,
};

use crate::store::{EspStore, Otadata};
use crate::trial::{TrialWatchdog, TRIAL_FEED_MS};

/// Give up on a USB reply after this long (host stopped reading).
const USB_SEND_TIMEOUT: Duration = Duration::from_millis(200);

type Store<'a, 'd> =
    EspStore<EspFlash<'a, 'd>, EspBootSelect<'a, 'd>, TrialWatchdog<RtcWatchdog, EspClock>>;
type Node<'a, 'd> = Agent<Store<'a, 'd>, AppHooks>;

impl Otadata for EspBootSelect<'_, '_> {
    fn running(&self) -> u8 {
        self.current_slot()
    }
    fn entries(&self) -> Result<[[u8; ENTRY_LEN]; 2], StoreError> {
        EspBootSelect::entries(self).map_err(|_| StoreError::Flash)
    }
    fn set_next(&mut self, slot: u8) -> Result<(), StoreError> {
        BootSelectV::set_next(self, slot).map_err(|_| StoreError::Flash)
    }
    fn confirm(&mut self) -> Result<(), StoreError> {
        EspBootSelect::confirm(self).map_err(|_| StoreError::Flash)
    }
}

pub struct AppHooks;

impl Hooks for AppHooks {
    fn self_check(&mut self) -> bool {
        !cfg!(feature = "no-confirm")
    }
}

fn trying(store: &mut Store<'_, '_>) -> bool {
    let st = store.status();
    st.slots
        .get(st.active as usize)
        .is_some_and(|s| s.state == SlotState::Trying)
}

/// Brings the board up and serves the agent forever. `mode` is the HELLO
/// mode (`payload::mode::APP` in ota_0/ota_1, `AGENT` in factory).
pub fn run(mode: u8) -> ! {
    let p = esp_hal::init(esp_hal::Config::default());
    let mut rwdt = RtcWatchdog::new(Rtc::new(p.RTC_TIMER).rwdt);
    rwdt.start(TRIAL_WDT_MS);

    // Without the expander the rear pins stay at their default (UART).
    let _ = board::route_rear_uart(p.I2C0, p.GPIO47, p.GPIO48);
    let mut uart = board::rear_uart(p.UART0, p.GPIO43, p.GPIO44);
    let mut usb = UsbSerialJtag::new(p.USB_DEVICE);

    let storage = RefCell::new(FlashStorage::new(p.FLASH));
    let Ok(boot) = EspBootSelect::new(&storage) else {
        // Wrong partition table: let the WDT reset (and the 2nd stage roll back).
        loop {
            core::hint::spin_loop();
        }
    };
    let wdt = TrialWatchdog::new(rwdt, EspClock);
    let mut store = EspStore::new(EspFlash::new(&storage), boot, wdt);
    if trying(&mut store) {
        store.wdt.begin_trial(TRIAL_FEED_MS);
    }
    let mut agent = Agent::new(crate::config(mode), store, AppHooks);

    let mut uart_rx = FrameCutter::new();
    let mut usb_rx = FrameCutter::new();
    let idle = Duration::from_millis(IDLE_TIMEOUT_MS.into());
    let mut last = Instant::now();
    loop {
        agent.store().wdt.feed();
        if last.elapsed() >= idle {
            agent.abort();
            last = Instant::now();
        }
        let mut b = [0u8];
        match uart.read_buffered(&mut b) {
            Ok(1) => {
                if let Some(f) = uart_rx.push(b[0]) {
                    last = Instant::now();
                    serve(&mut agent, f, &mut |r| uart_send(&mut uart, r));
                }
            }
            Ok(_) => {}
            // FIFO overflow or line error: the partial frame is lost.
            Err(_) => uart_rx.clear(),
        }
        if let Ok(byte) = usb.read_byte() {
            if let Some(f) = usb_rx.push(byte) {
                last = Instant::now();
                serve(&mut agent, f, &mut |r| usb_send(&mut usb, r));
            }
        }
    }
}

/// Answers one request frame; resets after the reply to REBOOT.
fn serve(agent: &mut Node<'_, '_>, frame: &[u8], send: &mut dyn FnMut(&[u8])) {
    #[cfg(feature = "conform")]
    {
        let mut out = [0u8; dagon_conform::node::RESPONSE_MAX];
        if let Some(n) = conform(agent, frame, &mut out) {
            send(&out[..n]);
            return;
        }
    }
    let Some(reply) = agent.handle(frame) else {
        return;
    };
    let reboot = reply.reboot;
    send(reply.frame);
    if reboot {
        agent.hooks().before_reboot();
        EspReset.reset();
    }
    let store = agent.store();
    if store.wdt.on_trial() && !trying(store) {
        store.wdt.end_trial();
    }
}

/// CONF_RUN in a conformance build. The scratch unit is the first erase unit
/// of the OTA slot that is neither running nor the fallback; without one the
/// request falls through to the agent (E_UNSUPPORTED).
#[cfg(feature = "conform")]
fn conform(agent: &mut Node<'_, '_>, frame: &[u8], out: &mut [u8]) -> Option<usize> {
    use dagon_conform::node::{handle_frame, NodeSetup};
    use dagon_esp32s3_port::layout::{app_partition, REGIONS, SLOT_A, SLOT_B};
    use dagon_proto::{Cmd, Frame};

    let max = dagon_proto::max_payload::UART;
    if Frame::decode(frame, max).ok()?.0.cmd != Cmd::ConfRun as u8 {
        return None;
    }
    let store = agent.store();
    let st = store.status();
    let idle = [SLOT_A, SLOT_B]
        .into_iter()
        .find(|&s| s != st.active && s != st.confirmed)?;
    let setup = NodeSetup {
        declared: &REGIONS,
        scratch: app_partition(idle)?.offset,
    };
    let (flash, wdt) = (&mut store.flash, &mut store.wdt);
    handle_frame(frame, max, flash, &setup, out, &mut || wdt.feed())
}

fn uart_send(uart: &mut Uart<'_, Blocking>, frame: &[u8]) {
    let mut sent = 0;
    while sent < frame.len() {
        match uart.write(&frame[sent..]) {
            Ok(n) => sent += n,
            Err(_) => return,
        }
    }
    // A lost reply is recovered by the sender's retry (same seq).
    let _ = uart.flush();
}

fn usb_send(usb: &mut UsbSerialJtag<'_, Blocking>, frame: &[u8]) {
    let start = Instant::now();
    for &b in frame {
        while usb.write_byte_nb(b).is_err() {
            if start.elapsed() >= USB_SEND_TIMEOUT {
                return;
            }
        }
    }
    while usb.flush_tx_nb().is_err() {
        if start.elapsed() >= USB_SEND_TIMEOUT {
            return;
        }
    }
}
