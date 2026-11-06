//! Request and response payloads. Response structs describe the body after
//! the status byte (see [`crate::split_status`]). `parse` rejects wrong
//! lengths with [`Status::Arg`]; `encode` returns the byte count written.

use crate::{FrameError, Status};
use dagon_format::Version;

/// HELLO / RELAY etc. field values.
pub mod role {
    pub const HUB: u8 = 0;
    pub const NODE: u8 = 1;
}

pub mod mode {
    pub const APP: u8 = 0;
    pub const AGENT: u8 = 3;
}

/// XFER_BEGIN / SET_PENDING / CONFIRM / STATUS `target`.
pub mod target {
    pub const APP: u8 = 0;
    /// A boot core slot (A/B) of a self-boot MCU with a stage0; the new
    /// boot core confirms itself, so CONFIRM never names it.
    pub const BOOTCORE: u8 = 1;
    /// The recovery agent's slot (`slot = 2`, factory) of a vendor-stage2
    /// MCU; written with its header last, never set pending or confirmed.
    pub const AGENT: u8 = 2;
    /// Hub display copy of the topology; `hdr_len = 0`, unsigned.
    pub const TOPOLOGY: u8 = 3;
}

/// STATUS request: empty for the APP slots, or one `target` byte
/// (`target::BOOTCORE`: the boot core slots, in the same response shape).
/// A device that keeps no such slots answers `Status::Unsupported`; one
/// that predates the byte answers `Status::Arg`.
pub fn parse_status_req(b: &[u8]) -> Result<u8, Status> {
    match b {
        [] => Ok(target::APP),
        [t] => Ok(*t),
        _ => Err(Status::Arg),
    }
}

/// Image header length announced in XFER_BEGIN for `target::APP`.
pub const IMAGE_HDR_LEN: u16 = 512;

pub mod reboot_mode {
    pub const NORMAL: u8 = 0;
}

/// ENTER_ROM `method`.
pub mod rom_method {
    /// Set FORCE_DOWNLOAD_BOOT, then software reset.
    pub const FORCE_DOWNLOAD_BOOT: u8 = 1;
    /// Ask the PMIC to restart the MCU in download mode.
    pub const PMIC: u8 = 2;
}

/// RELAY `link`.
pub mod link {
    pub const I2C0: u8 = 1;
    pub const PIO_UART: u8 = 2;
    /// `addr` for UART links.
    pub const UART_ADDR: u8 = 0xFF;
}

/// SHOW `phase`.
pub mod phase {
    pub const TRANSFER: u8 = 1;
    pub const VERIFY: u8 = 2;
    pub const REBOOT: u8 = 3;
    pub const CONFIRM: u8 = 4;
    pub const REJECTED: u8 = 5;
    pub const FAILED: u8 = 6;
    pub const ROM: u8 = 7;
}

pub const SHOW_TEXT_MAX: usize = 32;
pub const CONF_DETAIL_MAX: usize = 64;

/// SUBCTL `nrst`. NRST is only driven low or released, never high.
pub mod nrst {
    pub const RELEASE: u8 = 0;
    pub const LOW: u8 = 1;
    /// 10 ms low pulse.
    pub const PULSE: u8 = 2;
}

/// SUBCTL `boot0`.
pub mod boot0 {
    pub const LOW: u8 = 0;
    pub const HIGH: u8 = 1;
}

/// CONF_RUN `verdict`.
pub mod verdict {
    pub const PASS: u8 = 1;
    pub const FAIL: u8 = 2;
}

/// Per-slot state in STATUS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SlotState {
    /// No valid image.
    #[default]
    Empty = 0,
    /// Valid image, not selected for boot.
    Written = 1,
    /// Selected to be tried on the next boot.
    Pending = 2,
    /// Running on trial, not confirmed.
    Trying = 3,
    Confirmed = 4,
    /// Tried and rolled back.
    Reverted = 5,
}

impl SlotState {
    pub fn from_u8(v: u8) -> Option<SlotState> {
        use SlotState::*;
        [Empty, Written, Pending, Trying, Confirmed, Reverted]
            .into_iter()
            .find(|s| *s as u8 == v)
    }
}

struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Status> {
        if self.0.len() < n {
            return Err(Status::Arg);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, Status> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Status> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    fn u32(&mut self) -> Result<u32, Status> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], Status> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }
    fn rest(&mut self, max: usize) -> Result<&'a [u8], Status> {
        if self.0.len() > max {
            return Err(Status::Arg);
        }
        self.take(self.0.len())
    }
    fn end(&self) -> Result<(), Status> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Status::Arg)
        }
    }
}

struct Wr<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Wr<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Wr { buf, pos: 0 }
    }
    fn put(mut self, b: &[u8]) -> Result<Self, FrameError> {
        let end = self.pos + b.len();
        self.buf
            .get_mut(self.pos..end)
            .ok_or(FrameError::BufferTooSmall)?
            .copy_from_slice(b);
        self.pos = end;
        Ok(self)
    }
    fn u8(self, v: u8) -> Result<Self, FrameError> {
        self.put(&[v])
    }
    fn u16(self, v: u16) -> Result<Self, FrameError> {
        self.put(&v.to_le_bytes())
    }
    fn u32(self, v: u32) -> Result<Self, FrameError> {
        self.put(&v.to_le_bytes())
    }
    fn done(self) -> usize {
        self.pos
    }
}

/// HELLO request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloReq {
    pub proto_ver: u32,
    pub max_payload: u16,
}

impl HelloReq {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = HelloReq {
            proto_ver: r.u32()?,
            max_payload: r.u16()?,
        };
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out)
            .u32(self.proto_ver)?
            .u16(self.max_payload)?
            .done())
    }
}

/// HELLO response body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloResp {
    pub proto_ver: u32,
    pub role: u8,
    /// Image header `port_kind` values.
    pub port_kind: u8,
    pub mcu_id: u16,
    pub mode: u8,
    pub active_version: Version,
    /// Running on trial (not confirmed).
    pub trial: bool,
    pub key_id: [u8; 8],
    pub max_payload: u16,
}

impl HelloResp {
    pub const LEN: usize = 24;

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = HelloResp {
            proto_ver: r.u32()?,
            role: r.u8()?,
            port_kind: r.u8()?,
            mcu_id: r.u16()?,
            mode: r.u8()?,
            active_version: Version::from_raw(r.u32()?),
            trial: match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(Status::Arg),
            },
            key_id: r.arr()?,
            max_payload: r.u16()?,
        };
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out)
            .u32(self.proto_ver)?
            .u8(self.role)?
            .u8(self.port_kind)?
            .u16(self.mcu_id)?
            .u8(self.mode)?
            .u32(self.active_version.raw())?
            .u8(self.trial as u8)?
            .put(&self.key_id)?
            .u16(self.max_payload)?
            .done())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SlotInfo {
    pub state: SlotState,
    pub version: Version,
    /// First 16 bytes of the slot's image header SHA-256 (zero if empty).
    pub digest16: [u8; 16],
}

/// STATUS response body. Slots are A, B, factory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StatusResp {
    pub slots: [SlotInfo; 3],
    /// Running slot (`0xFF` none).
    pub active: u8,
    /// Last confirmed slot (`0xFF` none).
    pub confirmed: u8,
    /// Boot attempts of the slot on trial.
    pub attempts: u8,
    /// State log reason code of the last rollback/rejection (0 none).
    pub last_reason: u8,
    pub log_generation: u32,
}

impl StatusResp {
    pub const LEN: usize = 71;

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let mut slots = [SlotInfo::default(); 3];
        for s in &mut slots {
            *s = SlotInfo {
                state: SlotState::from_u8(r.u8()?).ok_or(Status::Arg)?,
                version: Version::from_raw(r.u32()?),
                digest16: r.arr()?,
            };
        }
        let v = StatusResp {
            slots,
            active: r.u8()?,
            confirmed: r.u8()?,
            attempts: r.u8()?,
            last_reason: r.u8()?,
            log_generation: r.u32()?,
        };
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = Wr::new(out);
        for s in &self.slots {
            w = w
                .u8(s.state as u8)?
                .u32(s.version.raw())?
                .put(&s.digest16)?;
        }
        Ok(w.u8(self.active)?
            .u8(self.confirmed)?
            .u8(self.attempts)?
            .u8(self.last_reason)?
            .u32(self.log_generation)?
            .done())
    }
}

/// VERSION request: empty for the summary set. A selector (a boot record
/// page, site/src/content/docs/en/explanation/lifecycle-events.md 5.4) is `Status::Unsupported`
/// here: this agent keeps no boot record.
pub fn parse_version_req(b: &[u8]) -> Result<(), Status> {
    if b.is_empty() {
        Ok(())
    } else {
        Err(Status::Unsupported)
    }
}

/// VERSION response TLV ids (site/src/content/docs/en/explanation/lifecycle-events.md 5.4). A
/// value the node cannot determine is left out; a reader skips ids it
/// does not know and values of a length it does not read.
pub mod version_id {
    /// The running boot core's version (`Version`, u32 LE).
    pub const BOOTCORE: u8 = 0x01;
    /// The image header `mcu_id` (u16 LE).
    pub const MCU: u8 = 0x06;
    /// The stage0 version mark (`dagon_format::stage0`; `Version`, u32 LE).
    pub const STAGE0: u8 = 0x0A;
    /// Boot core slots A, B: [`VersionSlots`].
    pub const BOOTCORE_SLOTS: u8 = 0x0B;
    /// App slots A, B, factory: [`VersionSlots`].
    pub const APP_SLOTS: u8 = 0x0C;
    /// The wire version the agent speaks (`PROTO_VERSION`, u8).
    pub const WIRE: u8 = 0x0D;

    /// Every id of the summary set and its value, as
    /// site/src/content/docs/en/explanation/lifecycle-events.md 5.4 lists them (a test keeps that
    /// table equal to this one). This agent sends the ids named above.
    pub const TABLE: [(u8, &str); 16] = [
        (0x01, "boot core version (`Version`, LE u32)"),
        (0x02, "installed boot image identity: first 8 bytes of its SHA-256"),
        (0x03, "agent (`dagon-agent`) version"),
        (0x04, "MCU package version"),
        (0x05, "board package version"),
        (0x06, "MCU identity (5.5): the value the image header and the compiled topology carry; with len 2 the image header's `mcu_id`, u16 LE"),
        (0x07, "isolation (4.5): `level u8`, `kind u8`, `boot_extensions u8`, `root_protection` (kind and exact level as the definition enumerates them, e.g. `rdp1`/`rdp2`, the write-protected sectors, and whether the level is reversible), `fuse` (Clear, or Tripped with the disabled extension's identity as the model encodes it, 5.5)"),
        (0x08, "extensions, for display: per linked extension `id` (up to 32 bytes), `trusted u8`, first 8 bytes of its source hash (4.7); repeated TLVs, one per extension"),
        (0x09, "extensions, for identity: per trusted extension its full 32-byte source hash (4.7); repeated TLVs"),
        (0x0A, "stage0 version: the version mark stage0 carries in its region (`dagon_format::stage0`; `Version`, LE u32); omitted for a stage0 without the mark and on a vendor-stage2 MCU"),
        (0x0B, "boot core slots A, B: `active u8` (the running slot), then per slot `state u8` (STATUS slot state) and `version` (LE u32)"),
        (0x0C, "app slots A, B, factory, in the shape of 0x0B"),
        (0x0D, "wire version the agent speaks (`PROTO_VERSION`, u8)"),
        (0x10, "formats: pairs `(format id u8, number u8)`: wire, image-header, state-log, event, definitions, boot-program, boot-record, extension-manifest"),
        (0x20, "boot record summary (5.5): which boots the record holds and the decision and reason of the newest; layout by the model"),
        (0x21, "one page of the raw boot record region: `offset u16`, then `len` bytes (`len ≤ RECORD_PAGE`, 10); the host reads the region page by page and decodes it with the model's reader"),
    ];
}

/// One slot in VERSION: its state and version (no digest; STATUS has it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SlotVersion {
    pub state: SlotState,
    pub version: Version,
}

/// Slots of one image kind and the running one (`0xFF` none), in STATUS
/// order: `active u8`, then per slot `state u8`, `version u32 LE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionSlots<const N: usize> {
    pub active: u8,
    pub slots: [SlotVersion; N],
}

impl<const N: usize> Default for VersionSlots<N> {
    fn default() -> Self {
        VersionSlots {
            active: 0,
            slots: [SlotVersion::default(); N],
        }
    }
}

impl<const N: usize> VersionSlots<N> {
    pub const LEN: usize = 1 + 5 * N;

    /// From a STATUS of the same slots (its first `N`).
    pub fn of(st: &StatusResp) -> Self {
        let mut slots = [SlotVersion::default(); N];
        for (o, s) in slots.iter_mut().zip(&st.slots) {
            *o = SlotVersion {
                state: s.state,
                version: s.version,
            };
        }
        VersionSlots {
            active: st.active,
            slots,
        }
    }

    /// The running slot's version.
    pub fn running(&self) -> Option<Version> {
        self.slots.get(usize::from(self.active)).map(|s| s.version)
    }

    fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let active = r.u8()?;
        let mut slots = [SlotVersion::default(); N];
        for s in &mut slots {
            *s = SlotVersion {
                state: SlotState::from_u8(r.u8()?).ok_or(Status::Arg)?,
                version: Version::from_raw(r.u32()?),
            };
        }
        r.end()?;
        Ok(VersionSlots { active, slots })
    }

    fn encode(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        let mut w = Wr::new(&mut b).u8(self.active).expect("fits");
        for s in &self.slots {
            w = w
                .u8(s.state as u8)
                .and_then(|w| w.u32(s.version.raw()))
                .expect("fits");
        }
        b
    }
}

/// VERSION response body: `count u8`, then `count` TLVs (`id u8`, `len
/// u8`, value). Adding a value is a new id; a field left `None` is not
/// sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VersionResp {
    pub wire: Option<u8>,
    pub mcu_id: Option<u16>,
    pub stage0: Option<Version>,
    /// The running boot core ([`version_id::BOOTCORE`]).
    pub boot_core: Option<Version>,
    pub core_slots: Option<VersionSlots<2>>,
    pub app_slots: Option<VersionSlots<3>>,
}

// The whole reply (a status byte before the body) fits the smallest link.
const _: () = assert!(VersionResp::MAX_LEN < crate::max_payload::I2C);

impl VersionResp {
    /// The longest body (every value present).
    pub const MAX_LEN: usize =
        1 + 6 * 2 + 1 + 2 + 4 + 4 + VersionSlots::<2>::LEN + VersionSlots::<3>::LEN;

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let mut v = VersionResp::default();
        for _ in 0..r.u8()? {
            let id = r.u8()?;
            let len = r.u8()?;
            let x = r.take(len.into())?;
            let u32_of = |x: &[u8]| x.try_into().ok().map(Version::from_le_bytes);
            match id {
                version_id::WIRE if len == 1 => v.wire = Some(x[0]),
                version_id::MCU if len == 2 => v.mcu_id = Some(u16::from_le_bytes([x[0], x[1]])),
                version_id::STAGE0 => v.stage0 = u32_of(x).or(v.stage0),
                version_id::BOOTCORE => v.boot_core = u32_of(x).or(v.boot_core),
                version_id::BOOTCORE_SLOTS if x.len() == VersionSlots::<2>::LEN => {
                    v.core_slots = Some(VersionSlots::parse(x)?)
                }
                version_id::APP_SLOTS if x.len() == VersionSlots::<3>::LEN => {
                    v.app_slots = Some(VersionSlots::parse(x)?)
                }
                _ => {}
            }
        }
        r.end().map(|_| v)
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let core = self.core_slots.map(|s| s.encode());
        let app = self.app_slots.map(|s| s.encode());
        let wire = self.wire.map(|w| [w]);
        let mcu = self.mcu_id.map(u16::to_le_bytes);
        let stage0 = self.stage0.map(Version::to_le_bytes);
        let boot_core = self.boot_core.map(Version::to_le_bytes);
        let tlvs: [(u8, Option<&[u8]>); 6] = [
            (version_id::WIRE, wire.as_ref().map(|x| &x[..])),
            (version_id::MCU, mcu.as_ref().map(|x| &x[..])),
            (version_id::STAGE0, stage0.as_ref().map(|x| &x[..])),
            (version_id::BOOTCORE, boot_core.as_ref().map(|x| &x[..])),
            (
                version_id::BOOTCORE_SLOTS,
                core.as_ref().map(|x| &x[..VersionSlots::<2>::LEN]),
            ),
            (
                version_id::APP_SLOTS,
                app.as_ref().map(|x| &x[..VersionSlots::<3>::LEN]),
            ),
        ];
        let count = tlvs.iter().filter(|t| t.1.is_some()).count() as u8;
        let mut w = Wr::new(out).u8(count)?;
        for (id, x) in tlvs {
            if let Some(x) = x {
                w = w.u8(id)?.u8(x.len() as u8)?.put(x)?;
            }
        }
        Ok(w.done())
    }
}

/// XFER_BEGIN request. `total_len` = `hdr_len` + body length; XFER_HDR
/// offsets run over the header, XFER_DATA offsets over the body, both from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XferBegin {
    pub target: u8,
    pub slot: u8,
    pub total_len: u32,
    pub hdr_len: u16,
}

impl XferBegin {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = XferBegin {
            target: r.u8()?,
            slot: r.u8()?,
            total_len: r.u32()?,
            hdr_len: r.u16()?,
        };
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out)
            .u8(self.target)?
            .u8(self.slot)?
            .u32(self.total_len)?
            .u16(self.hdr_len)?
            .done())
    }
    pub fn body_len(&self) -> Option<u32> {
        self.total_len.checked_sub(self.hdr_len as u32)
    }
}

/// XFER_HDR and XFER_DATA request. Response body: `next_offset` u32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XferChunk<'a> {
    pub offset: u32,
    pub data: &'a [u8],
}

impl<'a> XferChunk<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        Ok(XferChunk {
            offset: r.u32()?,
            data: r.rest(usize::MAX)?,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out).u32(self.offset)?.put(self.data)?.done())
    }
}

/// `next_offset` body of XFER_HDR / XFER_DATA responses.
pub fn parse_next_offset(b: &[u8]) -> Result<u32, Status> {
    let mut r = Rd(b);
    let v = r.u32()?;
    r.end().map(|_| v)
}

/// XFER_END response body: SHA-256 the receiver computed over the body.
pub fn parse_digest(b: &[u8]) -> Result<[u8; 32], Status> {
    let mut r = Rd(b);
    let v = r.arr()?;
    r.end().map(|_| v)
}

/// SET_PENDING request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetPending {
    pub target: u8,
    pub slot: u8,
}

impl SetPending {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = SetPending {
            target: r.u8()?,
            slot: r.u8()?,
        };
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out).u8(self.target)?.u8(self.slot)?.done())
    }
}

/// Single-byte requests: REBOOT `mode`, CONFIRM `target`.
pub fn parse_u8(b: &[u8]) -> Result<u8, Status> {
    let mut r = Rd(b);
    let v = r.u8()?;
    r.end().map(|_| v)
}

/// SHOW request: for the hub's display, or the display of the node being
/// written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Show<'a> {
    pub node: u8,
    pub phase: u8,
    pub percent: u8,
    pub version: Version,
    /// UTF-8, up to 32 bytes.
    pub text: &'a [u8],
}

impl<'a> Show<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        Ok(Show {
            node: r.u8()?,
            phase: r.u8()?,
            percent: r.u8()?,
            version: Version::from_raw(r.u32()?),
            text: r.rest(SHOW_TEXT_MAX)?,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        if self.text.len() > SHOW_TEXT_MAX {
            return Err(FrameError::BufferTooSmall);
        }
        Ok(Wr::new(out)
            .u8(self.node)?
            .u8(self.phase)?
            .u8(self.percent)?
            .u32(self.version.raw())?
            .put(self.text)?
            .done())
    }
}

/// RELAY request: forward `inner` (a complete frame) on `link`/`addr` and
/// wait up to `timeout_ms`. Response: status, then the node's response frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relay<'a> {
    pub link: u8,
    pub addr: u8,
    pub timeout_ms: u16,
    pub inner: &'a [u8],
}

impl<'a> Relay<'a> {
    pub const HEAD_LEN: usize = 4;

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        Ok(Relay {
            link: r.u8()?,
            addr: r.u8()?,
            timeout_ms: r.u16()?,
            inner: r.rest(usize::MAX)?,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out)
            .u8(self.link)?
            .u8(self.addr)?
            .u16(self.timeout_ms)?
            .put(self.inner)?
            .done())
    }
}

/// SUBCTL request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubCtl {
    pub nrst: u8,
    pub boot0: u8,
}

impl SubCtl {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = SubCtl {
            nrst: r.u8()?,
            boot0: r.u8()?,
        };
        if v.nrst > nrst::PULSE || v.boot0 > boot0::HIGH {
            return Err(Status::Arg);
        }
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out).u8(self.nrst)?.u8(self.boot0)?.done())
    }
}

/// ENTER_ROM request (an additive change). Response:
/// status only; the node enters its ROM loader after sending it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnterRom {
    pub method: u8,
}

impl EnterRom {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = EnterRom { method: r.u8()? };
        if !matches!(v.method, rom_method::FORCE_DOWNLOAD_BOOT | rom_method::PMIC) {
            return Err(Status::Arg);
        }
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out).u8(self.method)?.done())
    }
}

/// I2C_RAW request (to the ROM loader at 0x39). Response body: the bytes read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cRaw<'a> {
    pub read_len: u16,
    /// Bytes to write; `write_len` on the wire.
    pub data: &'a [u8],
}

impl<'a> I2cRaw<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let write_len = r.u16()? as usize;
        let read_len = r.u16()?;
        let data = r.take(write_len)?;
        r.end()?;
        Ok(I2cRaw { read_len, data })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let wl = u16::try_from(self.data.len()).map_err(|_| FrameError::BufferTooSmall)?;
        Ok(Wr::new(out)
            .u16(wl)?
            .u16(self.read_len)?
            .put(self.data)?
            .done())
    }
}

/// CONF_RUN request (conformance builds only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfRunReq {
    pub test_id: u8,
    pub param: u32,
}

impl ConfRunReq {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        let v = ConfRunReq {
            test_id: r.u8()?,
            param: r.u32()?,
        };
        r.end().map(|_| v)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        Ok(Wr::new(out).u8(self.test_id)?.u32(self.param)?.done())
    }
}

/// CONF_RUN response body, sent when the test has finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfRunResp<'a> {
    pub verdict: u8,
    pub measured: u32,
    pub detail: &'a [u8],
}

impl<'a> ConfRunResp<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let mut r = Rd(b);
        Ok(ConfRunResp {
            verdict: r.u8()?,
            measured: r.u32()?,
            detail: r.rest(CONF_DETAIL_MAX)?,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        if self.detail.len() > CONF_DETAIL_MAX {
            return Err(FrameError::BufferTooSmall);
        }
        Ok(Wr::new(out)
            .u8(self.verdict)?
            .u32(self.measured)?
            .put(self.detail)?
            .done())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_lengths() {
        let mut b = [0u8; 128];
        let h = HelloResp {
            proto_ver: 1,
            role: role::NODE,
            port_kind: 1,
            mcu_id: 2,
            mode: mode::APP,
            active_version: Version::new(1, 1, 0),
            trial: true,
            key_id: [9; 8],
            max_payload: 256,
        };
        assert_eq!(h.encode(&mut b).unwrap(), HelloResp::LEN);
        assert_eq!(HelloResp::parse(&b[..HelloResp::LEN]), Ok(h));
        let s = StatusResp {
            active: 0,
            confirmed: 0xFF,
            ..Default::default()
        };
        assert_eq!(s.encode(&mut b).unwrap(), StatusResp::LEN);
        assert_eq!(StatusResp::parse(&b[..StatusResp::LEN]), Ok(s));
        assert_eq!(
            StatusResp::parse(&b[..StatusResp::LEN - 1]),
            Err(Status::Arg)
        );
    }

    #[test]
    fn version_reply_skips_what_it_does_not_read() {
        let mut b = [0u8; 64];
        let full = VersionResp {
            wire: Some(1),
            mcu_id: Some(3),
            stage0: Some(Version::new(0, 1, 0)),
            boot_core: Some(Version::new(1, 0, 0)),
            core_slots: Some(VersionSlots::default()),
            app_slots: Some(VersionSlots::default()),
        };
        assert_eq!(full.encode(&mut b).unwrap(), VersionResp::MAX_LEN);
        let n = full.encode(&mut b).unwrap();
        assert_eq!(VersionResp::parse(&b[..n]), Ok(full));

        // Only what the node knows: no stage0, no boot core.
        let some = VersionResp {
            stage0: None,
            boot_core: None,
            core_slots: None,
            ..full
        };
        let n = some.encode(&mut b).unwrap();
        assert_eq!(b[0], 3, "three values sent");
        assert_eq!(VersionResp::parse(&b[..n]), Ok(some));

        // A later id and a known id at a length this reader does not read
        // are skipped.
        let mut later = b[..n].to_vec();
        later[0] += 2;
        later.extend([0x7E, 2, 0xAA, 0xBB, version_id::MCU, 8]);
        later.extend([0; 8]);
        assert_eq!(VersionResp::parse(&later), Ok(some));

        assert_eq!(
            VersionResp::parse(&b[..n - 1]),
            Err(Status::Arg),
            "cut short"
        );
        assert_eq!(VersionResp::parse(&[]), Err(Status::Arg));
        assert_eq!(
            VersionResp::parse(&[0, 0]),
            Err(Status::Arg),
            "trailing byte"
        );
        let mut bad_state = [0u8; 64];
        let n = VersionResp {
            app_slots: Some(VersionSlots::default()),
            ..VersionResp::default()
        }
        .encode(&mut bad_state)
        .unwrap();
        bad_state[4] = 6;
        assert_eq!(VersionResp::parse(&bad_state[..n]), Err(Status::Arg));
        assert_eq!(parse_version_req(&[]), Ok(()));
        assert_eq!(
            parse_version_req(&[0x21, 0, 0, 8]),
            Err(Status::Unsupported)
        );
    }

    #[test]
    fn variable_payloads() {
        let mut b = [0u8; 64];
        let raw = I2cRaw {
            read_len: 1,
            data: &[0x00, 0xFF],
        };
        let n = raw.encode(&mut b).unwrap();
        assert_eq!(&b[..n], &[2, 0, 1, 0, 0x00, 0xFF]);
        assert_eq!(I2cRaw::parse(&b[..n]), Ok(raw));
        assert_eq!(I2cRaw::parse(&b[..n - 1]), Err(Status::Arg));
        let long = [b'x'; 33];
        let show = Show {
            node: 1,
            phase: phase::ROM,
            percent: 0,
            version: Version::default(),
            text: &long,
        };
        assert!(show.encode(&mut b).is_err());
        assert_eq!(SubCtl::parse(&[3, 0]), Err(Status::Arg));
        assert_eq!(
            EnterRom::parse(&[2]),
            Ok(EnterRom {
                method: rom_method::PMIC
            })
        );
        for bad in [&[0][..], &[3], &[], &[1, 0]] {
            assert_eq!(EnterRom::parse(bad), Err(Status::Arg));
        }
    }
}
