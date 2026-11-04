//! I2C target mailbox. The hub writes a whole request frame in one write
//! transaction, polls the 3-byte state until READY, then reads the response
//! frame in one read transaction.
//!
//! A target cannot know how many bytes the next read takes, so the reads
//! alternate: with a response held, the first read gets the READY state and
//! the next one the frame. A read cut short of the whole frame (for example
//! the hub polling the state again) returns to sending the state, so the hub
//! recovers by polling once more.
//!
//! Every read offers one guard byte beyond what the hub takes. embassy-stm32
//! 0.6.0's I2C v1 target, once its DMA has drained the slice it was given,
//! writes 0x00 padding into DR before the hub's NACK/STOP ends the read
//! (`execute_slave_transmit_transfer` then `handle_padding_bytes`); that
//! stale byte can lead the next read (the frame then starts
//! `00 DA 6F`, which the hub rejects). With a guard byte the DMA is never
//! drained by a read of the advertised length. The advertised lengths
//! (READY's length, the 3-byte state) and the `sent` counting stay logical.

use dagon_proto::mailbox::{State, STATE_LEN};

/// Largest response the agent produces over I2C fits in this.
pub const RESP_MAX: usize = dagon_proto::frame_len(dagon_proto::max_payload::I2C);
/// Bytes offered beyond each logical read (see the module docs).
const GUARD: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// No response held: reads get BUSY.
    Empty,
    /// Response held; the next read gets the READY state.
    Ready,
    /// READY was read; the next read gets the frame.
    Frame,
}

pub struct Mailbox {
    phase: Phase,
    resp: [u8; RESP_MAX + GUARD],
    len: usize,
    reboot: bool,
    state: [u8; STATE_LEN + GUARD],
}

/// `bytes` followed by the zero guard.
fn guarded(bytes: [u8; STATE_LEN]) -> [u8; STATE_LEN + GUARD] {
    let mut out = [0; STATE_LEN + GUARD];
    out[..STATE_LEN].copy_from_slice(&bytes);
    out
}

impl Default for Mailbox {
    fn default() -> Self {
        Mailbox {
            phase: Phase::Empty,
            resp: [0; RESP_MAX + GUARD],
            len: 0,
            reboot: false,
            state: guarded(State::Busy.to_bytes()),
        }
    }
}

impl Mailbox {
    /// A new request arrived: any held response is dropped, reads get BUSY
    /// until [`Mailbox::respond`].
    pub fn request(&mut self) {
        *self = Mailbox::default();
    }

    /// Holds the agent's reply (`None`: request dropped, stays BUSY so the
    /// hub times out and resends). `reboot`: reset once the frame is read.
    pub fn respond(&mut self, frame: Option<&[u8]>, reboot: bool) {
        let Some(frame) = frame.filter(|f| f.len() <= RESP_MAX) else {
            return;
        };
        self.resp[..frame.len()].copy_from_slice(frame);
        self.resp[frame.len()] = 0;
        self.len = frame.len();
        self.reboot = reboot;
        self.state = guarded(State::Ready(frame.len() as u16).to_bytes());
        self.phase = Phase::Ready;
    }

    /// Reset once the held response has been read in full.
    pub fn reboot_after_read(&mut self) {
        self.reboot = true;
    }

    /// Bytes to hand the driver for the read transaction starting now: the
    /// logical bytes and the guard byte (the driver pads with 0x00 if the
    /// hub reads more).
    pub fn read_data(&self) -> &[u8] {
        match self.phase {
            Phase::Frame => &self.resp[..self.len + GUARD],
            Phase::Empty | Phase::Ready => &self.state,
        }
    }

    /// The phase, for the per-request debug line.
    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::Empty => "busy",
            Phase::Ready => "ready",
            Phase::Frame => "frame",
        }
    }

    /// The read transaction ended after the hub clocked `sent` bytes.
    /// Returns true when the response frame was delivered and the agent
    /// asked to reboot after it.
    pub fn read_done(&mut self, sent: usize) -> bool {
        match self.phase {
            Phase::Empty => false,
            Phase::Ready => {
                if sent >= STATE_LEN {
                    self.phase = Phase::Frame;
                }
                false
            }
            Phase::Frame if sent >= self.len => {
                let reboot = self.reboot;
                *self = Mailbox::default();
                reboot
            }
            Phase::Frame => {
                self.phase = Phase::Ready;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BUSY state and the guard byte.
    const BUSY: [u8; 4] = [0, 0, 0, 0];

    #[test]
    fn busy_until_reply_then_state_then_frame() {
        let mut m = Mailbox::default();
        assert_eq!(m.read_data(), BUSY);
        assert!(!m.read_done(3));
        m.request();
        assert_eq!(m.read_data(), BUSY);
        m.respond(Some(&[0xDA, 0x6F, 1, 2, 3]), false);
        assert_eq!(m.read_data(), [1, 5, 0, 0]);
        assert!(!m.read_done(3));
        assert_eq!(m.read_data(), [0xDA, 0x6F, 1, 2, 3, 0]);
        assert!(!m.read_done(5));
        assert_eq!(m.read_data(), BUSY);
    }

    #[test]
    fn every_phase_offers_one_guard_byte_beyond_the_logical_read() {
        let mut m = Mailbox::default();
        assert_eq!(
            (m.phase_name(), m.read_data().len()),
            ("busy", STATE_LEN + 1)
        );
        // A longer reply first, so a stale tail would show behind a shorter one.
        m.respond(Some(&[0xEE; 9]), false);
        m.read_done(3);
        m.read_done(9);
        m.respond(Some(&[0xDA, 0x6F, 7]), false);
        assert_eq!(
            (m.phase_name(), m.read_data()),
            ("ready", &[1, 3, 0, 0][..])
        );
        m.read_done(3);
        assert_eq!(
            (m.phase_name(), m.read_data()),
            ("frame", &[0xDA, 0x6F, 7, 0][..])
        );
    }

    #[test]
    fn sent_counts_logical_bytes_with_or_without_the_guard() {
        let mut m = Mailbox::default();
        m.respond(Some(&[9; 10]), false);
        // The hub took the guard byte too (the driver reports 4).
        assert!(!m.read_done(4));
        assert_eq!(m.phase_name(), "frame");
        assert!(!m.read_done(11));
        assert_eq!(m.phase_name(), "busy");
        m.respond(Some(&[9; 10]), false);
        m.read_done(3);
        // One byte short of the frame: back to the state.
        m.read_done(9);
        assert_eq!(m.phase_name(), "ready");
    }

    #[test]
    fn dropped_request_stays_busy() {
        let mut m = Mailbox::default();
        m.request();
        m.respond(None, false);
        assert_eq!(m.read_data(), BUSY);
    }

    #[test]
    fn short_frame_read_goes_back_to_state() {
        let mut m = Mailbox::default();
        m.respond(Some(&[9; 10]), false);
        m.read_done(3);
        // The hub polled the state again instead of reading the frame.
        assert!(!m.read_done(3));
        assert_eq!(m.read_data(), [1, 10, 0, 0]);
        m.read_done(3);
        assert_eq!(m.read_data()[..10], [9; 10]);
    }

    #[test]
    fn cut_state_read_repeats_state() {
        let mut m = Mailbox::default();
        m.respond(Some(&[9; 10]), false);
        m.read_done(1);
        assert_eq!(m.read_data(), [1, 10, 0, 0]);
    }

    #[test]
    fn reboot_only_after_frame_delivered() {
        let mut m = Mailbox::default();
        m.respond(Some(&[7; 13]), true);
        assert!(!m.read_done(3));
        assert!(!m.read_done(12));
        assert!(!m.read_done(3));
        assert!(m.read_done(16));
    }

    #[test]
    fn largest_reply_still_has_its_guard() {
        let mut m = Mailbox::default();
        m.respond(Some(&[5; RESP_MAX]), false);
        m.read_done(3);
        assert_eq!(m.read_data().len(), RESP_MAX + 1);
        assert_eq!(m.read_data()[RESP_MAX], 0);
    }

    #[test]
    fn new_request_drops_held_reply() {
        let mut m = Mailbox::default();
        m.respond(Some(&[7; 13]), true);
        m.request();
        assert_eq!(m.read_data(), BUSY);
        assert!(!m.read_done(13));
    }
}
