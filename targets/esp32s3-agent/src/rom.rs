//! Cooperative entry into the ESP32-S3 ROM download mode (ENTER_ROM).
//!
//! The watch has no external reset or boot control: the strapping pins are
//! only latched at a chip reset, which nothing outside can cause. The node
//! itself therefore switches to the ROM loader on request, after replying:
//!
//! | method | How | When it does not take |
//! |---|---|---|
//! | 1 | `RTC_CNTL_OPTION1_REG.FORCE_DOWNLOAD_BOOT`, then a software reset | the bit does not read back: method 2 |
//! | 2 | M5PM1 power manager `SYS_CMD` Download over the internal I2C | normal software reset (the agent answers again) |
//!
//! Method 1 is the first choice; the PC sends method 2 when the agent still
//! answers HELLO after method 1. This module is the request side and is
//! tested on the host; the register and I2C work is in `node`.

use dagon_proto::{flags, Frame, Status, HEADER_LEN};

/// ENTER_ROM command code (not one of the agent's commands).
pub const CMD_ENTER_ROM: u8 = 0x24;

/// M5PM1 on the internal SYS I2C (SDA G47 / SCL G48).
// ponytail: unverified. The datasheet gives SYS_CMD 0x0C = KEY 0xA + CMD
// 0b11 (Download); KEY in the high nibble and the 7-bit address 0x6E are
// assumptions. Confirm both on the watch before relying on method 2.
pub const M5PM1_ADDR: u8 = 0x6E;
pub const M5PM1_SYS_CMD: u8 = 0x0C;
pub const M5PM1_DOWNLOAD: u8 = 0xA3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    ForceDownloadBoot = 1,
    M5pm1 = 2,
}

/// Reply frame size of [`enter_rom_request`].
pub const REPLY_LEN: usize = dagon_proto::frame_len(1);

/// Answers an ENTER_ROM request: OK with the method to run after the reply
/// has been sent, or E_ARG (nothing to run). `None`: not an ENTER_ROM
/// request.
pub fn enter_rom_request(
    request: &[u8],
    max_payload: usize,
    out: &mut [u8; REPLY_LEN],
) -> Option<(usize, Option<Method>)> {
    let (f, _) = Frame::decode(request, max_payload).ok()?;
    if f.cmd != CMD_ENTER_ROM || f.flags & (flags::RESP | flags::EVENT) != 0 {
        return None;
    }
    let method = match f.payload {
        [1] => Some(Method::ForceDownloadBoot),
        [2] => Some(Method::M5pm1),
        _ => None,
    };
    out[HEADER_LEN] = if method.is_some() {
        Status::Ok
    } else {
        Status::Arg
    } as u8;
    let n = dagon_proto::finish(out, flags::RESP, f.seq, f.cmd, 1).ok()?;
    Some((n, method))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::{split_status, Cmd};

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
        assert_eq!((f.seq, f.cmd, f.flags), (9, CMD_ENTER_ROM, flags::RESP));
        (split_status(f.payload).unwrap().0, m)
    }

    #[test]
    fn methods_and_bad_arguments() {
        assert_eq!(
            answer(&req(CMD_ENTER_ROM, &[1])),
            (Status::Ok, Some(Method::ForceDownloadBoot))
        );
        assert_eq!(
            answer(&req(CMD_ENTER_ROM, &[2])),
            (Status::Ok, Some(Method::M5pm1))
        );
        for p in [&[][..], &[0], &[3], &[1, 0]] {
            assert_eq!(answer(&req(CMD_ENTER_ROM, p)), (Status::Arg, None));
        }
    }

    #[test]
    fn other_frames_pass_through() {
        let mut out = [0u8; REPLY_LEN];
        let hello = req(Cmd::Hello as u8, &[0; 6]);
        assert_eq!(enter_rom_request(&hello, 1024, &mut out), None);
        let mut resp = req(CMD_ENTER_ROM, &[1]);
        resp[3] = flags::RESP;
        let n = resp.len();
        dagon_proto::finish(&mut resp, flags::RESP, 9, CMD_ENTER_ROM, n - 12).unwrap();
        assert_eq!(enter_rom_request(&resp, 1024, &mut out), None);
    }
}
