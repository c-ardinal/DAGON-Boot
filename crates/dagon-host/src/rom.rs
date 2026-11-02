//! ROM path for the STM32F401: the hub's SUBCTL / I2C_RAW as a
//! [`RomLink`], and the first-install flash contents written through it.

use alloc::vec;
use dagon_conform::timeout::REQUEST_MS;
use dagon_format::header::{self, image_kind, mcu_id, HEADER_SIZE};
use dagon_format::statelog::{self, kind, reason, slot, FaceHeader, Record};
use dagon_format::{sha256, FormatError, ImageHeader};
use dagon_port::{BusError, I2cBus, Nrst, SubControl};
use dagon_proto::payload::{phase, I2cRaw};
use dagon_proto::{Cmd, Status};
use dagon_romlink::{Region, RomLink, FRAME_TIMEOUT_MS, ROM_ADDR};

use crate::ops::Event;
use crate::{Clock, Error, Host, Target, Transport};

/// F401 flash map: boot core payload, its trailer header, APP state log A,
/// APP-A (inline header, payload after it).
pub const F401_BOOTCORE: u32 = 0x0800_0000;
pub const F401_BOOTCORE_HEADER: u32 = 0x0800_7E00;
pub const F401_LOG_A: u32 = 0x0800_8000;
pub const F401_APP_A: u32 = 0x0802_0000;
/// Boot core (0-1), state logs A/B (2-3), APP-A (5).
pub const F401_ERASE: [u16; 5] = [0, 1, 2, 3, 5];
/// Face header, PROVISION, CONFIRMED.
pub const INITIAL_LOG_LEN: usize = statelog::FACE_HEADER_SIZE + 2 * statelog::RECORD_SIZE;

/// State log face for a fresh install: slot A of the APP provisioned and
/// confirmed with `app`'s header.
pub fn initial_log(app_header: &[u8; HEADER_SIZE]) -> Result<[u8; INITIAL_LOG_LEN], FormatError> {
    let h = ImageHeader::parse(app_header)?;
    let rec = |k, seq| Record {
        kind: k,
        target: statelog::target::APP,
        slot: slot::A,
        reason: reason::NONE,
        seq,
        version: h.version,
        digest16: header::header_digest16(app_header),
    };
    let mut out = [0u8; INITIAL_LOG_LEN];
    let (f, r) = out.split_at_mut(statelog::FACE_HEADER_SIZE);
    f.copy_from_slice(&FaceHeader { generation: 1 }.to_bytes());
    r[..statelog::RECORD_SIZE].copy_from_slice(&rec(kind::PROVISION, 1).to_bytes());
    r[statelog::RECORD_SIZE..].copy_from_slice(&rec(kind::CONFIRMED, 2).to_bytes());
    Ok(out)
}

/// Checks a signed F401 boot core image: signature under `pubkey`, kind,
/// MCU, payload digest and size (it must end before its trailer header).
pub fn check_bootcore(img: &[u8], pubkey: &[u8; 32]) -> Result<ImageHeader, FormatError> {
    let raw: &[u8; HEADER_SIZE] = img
        .get(..HEADER_SIZE)
        .and_then(|h| h.try_into().ok())
        .ok_or(FormatError::Truncated)?;
    let h = header::verify_header(raw, pubkey)?;
    let body = &img[HEADER_SIZE..];
    let fits = (F401_BOOTCORE + body.len() as u32) <= F401_BOOTCORE_HEADER;
    if h.image_kind != image_kind::BOOTCORE || h.mcu_id != mcu_id::STM32F401XE || !fits {
        return Err(FormatError::BadField);
    }
    if h.payload_size as usize != body.len() || sha256(body) != h.payload_sha256 {
        return Err(FormatError::DigestMismatch);
    }
    Ok(h)
}

/// APP-A is sector 5 (128 KiB): header and payload must fit it.
pub const F401_APP_A_LEN: usize = 0x2_0000;

/// An APP-A image (header + payload) that fits its sector.
pub fn check_app_a(img: &[u8]) -> Result<(), FormatError> {
    if img.len() < HEADER_SIZE || img.len() > F401_APP_A_LEN {
        return Err(FormatError::BadField);
    }
    Ok(())
}

/// Bodies first, then headers, then the state log.
pub fn f401_regions<'a>(boot: &'a [u8], app: &'a [u8], log: &'a [u8]) -> [Region<'a>; 5] {
    let (bh, bb) = boot.split_at(HEADER_SIZE);
    let (ah, ab) = app.split_at(HEADER_SIZE);
    let r = |addr, data| Region { addr, data };
    [
        r(F401_BOOTCORE, bb),
        r(F401_APP_A + HEADER_SIZE as u32, ab),
        r(F401_BOOTCORE_HEADER, bh),
        r(F401_APP_A, ah),
        r(F401_LOG_A, log),
    ]
}

/// The hub's NRST/BOOT0 and raw I2C, with progress mirrored as events and
/// SHOW phase ROM.
pub(crate) struct HubRom<'h, T: Transport, C: Clock> {
    pub host: &'h mut Host<T, C>,
    pub on_event: &'h mut dyn FnMut(Event),
    pub node: u8,
    pub version: dagon_format::Version,
    pub timeout: u32,
    pub shown: u8,
}

fn bus_error<E>(e: Error<E>) -> BusError {
    match e {
        // The hub got NACK (or nothing) from the loader.
        Error::Rejected {
            status: Status::Timeout,
            ..
        } => BusError::Nack,
        Error::Timeout | Error::Relay(_) => BusError::Timeout,
        _ => BusError::Io,
    }
}

impl<T: Transport, C: Clock> HubRom<'_, T, C> {
    async fn raw(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError> {
        if addr != ROM_ADDR {
            return Err(BusError::Nack);
        }
        let req = I2cRaw {
            read_len: buf.len() as u16,
            data,
        };
        let mut p = vec![0u8; 4 + data.len()];
        req.encode(&mut p).map_err(|_| BusError::Io)?;
        let body = self
            .host
            .call(Target::Hub, Cmd::I2cRaw, &p, self.timeout)
            .await
            .map_err(bus_error)?;
        if body.len() != buf.len() {
            return Err(BusError::Io);
        }
        buf.copy_from_slice(&body);
        Ok(())
    }
}

impl<T: Transport, C: Clock> I2cBus for HubRom<'_, T, C> {
    async fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BusError> {
        self.raw(addr, data, &mut []).await
    }
    async fn read(&mut self, addr: u8, buf: &mut [u8]) -> Result<(), BusError> {
        self.raw(addr, &[], buf).await
    }
    async fn write_read(&mut self, addr: u8, data: &[u8], buf: &mut [u8]) -> Result<(), BusError> {
        self.raw(addr, data, buf).await
    }
}

impl<T: Transport, C: Clock> SubControl for HubRom<'_, T, C> {
    async fn set_lines(&mut self, nrst: Nrst, boot0_high: bool) -> Result<(), BusError> {
        self.host
            .call(
                Target::Hub,
                Cmd::SubCtl,
                &[nrst as u8, boot0_high as u8],
                REQUEST_MS,
            )
            .await
            .map(|_| ())
            .map_err(bus_error)
    }
}

impl<T: Transport, C: Clock> Clock for HubRom<'_, T, C> {
    fn now_ms(&self) -> u64 {
        self.host.clock.now_ms()
    }
    async fn sleep_ms(&mut self, ms: u32) {
        self.host.clock.sleep_ms(ms).await
    }
}

impl<T: Transport, C: Clock> RomLink for HubRom<'_, T, C> {
    fn set_timeout_ms(&mut self, ms: u32) {
        self.timeout = ms;
    }

    async fn progress(&mut self, percent: u8) {
        const STEP: u8 = 10;
        if percent < self.shown + STEP {
            return;
        }
        self.shown = percent - percent % STEP;
        let (node, v, p) = (self.node, self.version, self.shown);
        self.host
            .emit(&mut *self.on_event, node, phase::ROM, p, v)
            .await;
    }
}

impl<'h, T: Transport, C: Clock> HubRom<'h, T, C> {
    pub fn new(
        host: &'h mut Host<T, C>,
        on_event: &'h mut dyn FnMut(Event),
        node: u8,
        version: dagon_format::Version,
    ) -> Self {
        HubRom {
            host,
            on_event,
            node,
            version,
            timeout: FRAME_TIMEOUT_MS,
            shown: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_a_larger_than_its_sector_is_refused() {
        assert!(super::check_app_a(&alloc::vec![0; super::F401_APP_A_LEN + 1]).is_err());
        assert!(super::check_app_a(&alloc::vec![0; super::F401_APP_A_LEN]).is_ok());
    }
}
