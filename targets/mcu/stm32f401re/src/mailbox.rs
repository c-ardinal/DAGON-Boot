//! I2C target mailbox. The hub writes a whole request frame in one write
//! transaction, polls the 3-byte state until READY, then reads the response
//! frame in one read transaction.
//!
//! A target cannot know how many bytes the next read takes, so the reads
//! alternate: with a response held, the first read gets the READY state and
//! the next one the frame. A read cut short of the whole frame (for example
//! the hub polling the state again) returns to sending the state, so the hub
//! recovers by polling once more.

use dagon_proto::mailbox::{State, STATE_LEN};

/// Largest response the agent produces over I2C fits in this.
pub const RESP_MAX: usize = dagon_proto::frame_len(dagon_proto::max_payload::I2C);

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
    resp: [u8; RESP_MAX],
    len: usize,
    reboot: bool,
    state: [u8; STATE_LEN],
}

impl Default for Mailbox {
    fn default() -> Self {
        Mailbox {
            phase: Phase::Empty,
            resp: [0; RESP_MAX],
            len: 0,
            reboot: false,
            state: State::Busy.to_bytes(),
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
        self.len = frame.len();
        self.reboot = reboot;
        self.state = State::Ready(frame.len() as u16).to_bytes();
        self.phase = Phase::Ready;
    }

    /// Bytes to send for the read transaction starting now (the driver pads
    /// with 0x00 if the hub reads more).
    pub fn read_data(&self) -> &[u8] {
        match self.phase {
            Phase::Frame => &self.resp[..self.len],
            Phase::Empty | Phase::Ready => &self.state,
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

    const BUSY: [u8; 3] = [0, 0, 0];

    #[test]
    fn busy_until_reply_then_state_then_frame() {
        let mut m = Mailbox::default();
        assert_eq!(m.read_data(), BUSY);
        assert!(!m.read_done(3));
        m.request();
        assert_eq!(m.read_data(), BUSY);
        m.respond(Some(&[0xDA, 0x6F, 1, 2, 3]), false);
        assert_eq!(m.read_data(), [1, 5, 0]);
        assert!(!m.read_done(3));
        assert_eq!(m.read_data(), [0xDA, 0x6F, 1, 2, 3]);
        assert!(!m.read_done(5));
        assert_eq!(m.read_data(), BUSY);
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
        assert_eq!(m.read_data(), [1, 10, 0]);
        m.read_done(3);
        assert_eq!(m.read_data(), [9; 10]);
    }

    #[test]
    fn cut_state_read_repeats_state() {
        let mut m = Mailbox::default();
        m.respond(Some(&[9; 10]), false);
        m.read_done(1);
        assert_eq!(m.read_data(), [1, 10, 0]);
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
    fn new_request_drops_held_reply() {
        let mut m = Mailbox::default();
        m.respond(Some(&[7; 13]), true);
        m.request();
        assert_eq!(m.read_data(), BUSY);
        assert!(!m.read_done(13));
    }
}
