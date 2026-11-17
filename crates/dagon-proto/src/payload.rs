//! Typed request and response payloads over the one reader and writer of
//! [`crate::field`] and the rows of [`crate::table`]. Response structs
//! describe the body after the status byte (see [`crate::split_status`]).
//! `parse` answers what the reader refuses (`Status::Arg`,
//! `Status::Field`) and `Status::Arg` for a value outside its range;
//! `encode` returns the byte count written.

use crate::field::{
    read, shortest, shortest_value, FieldSpec, FieldWriter, MsgSpec, Parsed, Refusal,
};
use crate::table::{
    conf_run, confirm, error, hello, i2c_raw, reboot, relay, set_pending, show, status, subctl,
    version, xfer_begin, xfer_end, ERROR,
};
use crate::{Cmd, FrameError, Status};
use dagon_format::Version;

/// HELLO / RELAY etc. field values.
pub mod role {
    pub const HUB: u8 = 0;
    pub const NODE: u8 = 1;
}

/// HELLO `mode`: what answers.
pub mod mode {
    /// The app's agent.
    pub const APP: u8 = 0;
    /// The boot core in update mode: it entered by STAY in its listen
    /// window or by the app's REBOOT mode 1, and takes a normal write.
    pub const UPDATE: u8 = 1;
    /// The boot core in recovery mode: its state log is unreadable or no
    /// app may start; it takes a normal write and starts no app by itself.
    pub const RECOVERY: u8 = 2;
    /// The recovery agent of a vendor-stage2 MCU.
    pub const AGENT: u8 = 3;
}

/// XFER_BEGIN / SET_PENDING / CONFIRM / STATUS `target`.
pub mod target {
    pub const APP: u8 = 0;
    /// A boot loader slot (A/B) of a self-boot MCU with a stage0; the new
    /// boot loader confirms itself, so CONFIRM never names it.
    pub const BOOTCORE: u8 = 1;
    /// The recovery agent's slot (`slot = 2`, factory) of a vendor-stage2
    /// MCU; written with its header last, never set pending or confirmed.
    pub const AGENT: u8 = 2;
    /// Reserved: the number of a removed target (a node's copy of the
    /// compiled topology), never given to another. No node takes it; one
    /// answers an XFER_BEGIN for it as for any target it does not take
    /// (`Status::Arg`).
    #[deprecated(note = "target 3 was removed and stays reserved; no node takes it")]
    pub const TOPOLOGY: u8 = 3;
}

/// Image header length announced in XFER_BEGIN for `target::APP`.
pub const IMAGE_HDR_LEN: u16 = 512;

/// REBOOT `mode`.
pub mod reboot_mode {
    pub const NORMAL: u8 = 0;
    /// Restart into the boot core's update mode: the app writes
    /// UPDATE_STAY to the state log, then resets; a boot core in update
    /// mode stays there. A node whose boot core has no update mode answers
    /// `Status::Unsupported`; one that predates the mode, `Status::Arg`.
    pub const UPDATE: u8 = 1;
}

/// ENTER_ROM `method`.
pub mod rom_method {
    /// Set FORCE_DOWNLOAD_BOOT, then software reset.
    pub const FORCE_DOWNLOAD_BOOT: u8 = 1;
    /// Ask the PMIC to restart the MCU in download mode.
    pub const PMIC: u8 = 2;
}

/// RELAY `link`. The link ids are an MCU's: its definition's
/// `[[relay_link]]` (`dagon_port::RelayLinkDef`).
pub mod link {
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

fn rd<'a>(b: &'a [u8], spec: &'a MsgSpec) -> Result<Parsed<'a>, Status> {
    read(b, spec).map_err(Status::from)
}

/// `N` bytes of `b` from `off`.
fn at<const N: usize>(b: &[u8], off: usize) -> Result<[u8; N], Status> {
    b.get(off..)
        .and_then(|x| x.get(..N))
        .and_then(|x| x.try_into().ok())
        .ok_or(Status::Arg)
}

/// A field's value as `N` bytes, when it was sent.
fn val<const N: usize>(p: &Parsed<'_>, f: &FieldSpec) -> Result<Option<[u8; N]>, Status> {
    p.field(f).map(|v| at(v, 0)).transpose()
}

/// A field this reader does not work without.
fn need<const N: usize>(p: &Parsed<'_>, f: &FieldSpec) -> Result<[u8; N], Status> {
    val(p, f)?.ok_or(Status::Arg)
}

fn byte(p: &Parsed<'_>, f: &FieldSpec, absent: u8) -> Result<u8, Status> {
    Ok(val::<1>(p, f)?.map_or(absent, |[v]| v))
}

/// A request or response that says nothing: no core and no field this
/// crate reads (STAY, XFER_END's request, every bare OK).
pub fn parse_empty(spec: &MsgSpec, b: &[u8]) -> Result<(), Status> {
    rd(b, spec).map(|_| ())
}

/// The longest refusal: the status, then the field number's key, length
/// and four varint bytes.
pub const REFUSAL_MAX: usize = 1 + 2 + 4;

/// The payload of a refusal: `status`, and for [`Status::Field`] the
/// number of the must field of `request` (the payload of a `cmd` request)
/// that was not known. Returns its length.
pub fn put_refusal(out: &mut [u8], status: Status, cmd: u8, request: &[u8]) -> usize {
    let Some((first, rest)) = out.split_first_mut() else {
        return 0;
    };
    *first = status as u8;
    let unknown = match Cmd::from_u8(cmd).map(|c| read(request, &c.spec().req)) {
        Some(Err(Refusal::Field(n))) if status == Status::Field => shortest(n),
        _ => None,
    };
    let mut w = FieldWriter::new(rest);
    if let Some((b, n)) = unknown {
        // Left out when the answer has no room: the status stands alone.
        let _ = b.get(..n).map(|v| w.field(&error::BAD_FIELD, v));
    }
    1 + w.len()
}

/// The field number a [`Status::Field`] response names (`body`: what
/// follows the status byte).
pub fn refused_field(body: &[u8]) -> Option<u32> {
    shortest_value(read(body, &ERROR).ok()?.field(&error::BAD_FIELD)?)
}

/// The wire version a `Status::Frame` response names (`body`: what follows
/// the status byte): the one its sender reads.
pub fn refused_wire(body: &[u8]) -> Option<u8> {
    read(body, &ERROR)
        .ok()?
        .field(&error::WIRE)?
        .first()
        .copied()
}

/// HELLO request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloReq {
    pub proto_ver: u32,
    pub max_payload: u16,
}

impl HelloReq {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::Hello.spec().req)?;
        Ok(HelloReq {
            proto_ver: u32::from_le_bytes(at(p.core, 0)?),
            max_payload: u16::from_le_bytes(at(p.core, 4)?),
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&self.proto_ver.to_le_bytes())?
            .put(&self.max_payload.to_le_bytes())?;
        Ok(w.len())
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
    /// The longest body this crate writes.
    pub const MAX_LEN: usize = 6 + 4 * 3 + 4 + 6 + 10;

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::Hello.spec().resp)?;
        let [role] = need(&p, &hello::ROLE)?;
        let [port_kind] = need(&p, &hello::PORT_KIND)?;
        let [mode] = need(&p, &hello::MODE)?;
        Ok(HelloResp {
            proto_ver: u32::from_le_bytes(at(p.core, 0)?),
            max_payload: u16::from_le_bytes(at(p.core, 4)?),
            role,
            port_kind,
            mcu_id: u16::from_le_bytes(need(&p, &hello::MCU_ID)?),
            mode,
            active_version: Version::from_le_bytes(need(&p, &hello::ACTIVE_VERSION)?),
            trial: match need(&p, &hello::TRIAL)? {
                [0] => false,
                [1] => true,
                _ => return Err(Status::Arg),
            },
            key_id: need(&p, &hello::KEY_ID)?,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&self.proto_ver.to_le_bytes())?
            .put(&self.max_payload.to_le_bytes())?
            .field(&hello::ROLE, &[self.role])?
            .field(&hello::PORT_KIND, &[self.port_kind])?
            .field(&hello::MCU_ID, &self.mcu_id.to_le_bytes())?
            .field(&hello::MODE, &[self.mode])?
            .field(&hello::ACTIVE_VERSION, &self.active_version.to_le_bytes())?
            .field(&hello::TRIAL, &[self.trial as u8])?
            .field(&hello::KEY_ID, &self.key_id)?;
        Ok(w.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SlotInfo {
    pub state: SlotState,
    pub version: Version,
    /// First 16 bytes of the slot's image header SHA-256 (zero if empty).
    pub digest16: [u8; 16],
}

const SLOT_LEN: usize = status::SLOT_LEN as usize;

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
    /// The longest body this crate writes.
    pub const MAX_LEN: usize = 2 + 3 * SLOT_LEN + 4 * 3 + 6;
    /// The image slot entries (A, B) before the factory entry: the most
    /// slots an image kind can have on the wire. A kind with fewer sends the
    /// others as empty.
    pub const IMAGE_SLOTS: usize = 2;
    /// `active` and `confirmed` of a response that leaves them out.
    pub const NO_SLOT: u8 = 0xFF;

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::Status.spec().resp)?;
        let mut slots = [SlotInfo::default(); 3];
        let sent = p.field(&status::SLOTS).unwrap_or(&[]);
        for (s, x) in slots.iter_mut().zip(sent.chunks_exact(SLOT_LEN)) {
            let [state] = at(x, 0)?;
            *s = SlotInfo {
                state: SlotState::from_u8(state).ok_or(Status::Arg)?,
                version: Version::from_le_bytes(at(x, 1)?),
                digest16: at(x, 5)?,
            };
        }
        let generation = val(&p, &status::LOG_GENERATION)?;
        Ok(StatusResp {
            slots,
            active: byte(&p, &status::ACTIVE, Self::NO_SLOT)?,
            confirmed: byte(&p, &status::CONFIRMED, Self::NO_SLOT)?,
            attempts: byte(&p, &status::ATTEMPTS, 0)?,
            last_reason: byte(&p, &status::LAST_REASON, 0)?,
            log_generation: generation.map_or(0, u32::from_le_bytes),
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut packed = [0u8; 3 * SLOT_LEN];
        for (x, s) in packed.chunks_exact_mut(SLOT_LEN).zip(&self.slots) {
            let mut w = FieldWriter::new(x);
            w.put(&[s.state as u8])?
                .put(&s.version.to_le_bytes())?
                .put(&s.digest16)?;
        }
        let mut w = FieldWriter::new(out);
        w.field(&status::SLOTS, &packed)?
            .field(&status::ACTIVE, &[self.active])?
            .field(&status::CONFIRMED, &[self.confirmed])?
            .field(&status::ATTEMPTS, &[self.attempts])?
            .field(&status::LAST_REASON, &[self.last_reason])?
            .field(&status::LOG_GENERATION, &self.log_generation.to_le_bytes())?;
        Ok(w.len())
    }
}

/// STATUS request: the slots of `target` (left out on the wire for
/// `target::APP`; `target::BOOTCORE`: the boot loader slots, in the same
/// response shape). A device that keeps no such slots answers
/// `Status::Unsupported`.
pub fn parse_status_req(b: &[u8]) -> Result<u8, Status> {
    byte(
        &rd(b, &Cmd::Status.spec().req)?,
        &status::TARGET,
        target::APP,
    )
}

pub fn encode_status_req(t: u8, out: &mut [u8]) -> Result<usize, FrameError> {
    let mut w = FieldWriter::new(out);
    w.field_unless(&status::TARGET, &[t], &[target::APP])?;
    Ok(w.len())
}

/// VERSION request: no field for the summary set. A selector (a boot
/// record page, site/src/content/docs/en/explanation/lifecycle-events.md
/// 5.4) is a must field, so an agent that keeps no boot record answers
/// `Status::Field` with its number.
pub fn parse_version_req(b: &[u8]) -> Result<(), Status> {
    parse_empty(&Cmd::Version.spec().req, b)
}

/// VERSION response field numbers
/// (site/src/content/docs/en/explanation/lifecycle-events.md 5.4). A
/// value the node cannot determine is left out; a reader skips the
/// numbers it does not know.
pub mod version_id {
    use crate::table::version;

    /// The running boot loader's version (`Version`, u32 LE).
    pub const BOOTCORE: u8 = version::BOOTCORE.num as u8;
    /// The image header `mcu_id` (u16 LE).
    pub const MCU: u8 = version::MCU.num as u8;
    /// The stage0 version mark (`dagon_format::stage0`; `Version`, u32 LE).
    pub const STAGE0: u8 = version::STAGE0.num as u8;
    /// Boot core slots A, B: [`super::VersionSlots`].
    pub const BOOTCORE_SLOTS: u8 = version::BOOTCORE_SLOTS.num as u8;
    /// App slots A, B, factory: [`super::VersionSlots`].
    pub const APP_SLOTS: u8 = version::APP_SLOTS.num as u8;
    /// The wire version the agent speaks (`PROTO_VERSION`, u8).
    pub const WIRE: u8 = version::WIRE.num as u8;

    /// Every number of the summary set and its value, as
    /// site/src/content/docs/en/explanation/lifecycle-events.md 5.4 lists them (a test keeps that
    /// table equal to this one). This agent sends the numbers named above.
    pub const TABLE: [(u8, &str); 16] = [
        (0x01, "boot loader version (`Version`, LE u32)"),
        (0x02, "installed boot image identity: first 8 bytes of its SHA-256"),
        (0x03, "agent (`dagon-agent`) version"),
        (0x04, "MCU definition version"),
        (0x05, "board definition version"),
        (0x06, "MCU identity (5.5): the image header's `mcu_id`, u16 LE"),
        (0x07, "isolation (4.5): `level u8`, `kind u8`, `boot_extensions u8`, `root_protection` (kind and exact level as the definition enumerates them, e.g. `rdp1`/`rdp2`, the write-protected sectors, and whether the level is reversible), `fuse` (Clear, or Tripped with the disabled extension's identity as the model encodes it, 5.5)"),
        (0x08, "extensions, for display: a packed array, per linked extension `id` (32 bytes, zero padded), `trusted u8`, first 8 bytes of its source hash (4.7)"),
        (0x09, "extensions, for identity: a packed array, per trusted extension its full 32-byte source hash (4.7)"),
        (0x0A, "stage0 version: the version mark stage0 carries in its region (`dagon_format::stage0`; `Version`, LE u32); omitted for a stage0 without the mark and on a vendor-stage2 MCU"),
        (0x0B, "boot loader slots A, B: `active u8` (the running slot), then per slot `state u8` (STATUS slot state) and `version` (LE u32)"),
        (0x0C, "app slots A, B, factory, in the shape of 0x0B"),
        (0x0D, "wire version the agent speaks (`PROTO_VERSION`, u8)"),
        (0x10, "formats: pairs `(format id u8, number u8)`: wire, image-header, state-log, event, definitions, boot-program, boot-record, extension-manifest"),
        (0x11, "boot record summary (5.5): which boots the record holds and the decision and reason of the newest; layout by the model"),
        (0x12, "one page of the raw boot record region: `offset u16`, then `len` bytes (`len ≤ RECORD_PAGE`, 10); the host reads the region page by page and decodes it with the model's reader"),
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
        let [active] = at(b, 0)?;
        let mut slots = [SlotVersion::default(); N];
        for (s, x) in slots
            .iter_mut()
            .zip(b.get(1..).unwrap_or(&[]).chunks_exact(5))
        {
            let [state] = at(x, 0)?;
            *s = SlotVersion {
                state: SlotState::from_u8(state).ok_or(Status::Arg)?,
                version: Version::from_le_bytes(at(x, 1)?),
            };
        }
        Ok(VersionSlots { active, slots })
    }

    fn encode(&self) -> Result<[u8; 16], FrameError> {
        let mut b = [0u8; 16];
        let mut w = FieldWriter::new(&mut b);
        w.put(&[self.active])?;
        for s in &self.slots {
            w.put(&[s.state as u8])?.put(&s.version.to_le_bytes())?;
        }
        Ok(b)
    }
}

/// VERSION response body: one field per value, numbered by
/// [`version_id`]. Adding a value is a new number; a value left `None` is
/// not sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct VersionResp {
    pub wire: Option<u8>,
    pub mcu_id: Option<u16>,
    pub stage0: Option<Version>,
    /// The running boot loader ([`version_id::BOOTCORE`]).
    pub boot_core: Option<Version>,
    pub core_slots: Option<VersionSlots<2>>,
    pub app_slots: Option<VersionSlots<3>>,
}

// The whole reply (a status byte before the body) fits the smallest link.
const _: () = assert!(VersionResp::MAX_LEN < crate::max_payload::I2C);

impl VersionResp {
    /// The longest body (every value present).
    pub const MAX_LEN: usize =
        6 * 2 + 1 + 2 + 4 + 4 + VersionSlots::<2>::LEN + VersionSlots::<3>::LEN;

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::Version.spec().resp)?;
        let slots2 = p.field(&version::BOOTCORE_SLOTS);
        let slots3 = p.field(&version::APP_SLOTS);
        Ok(VersionResp {
            wire: val::<1>(&p, &version::WIRE)?.map(|[v]| v),
            mcu_id: val(&p, &version::MCU)?.map(u16::from_le_bytes),
            stage0: val(&p, &version::STAGE0)?.map(Version::from_le_bytes),
            boot_core: val(&p, &version::BOOTCORE)?.map(Version::from_le_bytes),
            core_slots: slots2.map(VersionSlots::parse).transpose()?,
            app_slots: slots3.map(VersionSlots::parse).transpose()?,
        })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        if let Some(v) = self.boot_core {
            w.field(&version::BOOTCORE, &v.to_le_bytes())?;
        }
        if let Some(v) = self.mcu_id {
            w.field(&version::MCU, &v.to_le_bytes())?;
        }
        if let Some(v) = self.stage0 {
            w.field(&version::STAGE0, &v.to_le_bytes())?;
        }
        if let Some(s) = self.core_slots {
            let b = s.encode()?;
            let v = b.get(..VersionSlots::<2>::LEN);
            w.field(&version::BOOTCORE_SLOTS, v.unwrap_or(&[]))?;
        }
        if let Some(s) = self.app_slots {
            w.field(&version::APP_SLOTS, &s.encode()?)?;
        }
        if let Some(v) = self.wire {
            w.field(&version::WIRE, &[v])?;
        }
        Ok(w.len())
    }
}

/// XFER_BEGIN request. `total_len` = `hdr_len` + body length; XFER_HDR
/// offsets run over the header, XFER_DATA offsets over the body, both from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XferBegin {
    pub target: u8,
    pub slot: u8,
    pub total_len: u32,
    /// 0 (left out on the wire): no header.
    pub hdr_len: u16,
}

impl XferBegin {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::XferBegin.spec().req)?;
        let [target] = at(p.core, 0)?;
        let [slot] = need(&p, &xfer_begin::SLOT)?;
        Ok(XferBegin {
            target,
            slot,
            total_len: u32::from_le_bytes(at(p.core, 1)?),
            hdr_len: val(&p, &xfer_begin::HDR_LEN)?.map_or(0, u16::from_le_bytes),
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.target])?
            .put(&self.total_len.to_le_bytes())?
            .field(&xfer_begin::SLOT, &[self.slot])?
            .field_unless(&xfer_begin::HDR_LEN, &self.hdr_len.to_le_bytes(), &[0, 0])?;
        Ok(w.len())
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
    /// Bytes of the request around its data.
    pub const OVERHEAD: usize = 4 + 1;

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::XferData.spec().req)?;
        Ok(XferChunk {
            offset: u32::from_le_bytes(at(p.core, 0)?),
            data: p.bulk,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&self.offset.to_le_bytes())?.bulk(self.data)?;
        Ok(w.len())
    }
}

/// `next_offset` body of XFER_HDR / XFER_DATA responses.
pub fn parse_next_offset(b: &[u8]) -> Result<u32, Status> {
    let p = rd(b, &Cmd::XferData.spec().resp)?;
    Ok(u32::from_le_bytes(at(p.core, 0)?))
}

pub fn encode_next_offset(next: u32, out: &mut [u8]) -> Result<usize, FrameError> {
    let mut w = FieldWriter::new(out);
    w.put(&next.to_le_bytes())?;
    Ok(w.len())
}

/// XFER_END response body: SHA-256 the receiver computed over the body.
pub fn parse_digest(b: &[u8]) -> Result<[u8; 32], Status> {
    need(&rd(b, &Cmd::XferEnd.spec().resp)?, &xfer_end::SHA256)
}

pub fn encode_digest(digest: &[u8; 32], out: &mut [u8]) -> Result<usize, FrameError> {
    let mut w = FieldWriter::new(out);
    w.field(&xfer_end::SHA256, digest)?;
    Ok(w.len())
}

/// SET_PENDING request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetPending {
    pub target: u8,
    pub slot: u8,
}

impl SetPending {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::SetPending.spec().req)?;
        let [target] = at(p.core, 0)?;
        let [slot] = need(&p, &set_pending::SLOT)?;
        Ok(SetPending { target, slot })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.target])?
            .field(&set_pending::SLOT, &[self.slot])?;
        Ok(w.len())
    }
}

/// REBOOT request: its `mode` ([`reboot_mode`]; left out on the wire for
/// a normal restart).
pub fn parse_reboot(b: &[u8]) -> Result<u8, Status> {
    let p = rd(b, &Cmd::Reboot.spec().req)?;
    byte(&p, &reboot::MODE, reboot_mode::NORMAL)
}

pub fn encode_reboot(mode: u8, out: &mut [u8]) -> Result<usize, FrameError> {
    let mut w = FieldWriter::new(out);
    w.field_unless(&reboot::MODE, &[mode], &[reboot_mode::NORMAL])?;
    Ok(w.len())
}

/// CONFIRM request: its `target` (left out on the wire for the app).
pub fn parse_confirm(b: &[u8]) -> Result<u8, Status> {
    let p = rd(b, &Cmd::Confirm.spec().req)?;
    byte(&p, &confirm::TARGET, target::APP)
}

pub fn encode_confirm(t: u8, out: &mut [u8]) -> Result<usize, FrameError> {
    let mut w = FieldWriter::new(out);
    w.field_unless(&confirm::TARGET, &[t], &[target::APP])?;
    Ok(w.len())
}

/// SHOW request: the host's progress for the node being written, which
/// that node's agent hands to its app.
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
    /// The longest request.
    pub const MAX_LEN: usize = 1 + 3 + 3 + 6 + 2 + SHOW_TEXT_MAX;

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::Show.spec().req)?;
        let [phase] = at(p.core, 0)?;
        Ok(Show {
            node: byte(&p, &show::NODE, 0)?,
            phase,
            percent: byte(&p, &show::PERCENT, 0)?,
            version: val(&p, &show::VERSION)?.map_or(Version::default(), Version::from_le_bytes),
            text: p.field(&show::TEXT).unwrap_or(&[]),
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.phase])?
            .field(&show::NODE, &[self.node])?
            .field(&show::PERCENT, &[self.percent])?
            .field(&show::VERSION, &self.version.to_le_bytes())?
            .field(&show::TEXT, self.text)?;
        Ok(w.len())
    }
}

/// RELAY request: forward `inner` (a complete frame) on `link`/`addr` and
/// wait up to `timeout_ms`. Response: status, then the node's response
/// frame as bulk ([`parse_bulk_resp`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relay<'a> {
    pub link: u8,
    /// `link::UART_ADDR` (left out on the wire): a link without addresses.
    pub addr: u8,
    /// `None`: the relay's own wait for that link.
    pub timeout_ms: Option<u16>,
    pub inner: &'a [u8],
}

impl<'a> Relay<'a> {
    /// The most bytes of a request around its inner frame: the link, both
    /// fields and the delimiter.
    pub const OVERHEAD_MAX: usize = 1 + 3 + 4 + 1;

    /// The bytes of an OK response around the node's response frame: the
    /// status and the delimiter.
    pub const RESP_OVERHEAD: usize = 1 + BULK_RESP_AT;

    /// The largest inner frame a relay forwards over a link carrying
    /// `egress` payload bytes, for a request that came in on a link
    /// carrying `ingress`: one frame of the egress link, and no more than
    /// a RELAY request and its OK response carry on the ingress link. For
    /// a relay reached through another, `ingress` is what the outer tunnel
    /// carries as payload (its MTU less `frame_len(0)`).
    pub const fn tunnel_mtu(ingress: usize, egress: usize) -> usize {
        let request = ingress.saturating_sub(Self::OVERHEAD_MAX);
        let response = ingress.saturating_sub(Self::RESP_OVERHEAD);
        let link = crate::frame_len(egress);
        let around = if request < response {
            request
        } else {
            response
        };
        if link < around {
            link
        } else {
            around
        }
    }

    /// Whether a relay forwards this request from a link carrying
    /// `ingress` payload bytes over one carrying `egress`: its inner frame
    /// is one whole frame of this wire version (start, version, length and
    /// CRC; the payload is not read) within [`Relay::tunnel_mtu`].
    pub fn admits(&self, ingress: usize, egress: usize) -> bool {
        let n = self.inner.len();
        n <= Self::tunnel_mtu(ingress, egress)
            && crate::Frame::decode(self.inner, n.saturating_sub(crate::frame_len(0)))
                .is_ok_and(|(_, len)| len == n)
    }

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::Relay.spec().req)?;
        let [link] = at(p.core, 0)?;
        Ok(Relay {
            link,
            addr: byte(&p, &relay::ADDR, link::UART_ADDR)?,
            timeout_ms: val(&p, &relay::TIMEOUT_MS)?.map(u16::from_le_bytes),
            inner: p.bulk,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.link])?
            .field_unless(&relay::ADDR, &[self.addr], &[link::UART_ADDR])?;
        if let Some(ms) = self.timeout_ms {
            w.field(&relay::TIMEOUT_MS, &ms.to_le_bytes())?;
        }
        w.bulk(self.inner)?;
        Ok(w.len())
    }
}

/// Where the bulk of an OK RELAY or I2C_RAW response starts in its body
/// (after the delimiter), for a relay that receives it in place.
pub const BULK_RESP_AT: usize = 1;

/// The body of an OK RELAY or I2C_RAW response whose bulk is already at
/// `out[BULK_RESP_AT..][..bulk_len]`; returns the body's length.
pub fn finish_bulk_resp(out: &mut [u8], bulk_len: usize) -> Result<usize, FrameError> {
    *out.first_mut().ok_or(FrameError::BufferTooSmall)? = 0;
    Ok(BULK_RESP_AT + bulk_len)
}

pub fn encode_bulk_resp(bulk: &[u8], out: &mut [u8]) -> Result<usize, FrameError> {
    let mut w = FieldWriter::new(out);
    w.bulk(bulk)?;
    Ok(w.len())
}

/// The bulk of an OK response of `cmd` (RELAY: the node's response frame;
/// I2C_RAW: the bytes read).
pub fn parse_bulk_resp(cmd: Cmd, b: &[u8]) -> Result<&[u8], Status> {
    Ok(rd(b, &cmd.spec().resp)?.bulk)
}

/// SUBCTL request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubCtl {
    pub nrst: u8,
    pub boot0: u8,
}

impl SubCtl {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::SubCtl.spec().req)?;
        let [n] = need(&p, &subctl::NRST)?;
        let [b0] = need(&p, &subctl::BOOT0)?;
        if n > nrst::PULSE || b0 > boot0::HIGH {
            return Err(Status::Arg);
        }
        Ok(SubCtl { nrst: n, boot0: b0 })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.field(&subctl::NRST, &[self.nrst])?
            .field(&subctl::BOOT0, &[self.boot0])?;
        Ok(w.len())
    }
}

/// ENTER_ROM request. Response: status only; the node enters its ROM
/// loader after sending it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnterRom {
    pub method: u8,
}

impl EnterRom {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::EnterRom.spec().req)?;
        let [method] = at(p.core, 0)?;
        if !matches!(method, rom_method::FORCE_DOWNLOAD_BOOT | rom_method::PMIC) {
            return Err(Status::Arg);
        }
        Ok(EnterRom { method })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.method])?;
        Ok(w.len())
    }
}

/// I2C_RAW request (to the ROM loader on the recovery lines, at the address
/// its MCU definition gives). Response: the bytes read, as bulk
/// ([`parse_bulk_resp`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cRaw<'a> {
    /// 0 (left out on the wire): nothing is read.
    pub read_len: u16,
    /// Bytes to write.
    pub data: &'a [u8],
}

impl<'a> I2cRaw<'a> {
    /// The most bytes of a request around its data.
    pub const OVERHEAD_MAX: usize = 4 + 1;

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::I2cRaw.spec().req)?;
        Ok(I2cRaw {
            read_len: val(&p, &i2c_raw::READ_LEN)?.map_or(0, u16::from_le_bytes),
            data: p.bulk,
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.field_unless(&i2c_raw::READ_LEN, &self.read_len.to_le_bytes(), &[0, 0])?
            .bulk(self.data)?;
        Ok(w.len())
    }
}

/// CONF_RUN request (conformance builds only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfRunReq {
    pub test_id: u8,
    /// 0 is left out on the wire.
    pub param: u32,
}

impl ConfRunReq {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::ConfRun.spec().req)?;
        let [test_id] = at(p.core, 0)?;
        Ok(ConfRunReq {
            test_id,
            param: val(&p, &conf_run::PARAM)?.map_or(0, u32::from_le_bytes),
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.test_id])?.field_unless(
            &conf_run::PARAM,
            &self.param.to_le_bytes(),
            &[0; 4],
        )?;
        Ok(w.len())
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
    /// The longest body.
    pub const MAX_LEN: usize = 1 + 6 + 2 + CONF_DETAIL_MAX;

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::ConfRun.spec().resp)?;
        let [verdict] = at(p.core, 0)?;
        Ok(ConfRunResp {
            verdict,
            measured: val(&p, &conf_run::MEASURED)?.map_or(0, u32::from_le_bytes),
            detail: p.field(&conf_run::DETAIL).unwrap_or(&[]),
        })
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.verdict])?
            .field(&conf_run::MEASURED, &self.measured.to_le_bytes())?
            .field(&conf_run::DETAIL, self.detail)?;
        Ok(w.len())
    }
}

// The table's lengths are the ones the typed payloads name.
const _: () = assert!(show::TEXT.max as usize == SHOW_TEXT_MAX);
const _: () = assert!(conf_run::DETAIL.max as usize == CONF_DETAIL_MAX);
const _: () = assert!(version::BOOTCORE_SLOTS.max as usize == VersionSlots::<2>::LEN);
const _: () = assert!(version::APP_SLOTS.max as usize == VersionSlots::<3>::LEN);

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(m: u8) -> HelloResp {
        HelloResp {
            proto_ver: 1,
            role: role::NODE,
            port_kind: 1,
            mcu_id: 2,
            mode: m,
            active_version: Version::new(1, 1, 0),
            trial: true,
            key_id: [9; 8],
            max_payload: 256,
        }
    }

    #[test]
    fn longest_bodies() {
        let mut b = [0u8; 128];
        let h = hello(mode::APP);
        assert_eq!(h.encode(&mut b).unwrap(), HelloResp::MAX_LEN);
        assert_eq!(HelloResp::parse(&b[..HelloResp::MAX_LEN]), Ok(h));
        let s = StatusResp {
            active: 0,
            confirmed: 0xFF,
            ..Default::default()
        };
        assert_eq!(s.encode(&mut b).unwrap(), StatusResp::MAX_LEN);
        assert_eq!(StatusResp::parse(&b[..StatusResp::MAX_LEN]), Ok(s));
        assert_eq!(
            StatusResp::parse(&b[..StatusResp::MAX_LEN - 1]),
            Err(Status::Arg)
        );
        let show = Show {
            node: 1,
            phase: phase::ROM,
            percent: 0,
            version: Version::default(),
            text: &[b'x'; SHOW_TEXT_MAX],
        };
        assert_eq!(show.encode(&mut b).unwrap(), Show::MAX_LEN);
        let long = Show {
            text: &[b'x'; SHOW_TEXT_MAX + 1],
            ..show
        };
        assert_eq!(long.encode(&mut b), Err(FrameError::Field));
    }

    /// HELLO's modes (0 app, 1 update, 2 recovery, 3 agent) each round
    /// trip; REBOOT mode 1 asks for update mode.
    #[test]
    fn hello_modes_and_reboot_into_update_mode() {
        let modes = [mode::APP, mode::UPDATE, mode::RECOVERY, mode::AGENT];
        assert_eq!(modes, [0, 1, 2, 3]);
        let mut b = [0u8; HelloResp::MAX_LEN];
        for m in modes {
            assert_eq!(hello(m).encode(&mut b).unwrap(), HelloResp::MAX_LEN);
            assert_eq!(HelloResp::parse(&b), Ok(hello(m)));
        }
        assert_eq!((reboot_mode::NORMAL, reboot_mode::UPDATE), (0, 1));
        let n = encode_reboot(reboot_mode::UPDATE, &mut b).unwrap();
        assert_eq!(parse_reboot(&b[..n]), Ok(reboot_mode::UPDATE));
        assert_eq!(encode_reboot(reboot_mode::NORMAL, &mut b), Ok(0));
        assert_eq!(parse_reboot(&[]), Ok(reboot_mode::NORMAL));
    }

    #[test]
    fn what_is_left_out_reads_as_its_default() {
        assert_eq!(parse_status_req(&[]), Ok(target::APP));
        assert_eq!(parse_confirm(&[]), Ok(target::APP));
        assert_eq!(
            StatusResp::parse(&[]),
            Ok(StatusResp {
                active: StatusResp::NO_SLOT,
                confirmed: StatusResp::NO_SLOT,
                ..Default::default()
            })
        );
        assert_eq!(VersionResp::parse(&[]), Ok(VersionResp::default()));
        let begin = XferBegin::parse(&[2, 9, 0, 0, 0, 3, 1, 2]).unwrap();
        assert_eq!((begin.slot, begin.hdr_len, begin.total_len), (2, 0, 9));
        assert_eq!(
            XferBegin::parse(&[2, 9, 0, 0, 0]),
            Err(Status::Arg),
            "no slot"
        );
        let relay = Relay::parse(&[1, 0, 7]).unwrap();
        assert_eq!((relay.addr, relay.timeout_ms), (link::UART_ADDR, None));
        assert_eq!(relay.inner, &[7]);
        assert_eq!(I2cRaw::parse(&[0]).map(|r| r.read_len), Ok(0));
        assert_eq!(HelloResp::parse(&[1, 0, 0, 0, 0, 1]), Err(Status::Arg));
    }

    #[test]
    fn values_outside_their_range_are_refused() {
        let mut b = [0u8; 16];
        let n = SubCtl { nrst: 3, boot0: 0 }.encode(&mut b).unwrap();
        assert_eq!(SubCtl::parse(&b[..n]), Err(Status::Arg));
        assert_eq!(
            EnterRom::parse(&[2]),
            Ok(EnterRom {
                method: rom_method::PMIC
            })
        );
        for bad in [&[0][..], &[3], &[]] {
            assert_eq!(EnterRom::parse(bad), Err(Status::Arg));
        }
        let mut bad_state = [0u8; 64];
        let n = VersionResp {
            app_slots: Some(VersionSlots::default()),
            ..VersionResp::default()
        }
        .encode(&mut bad_state)
        .unwrap();
        bad_state[3] = 6;
        assert_eq!(VersionResp::parse(&bad_state[..n]), Err(Status::Arg));
        // A selector this agent does not know: a must field.
        assert_eq!(parse_version_req(&[0x25, 2, 0, 8]), Err(Status::Field));
    }
}
