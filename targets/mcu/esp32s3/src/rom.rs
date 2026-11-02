//! Cooperative entry into the ESP32-S3 ROM download mode (ENTER_ROM).
//!
//! On boards without external reset or boot control (the strapping pins are
//! only latched at a chip reset) the node itself switches to the ROM loader
//! on request, after replying:
//!
//! | method | How | When it does not take |
//! |---|---|---|
//! | 1 | `RTC_CNTL_OPTION1_REG.FORCE_DOWNLOAD_BOOT`, then a software reset | normal software reset (the agent answers again) |
//! | 2 | the board's power manager restarts the chip in download mode (`board::BoardDef::pmic_download`); E_UNSUPPORTED when the board has none | normal software reset |
//!
//! This module is the request side and is tested on the host; the register
//! work is in `node`, method 2 in the board crate.

use dagon_proto::payload::{rom_method, EnterRom};
use dagon_proto::{flags, Cmd, Frame, Status, HEADER_LEN};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    ForceDownloadBoot,
    Pmic,
}

/// One action of the switch to the ROM loader, run by `node` in order once
/// the OK reply has left the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Disable the RTC watchdog and keep the super watchdog auto-fed: both
    /// sit in the RTC domain, survive the software reset, and nothing in
    /// the ROM loader feeds them, so a running one would reset the loader
    /// (and with it FORCE_DOWNLOAD_BOOT) a few seconds into flashing.
    StopWatchdogs,
    ForceDownloadBoot,
    /// The board's power manager Download command (it pauses for it to act).
    PmicDownload,
    /// Software reset: into the ROM loader if the method took, otherwise
    /// back into the agent.
    Reset,
}

/// The actions for `method`, in order.
pub const fn steps(method: Method) -> &'static [Step] {
    match method {
        Method::ForceDownloadBoot => &[Step::StopWatchdogs, Step::ForceDownloadBoot, Step::Reset],
        Method::Pmic => &[Step::StopWatchdogs, Step::PmicDownload, Step::Reset],
    }
}

/// Reply frame size of [`enter_rom_request`].
pub const REPLY_LEN: usize = dagon_proto::frame_len(1);

/// Answers an ENTER_ROM request: OK with the method to run after the reply
/// has been sent, E_UNSUPPORTED for method 2 when `pmic` (the board supports
/// it) is false, or E_ARG (nothing to run either way). `None`: not an
/// ENTER_ROM request.
pub fn enter_rom_request(
    request: &[u8],
    max_payload: usize,
    pmic: bool,
    out: &mut [u8; REPLY_LEN],
) -> Option<(usize, Option<Method>)> {
    let (f, _) = Frame::decode(request, max_payload).ok()?;
    if f.cmd != Cmd::EnterRom as u8 || f.flags & (flags::RESP | flags::EVENT) != 0 {
        return None;
    }
    let (status, method) = match EnterRom::parse(f.payload).map(|r| r.method) {
        Ok(rom_method::FORCE_DOWNLOAD_BOOT) => (Status::Ok, Some(Method::ForceDownloadBoot)),
        Ok(rom_method::PMIC) if pmic => (Status::Ok, Some(Method::Pmic)),
        Ok(_) => (Status::Unsupported, None),
        Err(s) => (s, None),
    };
    out[HEADER_LEN] = status as u8;
    let n = dagon_proto::finish(out, flags::RESP, f.seq, f.cmd, 1).ok()?;
    Some((n, method))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::split_status;

    fn req(cmd: u8, payload: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; 64];
        let n = Frame {
            flags: 0,
            seq: 9,
            cmd,
            payload,
        }
        .encode(&mut buf)
        .unwrap();
        buf.truncate(n);
        buf
    }

    fn answer(r: &[u8], pmic: bool) -> (Status, Option<Method>) {
        let mut out = [0u8; REPLY_LEN];
        let (n, m) = enter_rom_request(r, 1024, pmic, &mut out).unwrap();
        let (f, _) = Frame::decode(&out[..n], 1024).unwrap();
        assert_eq!(
            (f.seq, f.cmd, f.flags),
            (9, Cmd::EnterRom as u8, flags::RESP)
        );
        (split_status(f.payload).unwrap().0, m)
    }

    #[test]
    fn methods_and_bad_arguments() {
        for pmic in [false, true] {
            assert_eq!(
                answer(&req(Cmd::EnterRom as u8, &[1]), pmic),
                (Status::Ok, Some(Method::ForceDownloadBoot))
            );
            for p in [&[][..], &[0], &[3], &[1, 0]] {
                assert_eq!(
                    answer(&req(Cmd::EnterRom as u8, p), pmic),
                    (Status::Arg, None)
                );
            }
        }
        // Method 2 only where the board has a power manager that can do it.
        assert_eq!(
            answer(&req(Cmd::EnterRom as u8, &[2]), true),
            (Status::Ok, Some(Method::Pmic))
        );
        assert_eq!(
            answer(&req(Cmd::EnterRom as u8, &[2]), false),
            (Status::Unsupported, None)
        );
    }

    #[test]
    fn watchdogs_stop_before_the_mode_switch_and_reset_comes_last() {
        for m in [Method::ForceDownloadBoot, Method::Pmic] {
            let s = steps(m);
            assert_eq!(s.first(), Some(&Step::StopWatchdogs), "{m:?}");
            assert_eq!(s.last(), Some(&Step::Reset), "{m:?}");
            assert_eq!(s.iter().filter(|&&x| x == Step::Reset).count(), 1);
        }
        assert_eq!(steps(Method::ForceDownloadBoot)[1], Step::ForceDownloadBoot);
        assert_eq!(steps(Method::Pmic)[1], Step::PmicDownload);
    }

    #[test]
    fn other_frames_pass_through() {
        let mut out = [0u8; REPLY_LEN];
        let hello = req(Cmd::Hello as u8, &[0; 6]);
        assert_eq!(enter_rom_request(&hello, 1024, true, &mut out), None);
        let mut resp = req(Cmd::EnterRom as u8, &[1]);
        resp[3] = flags::RESP;
        let n = resp.len();
        dagon_proto::finish(&mut resp, flags::RESP, 9, Cmd::EnterRom as u8, n - 12).unwrap();
        assert_eq!(enter_rom_request(&resp, 1024, true, &mut out), None);
    }
}
