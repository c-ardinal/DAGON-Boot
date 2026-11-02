//! Cooperative entry into the ESP32-S3 ROM download mode (ENTER_ROM).
//!
//! The watch has no external reset or boot control: the strapping pins are
//! only latched at a chip reset, which nothing outside can cause. The node
//! itself therefore switches to the ROM loader on request, after replying:
//!
//! | method | How | When it does not take |
//! |---|---|---|
//! | 1 | `RTC_CNTL_OPTION1_REG.FORCE_DOWNLOAD_BOOT`, then a software reset | normal software reset (the agent answers again) |
//! | 2 | M5PM1 power manager `SYS_CMD` Download over the internal I2C; only with the `pmic-download` feature, else E_UNSUPPORTED | normal software reset |
//!
//! This module is the request side and is tested on the host; the register
//! and I2C work is in `node`.

use dagon_proto::payload::{rom_method, EnterRom};
use dagon_proto::{flags, Cmd, Frame, Status, HEADER_LEN};

/// M5PM1 on the internal SYS I2C (SDA G47 / SCL G48).
// Unverified: the datasheet gives SYS_CMD 0x0C = KEY 0xA + CMD 0b11
// (Download); KEY in the high nibble and the 7-bit address 0x6E are
// assumptions, hence the feature gate until M5Stack documents or the
// hardware confirm them.
pub const M5PM1_ADDR: u8 = 0x6E;
pub const M5PM1_SYS_CMD: u8 = 0x0C;
pub const M5PM1_DOWNLOAD: u8 = 0xA3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    ForceDownloadBoot,
    M5pm1,
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
    /// M5PM1 Download command, then a pause for it to act.
    PmicDownload,
    /// Software reset: into the ROM loader if the method took, otherwise
    /// back into the agent.
    Reset,
}

/// The actions for `method`, in order.
pub const fn steps(method: Method) -> &'static [Step] {
    match method {
        Method::ForceDownloadBoot => &[Step::StopWatchdogs, Step::ForceDownloadBoot, Step::Reset],
        Method::M5pm1 => &[Step::StopWatchdogs, Step::PmicDownload, Step::Reset],
    }
}

/// Reply frame size of [`enter_rom_request`].
pub const REPLY_LEN: usize = dagon_proto::frame_len(1);

/// Answers an ENTER_ROM request: OK with the method to run after the reply
/// has been sent, E_UNSUPPORTED for method 2 without `pmic-download`, or
/// E_ARG (nothing to run either way). `None`: not an ENTER_ROM request.
pub fn enter_rom_request(
    request: &[u8],
    max_payload: usize,
    out: &mut [u8; REPLY_LEN],
) -> Option<(usize, Option<Method>)> {
    let (f, _) = Frame::decode(request, max_payload).ok()?;
    if f.cmd != Cmd::EnterRom as u8 || f.flags & (flags::RESP | flags::EVENT) != 0 {
        return None;
    }
    let (status, method) = match EnterRom::parse(f.payload).map(|r| r.method) {
        Ok(rom_method::FORCE_DOWNLOAD_BOOT) => (Status::Ok, Some(Method::ForceDownloadBoot)),
        Ok(rom_method::PMIC) if cfg!(feature = "pmic-download") => {
            (Status::Ok, Some(Method::M5pm1))
        }
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

    fn answer(r: &[u8]) -> (Status, Option<Method>) {
        let mut out = [0u8; REPLY_LEN];
        let (n, m) = enter_rom_request(r, 1024, &mut out).unwrap();
        let (f, _) = Frame::decode(&out[..n], 1024).unwrap();
        assert_eq!(
            (f.seq, f.cmd, f.flags),
            (9, Cmd::EnterRom as u8, flags::RESP)
        );
        (split_status(f.payload).unwrap().0, m)
    }

    #[test]
    fn methods_and_bad_arguments() {
        assert_eq!(
            answer(&req(Cmd::EnterRom as u8, &[1])),
            (Status::Ok, Some(Method::ForceDownloadBoot))
        );
        let method2 = if cfg!(feature = "pmic-download") {
            (Status::Ok, Some(Method::M5pm1))
        } else {
            (Status::Unsupported, None)
        };
        assert_eq!(answer(&req(Cmd::EnterRom as u8, &[2])), method2);
        for p in [&[][..], &[0], &[3], &[1, 0]] {
            assert_eq!(answer(&req(Cmd::EnterRom as u8, p)), (Status::Arg, None));
        }
    }

    #[test]
    fn watchdogs_stop_before_the_mode_switch_and_reset_comes_last() {
        for m in [Method::ForceDownloadBoot, Method::M5pm1] {
            let s = steps(m);
            assert_eq!(s.first(), Some(&Step::StopWatchdogs), "{m:?}");
            assert_eq!(s.last(), Some(&Step::Reset), "{m:?}");
            assert_eq!(s.iter().filter(|&&x| x == Step::Reset).count(), 1);
        }
        assert_eq!(steps(Method::ForceDownloadBoot)[1], Step::ForceDownloadBoot);
        assert_eq!(steps(Method::M5pm1)[1], Step::PmicDownload);
    }

    #[test]
    fn other_frames_pass_through() {
        let mut out = [0u8; REPLY_LEN];
        let hello = req(Cmd::Hello as u8, &[0; 6]);
        assert_eq!(enter_rom_request(&hello, 1024, &mut out), None);
        let mut resp = req(Cmd::EnterRom as u8, &[1]);
        resp[3] = flags::RESP;
        let n = resp.len();
        dagon_proto::finish(&mut resp, flags::RESP, 9, Cmd::EnterRom as u8, n - 12).unwrap();
        assert_eq!(enter_rom_request(&resp, 1024, &mut out), None);
    }
}
