//! SimNode: one board running dagon-agent over a SimStore, with its boot
//! decision at reset, an I2C target mailbox, the ROM loader (BOOT0 high) and
//! a configurable behaviour for images running on trial.

use crate::flash::SimFlash;
use crate::rom::{SimRom, ROM_ADDR};
use crate::store::{Boot, Layout, SimStore, MAX_ATTEMPTS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dagon_agent::{Agent, Config, SlotStore, StoreError};
use dagon_events::{
    compose, Bounds, Composed, Ctx, Extension, Health, Layer, Nop, Published, TrialInfo, Watchdog,
};
use dagon_format::statelog::slot;
use dagon_port::BusError;
use dagon_proto::mailbox::{State, STATE_LEN};
use dagon_proto::payload::{role, SlotState, StatusResp};

/// How an image running on trial behaves (confirmed images always work).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TrialApp {
    /// Answers and lets CONFIRM through.
    #[default]
    Confirms,
    /// Answers, but its self-check refuses CONFIRM.
    NeverConfirms,
    /// Answers; its self-check passes only this many ms after boot (an app
    /// whose tasks are still starting).
    ConfirmsAfter(u64),
    /// Resets right after start.
    CrashesAtBoot,
    /// Runs but never answers on the link.
    Silent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    /// Application running from this slot (`0xFF`: hub firmware without a
    /// recorded image).
    App(u8),
    /// ROM loader (BOOT0 was high at reset).
    Rom,
    /// NRST held low.
    Held,
    /// Nothing valid to run.
    Dead,
    /// Power was lost; see [`SimNode::power_on`].
    Off,
}

/// The simulated app's answer to `self_check`, set by the node as its
/// trial behaviour asks.
#[derive(Clone, Default)]
pub struct SimCheck(Arc<AtomicBool>);

impl SimCheck {
    fn set(&self, healthy: bool) {
        self.0.store(healthy, Ordering::Relaxed);
    }
}

impl Extension for SimCheck {
    fn id(&self) -> &'static str {
        "sim-app"
    }

    fn self_check(&mut self, _: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        Some(if self.0.load(Ordering::Relaxed) {
            Health::Healthy
        } else {
            Health::Unhealthy
        })
    }
}

/// The app layer's chain of a simulated node.
pub type SimChain = Composed<&'static Published<(), 8>, Nop, SimCheck, fn() -> u64>;

/// The chain of a simulated node, made once with the node: its resets
/// restart the agent over the same chain, as the publisher outlives the
/// simulated app.
fn chain(check: SimCheck) -> SimChain {
    let events: &'static mut Published<(), 8> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    compose(
        Layer::App,
        events,
        Nop,
        check,
        Bounds::new(now, Watchdog::new(TRIAL_WDT_MS, TRIAL_WDT_MS)),
    )
}

/// Trial watchdog period of the simulated layer (the default of
/// [`SimNode::trial_wdt_ms`]).
const TRIAL_WDT_MS: u32 = 10_000;

pub struct SimNode {
    agent: Agent<SimStore, SimChain>,
    check: SimCheck,
    cfg: Config,
    pub i2c_addr: Option<u8>,
    pub trial_app: TrialApp,
    /// A trial image not confirmed this long after boot is reset (trial
    /// watchdog).
    pub trial_wdt_ms: u64,
    /// 3-byte state reads answered BUSY before a response becomes READY.
    pub busy_reads: u32,
    pub rom: SimRom,
    run: Run,
    on_trial: bool,
    nrst_low: bool,
    boot0: bool,
    booted_ms: u64,
    boots: u32,
    mailbox: Option<Vec<u8>>,
    busy_left: u32,
    reboot_after_read: bool,
}

impl SimNode {
    /// Powers up and boots whatever the flash holds.
    pub fn new(cfg: Config, flash: SimFlash, layout: Layout) -> Self {
        let pubkey = cfg.pubkey;
        let check = SimCheck::default();
        let mut n = SimNode {
            agent: Agent::new(
                cfg.clone(),
                SimStore::new(flash, layout, pubkey),
                chain(check.clone()),
            ),
            check,
            cfg,
            i2c_addr: None,
            trial_app: TrialApp::default(),
            trial_wdt_ms: TRIAL_WDT_MS.into(),
            busy_reads: 1,
            rom: SimRom::default(),
            run: Run::Dead,
            on_trial: false,
            nrst_low: false,
            boot0: false,
            booted_ms: 0,
            boots: 0,
            mailbox: None,
            busy_left: 0,
            reboot_after_read: false,
        };
        n.reset(0);
        n
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn run(&self) -> Run {
        self.run
    }

    /// Resets seen so far (each one is a boot decision).
    pub fn boots(&self) -> u32 {
        self.boots
    }

    pub fn store(&mut self) -> &mut SimStore {
        self.agent.store()
    }

    pub fn flash(&mut self) -> &mut SimFlash {
        &mut self.agent.store().flash
    }

    pub fn status(&mut self) -> StatusResp {
        self.agent.store().status()
    }

    /// Writes a factory image into slot A as provisioned and confirmed, then
    /// resets.
    pub fn provision(&mut self, image: &[u8], now_ms: u64) -> Result<(), StoreError> {
        self.agent.store().provision(slot::A, image)?;
        self.reset(now_ms);
        Ok(())
    }

    /// Reset: BOOT0 high starts the ROM loader, otherwise the boot decision
    /// runs. RAM state (agent session, mailbox) is lost.
    pub fn reset(&mut self, now_ms: u64) {
        self.boots += 1;
        self.reboot_after_read = false;
        self.busy_left = 0;
        self.check.set(true);
        self.booted_ms = now_ms;
        self.on_trial = false;
        self.run = if !self.flash().is_powered() {
            Run::Off
        } else if self.nrst_low {
            Run::Held
        } else if self.boot0 {
            self.rom.restart(now_ms);
            Run::Rom
        } else {
            self.boot_app()
        };
        // The app starts over the store as the boot core left it.
        self.agent.restart();
    }

    fn boot_app(&mut self) -> Run {
        for _ in 0..=MAX_ATTEMPTS + 1 {
            let boot = self.agent.store().boot();
            if !self.flash().is_powered() {
                return Run::Off;
            }
            let Boot::Run(s) = boot else {
                return self.idle_firmware();
            };
            self.on_trial = self.status().slots[s as usize].state == SlotState::Trying;
            if self.on_trial && self.trial_app == TrialApp::CrashesAtBoot {
                self.boots += 1;
                continue;
            }
            self.check.set(
                !(self.on_trial
                    && matches!(
                        self.trial_app,
                        TrialApp::NeverConfirms | TrialApp::ConfirmsAfter(_)
                    )),
            );
            return Run::App(s);
        }
        self.idle_firmware()
    }

    /// The hub's firmware answers on USB even with no recorded image.
    fn idle_firmware(&self) -> Run {
        if self.cfg.role == role::HUB {
            Run::App(slot::NONE)
        } else {
            Run::Dead
        }
    }

    pub fn set_nrst_low(&mut self, low: bool, now_ms: u64) {
        let was = self.nrst_low;
        self.nrst_low = low;
        if low {
            self.run = Run::Held;
        } else if was {
            self.reset(now_ms);
        }
    }

    pub fn set_boot0(&mut self, high: bool) {
        self.boot0 = high;
    }

    pub fn boot0(&self) -> bool {
        self.boot0
    }

    /// Restores power after a cut and boots.
    pub fn power_on(&mut self, now_ms: u64) {
        self.flash().power_on();
        self.reset(now_ms);
    }

    /// Advances time: notices a power cut and fires the trial watchdog.
    pub fn tick(&mut self, now_ms: u64) {
        if !matches!(self.run, Run::Off) && !self.flash().is_powered() {
            self.run = Run::Off;
            return;
        }
        if !matches!(self.run, Run::App(_)) || !self.on_trial {
            return;
        }
        if now_ms.saturating_sub(self.booted_ms) < self.trial_wdt_ms {
            return;
        }
        let st = self.status();
        if st
            .slots
            .get(st.active as usize)
            .is_some_and(|s| s.state == SlotState::Trying)
        {
            self.reset(now_ms);
        } else {
            self.on_trial = false;
        }
    }

    /// Whether the application answers on its link right now.
    pub fn responsive(&self) -> bool {
        matches!(self.run, Run::App(_)) && !(self.on_trial && self.trial_app == TrialApp::Silent)
    }

    /// One request, one response, no reboot yet. `None`: no answer.
    fn serve(&mut self, request: &[u8], now_ms: u64) -> Option<(Vec<u8>, bool)> {
        self.tick(now_ms);
        if !self.responsive() {
            return None;
        }
        if let (true, TrialApp::ConfirmsAfter(ms)) = (self.on_trial, self.trial_app) {
            self.check.set(now_ms.saturating_sub(self.booted_ms) >= ms);
        }
        let reply = self
            .agent
            .handle(request)
            .map(|r| (r.frame.to_vec(), r.reboot));
        if !self.flash().is_powered() {
            self.run = Run::Off;
            return None;
        }
        reply
    }

    /// Stream links (UART, USB): returns the response frame; a REBOOT reply
    /// is sent before the reset.
    pub fn handle_frame(&mut self, request: &[u8], now_ms: u64) -> Option<Vec<u8>> {
        let (frame, reboot) = self.serve(request, now_ms)?;
        if reboot {
            self.reset(now_ms);
        }
        Some(frame)
    }

    /// Whether `addr` ACKs on the I2C bus.
    pub fn i2c_present(&self, addr: u8) -> bool {
        match self.run {
            Run::Rom => addr == ROM_ADDR,
            _ => self.responsive() && self.i2c_addr == Some(addr),
        }
    }

    /// I2C write transaction to `addr`. An empty write is an address probe.
    pub fn i2c_write(&mut self, addr: u8, data: &[u8], now_ms: &mut u64) -> Result<(), BusError> {
        self.tick(*now_ms);
        if !self.i2c_present(addr) {
            return Err(BusError::Nack);
        }
        if data.is_empty() {
            return Ok(());
        }
        if self.run == Run::Rom {
            let flash = &mut self.agent.store().flash;
            self.rom.write(flash, data, now_ms);
            return Ok(());
        }
        let served = self.serve(data, *now_ms);
        self.reboot_after_read = served.as_ref().is_some_and(|s| s.1);
        self.mailbox = served.map(|s| s.0);
        self.busy_left = self.busy_reads;
        Ok(())
    }

    /// I2C read transaction: a 3-byte read is the mailbox state, any other
    /// length takes the response frame.
    pub fn i2c_read(&mut self, addr: u8, buf: &mut [u8], now_ms: u64) -> Result<(), BusError> {
        self.tick(now_ms);
        if !self.i2c_present(addr) {
            return Err(BusError::Nack);
        }
        if self.run == Run::Rom {
            return self.rom.read(buf, now_ms);
        }
        if buf.len() == STATE_LEN {
            let state = match &self.mailbox {
                Some(f) if self.busy_left == 0 => State::Ready(f.len() as u16),
                _ => {
                    self.busy_left = self.busy_left.saturating_sub(1);
                    State::Busy
                }
            };
            buf.copy_from_slice(&state.to_bytes());
            return Ok(());
        }
        buf.fill(0);
        if let Some(f) = self.mailbox.take() {
            let n = f.len().min(buf.len());
            buf[..n].copy_from_slice(&f[..n]);
            if self.reboot_after_read {
                self.reset(now_ms);
            }
        }
        Ok(())
    }
}
