//! The simulator behind `dagon demo`: dagon-sim's hub with its two
//! nodes, reached by the page through serve instead of Web Serial
//! (POST /v1/sim/write, GET /v1/sim/read). The page runs dagon-host as it
//! does with a real hub; only the bytes go another way.
//!
//! Simulated time follows the wall clock times `speed`; dagon-sim's
//! [`TimedHub`] holds each answer back for as long as the real links and
//! flash take, so progress looks as it does on hardware.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub use dagon_sim::Medium;
#[cfg(test)]
use dagon_sim::SimHub;
use dagon_sim::TimedHub;

pub struct SimDevice {
    state: Mutex<State>,
    answered: Condvar,
    /// Simulated ms per wall ms.
    pub speed: f64,
    /// The node set to fail its update (for the page and the log).
    pub fail: Option<String>,
    /// Lines for serve's log, under its banner (what is simulated, the
    /// demo key).
    pub notes: Vec<String>,
}

struct State {
    hub: TimedHub,
    wall0: Instant,
    sim0: u64,
}

impl SimDevice {
    #[cfg(test)]
    pub fn new(hub: SimHub, speed: f64, fail: Option<String>) -> SimDevice {
        let sim0 = hub.now_ms();
        SimDevice {
            state: Mutex::new(State {
                hub: TimedHub::new(hub),
                wall0: Instant::now(),
                sim0,
            }),
            answered: Condvar::new(),
            speed,
            fail,
            notes: Vec::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn medium(&self) -> Medium {
        self.lock().hub.medium()
    }

    /// Switches the simulated uplink (the demo 構成 names another).
    pub fn set_medium(&self, m: Medium) {
        self.lock().hub.set_medium(m);
    }

    /// The simulator catches up with the wall clock.
    fn sync(&self, st: &mut State) {
        let elapsed = st.wall0.elapsed().as_secs_f64() * 1_000.0 * self.speed;
        let target = st.sim0 + elapsed as u64;
        let now = st.hub.now_ms();
        if target > now {
            st.hub.advance_ms(target - now);
        }
    }

    /// Bytes from the page to the hub's USB port.
    pub fn write(&self, bytes: &[u8]) {
        let mut st = self.lock();
        self.sync(&mut st);
        st.hub.write(bytes);
        self.answered.notify_all();
    }

    /// The answers that have arrived, waiting up to `wait` for one; empty
    /// when none came.
    pub fn read(&self, wait: Duration) -> Vec<u8> {
        let until = Instant::now() + wait;
        let mut st = self.lock();
        loop {
            self.sync(&mut st);
            let got = st.hub.read();
            let left = until.saturating_duration_since(Instant::now());
            if !got.is_empty() || left.is_zero() {
                return got;
            }
            let now = st.hub.now_ms();
            let next = st.hub.next_due().map(|due| {
                Duration::from_secs_f64(due.saturating_sub(now) as f64 / self.speed / 1_000.0)
            });
            let pause = next
                .map_or(left, |n| n.min(left))
                .max(Duration::from_millis(1));
            st = self
                .answered
                .wait_timeout(st, pause)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::Cmd;

    #[test]
    fn answers_come_when_their_time_has_passed() {
        let key = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([7; 32]));
        let dev = SimDevice::new(crate::demo::sim_hub(&key, None, 1).unwrap(), 50.0, None);
        let payload = [1, 0, 0, 0, 0x40, 0x04];
        let mut hello = vec![0u8; dagon_proto::frame_len(payload.len())];
        dagon_proto::Frame {
            flags: 0,
            seq: 1,
            cmd: Cmd::Hello as u8,
            payload: &payload,
        }
        .encode(&mut hello)
        .unwrap();
        dev.write(&hello);
        let got = dev.read(Duration::from_millis(500));
        assert!(got.starts_with(&dagon_proto::SOF), "{got:?}");
        assert!(
            dev.read(Duration::from_millis(5)).is_empty(),
            "one answer only"
        );
    }
}
