//! What the link task does once a reply has been sent (or failed to be).
//! Pure, so the ordering rules are tested on the host.

use crate::rom::Method;

/// Follow-up attached to a reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    Nothing,
    /// ENTER_ROM accepted.
    Rom(Method),
    /// REBOOT accepted; reset after this many ms (`allow_reboot_now`).
    Reboot(u32),
    /// An agent reply: a CONFIRM may have ended the trial.
    CheckTrial,
}

/// The step to take after sending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Stay,
    EnterRom(Method),
    /// Reset after this many ms, feeding meanwhile.
    Reset(u32),
    CheckTrial,
    /// The ENTER_ROM OK did not leave the node: stay in the app; the host
    /// retries (the same seq gets the same answer) or gives up.
    RomReplyLost,
}

/// `sent`: the reply left the node (UART transmitter idle, USB written and
/// flushed) within the send timeout.
pub fn next(then: Then, sent: bool) -> Next {
    match then {
        Then::Nothing => Next::Stay,
        Then::Rom(m) if sent => Next::EnterRom(m),
        Then::Rom(_) => Next::RomReplyLost,
        // A lost REBOOT reply is recovered by the host's HELLO polling.
        Then::Reboot(ms) => Next::Reset(ms),
        Then::CheckTrial => Next::CheckTrial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_entry_only_after_the_reply_left() {
        let m = Method::ForceDownloadBoot;
        assert_eq!(next(Then::Rom(m), true), Next::EnterRom(m));
        assert_eq!(next(Then::Rom(m), false), Next::RomReplyLost);
        assert_eq!(next(Then::Reboot(0), false), Next::Reset(0));
        assert_eq!(next(Then::Reboot(300), true), Next::Reset(300));
        assert_eq!(next(Then::CheckTrial, false), Next::CheckTrial);
        assert_eq!(next(Then::Nothing, true), Next::Stay);
    }
}
