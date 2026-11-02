//! PC side: the conformance sequence, driven through the hub.
//!
//! [`Runner::run_all`] runs [`CTS`](crate::CTS) in order against one node.
//! Start with the node running a confirmed conformance build. The run leaves
//! the node running `good` from the other slot, confirmed.
//!
//! | Test | Procedure | Passes when |
//! |---|---|---|
//! | CT-01..03, 07, 08 | CONF_RUN `test_id` (param 0) | verdict PASS |
//! | CT-04 | XFER_BEGIN to the running slot (and the confirmed slot if different) | each answers E_STATE |
//! | CT-20 | header re-signed with another `mcu_id`, then another `target_tag`, sent as XFER_BEGIN + XFER_HDR to the idle slot | the last XFER_HDR answers E_COMPAT, each time |
//! | CT-09 | 20 HELLO, then `good` to the idle slot with full-size XFER_DATA | all OK, XFER_END digest matches, within [`timeout::transfer_ms`] |
//! | CT-10 | REBOOT | HELLO within [`timeout::REBOOT_WAIT_MS`], same version, not on trial |
//! | CT-11 | `good` to the idle slot, SET_PENDING, REBOOT, CONFIRM, REBOOT | new slot on trial, then confirmed, still confirmed after the second reboot |
//! | CT-12 | `no_confirm` to the idle slot, SET_PENDING, REBOOT, never CONFIRM | back on the previous slot within [`timeout::REBOOT_WAIT_MS`], STATUS `last_reason` = TRIAL_EXHAUSTED, tried slot REVERTED |
//! | CT-18 | HELLO + STATUS after CT-11 and after CT-12 | every field matches the state those tests produced |

use crate::report::{CtResult, Report, Verdict};
use crate::{ct_info, detail, timeout, CtInfo, Detail, CTS};
use dagon_format::header::{self, ImageHeader, HEADER_SIZE, SIGNED_LEN};
use dagon_format::statelog::reason;
use dagon_format::{sha256, Version};
use dagon_port::LinkKind;
use dagon_proto::payload::{
    self, reboot_mode, role, target, ConfRunReq, ConfRunResp, HelloReq, HelloResp, SetPending,
    SlotState, StatusResp, XferBegin, XferChunk, IMAGE_HDR_LEN,
};
use dagon_proto::{split_status, Cmd, Status};

/// The way to the node under test (normally RELAY through the hub).
pub trait Channel {
    /// Sends one request and waits up to `timeout_ms` for the answer. Writes
    /// the response payload (status byte first) to `resp` and returns its
    /// length. `Err` when no answer arrived.
    fn request(
        &mut self,
        cmd: Cmd,
        payload: &[u8],
        timeout_ms: u32,
        resp: &mut [u8],
    ) -> Result<usize, NoAnswer>;
    /// Monotonic milliseconds.
    fn now_ms(&mut self) -> u64;
    fn sleep_ms(&mut self, ms: u32);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoAnswer;

/// Signs an image header: returns Ed25519(`SIGN_DOMAIN` || `signed_part`)
/// with the release key. `signed_part` is header bytes `0..SIGNED_LEN`.
pub type Signer<'s> = &'s mut dyn FnMut(&[u8]) -> [u8; 64];

/// Images the run sends, index = slot (0 A, 1 B). Each is a full signed image
/// (512-byte header + body); boards with slot-independent images pass the
/// same image twice.
#[derive(Clone, Copy, Debug)]
pub struct Images<'a> {
    /// A conformance build that confirms normally.
    pub good: [&'a [u8]; 2],
    /// A build that never confirms. Its version must differ from `good`.
    pub no_confirm: [&'a [u8]; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupError {
    /// An image is not a parsable header + body of the declared size and
    /// digest; index into `good` then `no_confirm` (0..4).
    BadImage(usize),
    /// `no_confirm` has the same version as `good`.
    SameVersion,
    /// The first HELLO / STATUS did not answer.
    Unreachable,
}

const RESP_MAX: usize = 1 + StatusResp::LEN;
const HELLO_ROUNDS: u32 = 20;

/// What HELLO and STATUS should show after CT-11 / CT-12.
#[derive(Clone, Copy, Debug)]
struct Expected {
    active: u8,
    active_version: Version,
    active_digest: [u8; 16],
    other_state: SlotState,
    other_version: Version,
    other_digest: [u8; 16],
    last_reason: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
struct Img<'a> {
    raw: &'a [u8],
    hdr: [u8; HEADER_SIZE],
    parsed: Ident,
}

#[derive(Clone, Copy, Debug)]
struct Ident {
    version: Version,
    mcu_id: u16,
    port_kind: u8,
    key_id: [u8; 8],
}

impl<'a> Img<'a> {
    fn new(raw: &'a [u8], slot: u8) -> Option<Self> {
        let hdr: [u8; HEADER_SIZE] = raw.get(..HEADER_SIZE)?.try_into().ok()?;
        let h = ImageHeader::parse(&hdr).ok()?;
        let body = &raw[HEADER_SIZE..];
        let bound = h.slot_binding == slot || h.slot_binding == header::slot_binding::ANY;
        if !bound || h.payload_size as usize != body.len() || h.payload_sha256 != sha256(body) {
            return None;
        }
        Some(Img {
            raw,
            hdr,
            parsed: Ident {
                version: h.version,
                mcu_id: h.mcu_id,
                port_kind: h.port_kind,
                key_id: h.key_id,
            },
        })
    }

    fn digest16(&self) -> [u8; 16] {
        header::header_digest16(&self.hdr)
    }
}

/// One conformance run against one node.
pub struct Runner<'a, 's, C: Channel> {
    ch: C,
    link: LinkKind,
    good: [Img<'a>; 2],
    no_confirm: [Img<'a>; 2],
    sign: Signer<'s>,
    hello: HelloResp,
    after_ct11: Option<Result<u32, Detail>>,
    after_ct12: Option<Expected>,
}

type Step<T> = Result<T, Detail>;

impl<'a, 's, C: Channel> Runner<'a, 's, C> {
    /// Checks the images and greets the node.
    pub fn new(
        mut ch: C,
        link: LinkKind,
        images: Images<'a>,
        sign: Signer<'s>,
    ) -> Result<Self, SetupError> {
        let img = |i: usize, raw| Img::new(raw, (i % 2) as u8).ok_or(SetupError::BadImage(i));
        let good = [img(0, images.good[0])?, img(1, images.good[1])?];
        let no_confirm = [img(2, images.no_confirm[0])?, img(3, images.no_confirm[1])?];
        if (0..2).any(|s| no_confirm[s].parsed.version == good[s].parsed.version) {
            return Err(SetupError::SameVersion);
        }
        let hello = hello_once(&mut ch).map_err(|_| SetupError::Unreachable)?;
        Ok(Runner {
            ch,
            link,
            good,
            no_confirm,
            sign,
            hello,
            after_ct11: None,
            after_ct12: None,
        })
    }

    pub fn channel(&mut self) -> &mut C {
        &mut self.ch
    }

    /// Runs every test in [`CTS`] order.
    pub fn run_all(&mut self) -> Report {
        let results = CTS.map(|c| self.run(c.id));
        Report {
            hello: Some(self.hello),
            link: self.link,
            results,
        }
    }

    /// Runs one test by its `CT-nn` number.
    pub fn run(&mut self, id: u8) -> CtResult {
        let Some(info) = ct_info(id) else {
            return CtResult::not_run(id);
        };
        let t0 = self.ch.now_ms();
        let r = self.dispatch(info);
        let duration_ms = (self.ch.now_ms() - t0) as u32;
        let (verdict, measured, detail) = match r {
            Ok((m, d)) => (Verdict::Pass, m, d),
            Err(d) => (Verdict::Fail, 0, d),
        };
        CtResult {
            id,
            verdict,
            measured,
            duration_ms,
            detail,
        }
    }

    fn dispatch(&mut self, info: &CtInfo) -> Step<(u32, Detail)> {
        let plain = |r: Step<u32>| r.map(|m| (m, Detail::new()));
        match info.id {
            1 | 2 | 3 | 7 | 8 => self.conf_run(info.id),
            4 => plain(self.ct04()),
            20 => plain(self.ct20()),
            9 => plain(self.ct09()),
            10 => plain(self.ct10()),
            11 => plain(self.ct11()),
            12 => plain(self.ct12()),
            18 => plain(self.ct18()),
            _ => Err(detail!("no procedure")),
        }
    }

    // ---- requests -------------------------------------------------------

    fn call(&mut self, cmd: Cmd, payload: &[u8], timeout_ms: u32) -> Step<Resp> {
        let mut r = Resp {
            buf: [0; RESP_MAX],
            len: 0,
            status: Status::Ok,
        };
        let n = self
            .ch
            .request(cmd, payload, timeout_ms, &mut r.buf)
            .map_err(|_| detail!("{:?}: no answer in {} ms", cmd, timeout_ms))?;
        let (status, body) = split_status(r.buf.get(..n).unwrap_or(&[]))
            .ok_or_else(|| detail!("{:?}: bad response", cmd))?;
        r.len = body.len();
        r.status = status;
        r.buf.copy_within(1..1 + r.len, 0);
        Ok(r)
    }

    /// Request that must answer OK; returns the body.
    fn ok(&mut self, cmd: Cmd, payload: &[u8], timeout_ms: u32) -> Step<Resp> {
        let r = self.call(cmd, payload, timeout_ms)?;
        if r.status != Status::Ok {
            return Err(detail!("{:?}: {:?}", cmd, r.status));
        }
        Ok(r)
    }

    fn hello(&mut self) -> Step<HelloResp> {
        let r = self.ok(Cmd::Hello, &hello_req(), timeout::REQUEST_MS)?;
        HelloResp::parse(r.body()).map_err(|_| detail!("HELLO: bad body"))
    }

    fn status(&mut self) -> Step<StatusResp> {
        let r = self.ok(Cmd::Status, &[], timeout::REQUEST_MS)?;
        StatusResp::parse(r.body()).map_err(|_| detail!("STATUS: bad body"))
    }

    fn conf_run(&mut self, id: u8) -> Step<(u32, Detail)> {
        let mut p = [0u8; 5];
        ConfRunReq {
            test_id: id,
            param: 0,
        }
        .encode(&mut p)
        .map_err(|_| detail!("encode"))?;
        let r = self.ok(Cmd::ConfRun, &p, timeout::CONF_RUN_MS)?;
        let c = ConfRunResp::parse(r.body()).map_err(|_| detail!("CONF_RUN: bad body"))?;
        let d = Detail::from_bytes(c.detail);
        match c.verdict {
            payload::verdict::PASS => Ok((c.measured, d)),
            payload::verdict::FAIL => Err(d),
            v => Err(detail!("CONF_RUN: verdict {}", v)),
        }
    }

    /// Running slot (A or B) and the other one.
    fn slots(&mut self) -> Step<(StatusResp, u8, u8)> {
        let st = self.status()?;
        if st.active > 1 {
            return Err(detail!("running slot {} is not A or B", st.active));
        }
        Ok((st, st.active, 1 - st.active))
    }

    fn begin(&mut self, slot: u8, total_len: u32) -> Step<Resp> {
        let mut p = [0u8; 8];
        XferBegin {
            target: target::APP,
            slot,
            total_len,
            hdr_len: IMAGE_HDR_LEN,
        }
        .encode(&mut p)
        .map_err(|_| detail!("encode"))?;
        self.call(Cmd::XferBegin, &p, timeout::REQUEST_MS)
    }

    fn chunk_max(&self) -> usize {
        self.hello.max_payload as usize - 4
    }

    /// Sends `part` as XFER_HDR or XFER_DATA chunks; the last chunk gets
    /// `last_timeout`. Returns the status of the last chunk sent, stopping at
    /// the first non-OK one.
    fn chunks(&mut self, cmd: Cmd, part: &[u8], last_timeout: u32) -> Step<Status> {
        let max = self.chunk_max();
        let mut buf = [0u8; dagon_proto::MAX_FRAME];
        let n_chunks = part.len().div_ceil(max);
        for (i, data) in part.chunks(max).enumerate() {
            let offset = (i * max) as u32;
            let n = XferChunk { offset, data }
                .encode(&mut buf)
                .map_err(|_| detail!("encode"))?;
            let t = if i + 1 == n_chunks {
                last_timeout
            } else {
                timeout::REQUEST_MS
            };
            let r = self.call(cmd, &buf[..n], t)?;
            if r.status != Status::Ok {
                return Ok(r.status);
            }
            let next = payload::parse_next_offset(r.body())
                .map_err(|_| detail!("{:?}: bad next_offset", cmd))?;
            if next as usize != offset as usize + data.len() {
                return Err(detail!("{:?}: next_offset {} at {}", cmd, next, offset));
            }
        }
        Ok(Status::Ok)
    }

    /// Full transfer of `raw` to `slot`; returns the elapsed ms.
    fn transfer(&mut self, slot: u8, raw: &[u8]) -> Step<u32> {
        let t0 = self.ch.now_ms();
        let (hdr, body) = raw.split_at(HEADER_SIZE);
        let s = self.begin(slot, raw.len() as u32)?.status;
        expect_ok(Cmd::XferBegin, s)?;
        let erase = timeout::erase_ms(self.hello.mcu_id);
        let s = self.chunks(Cmd::XferHdr, hdr, erase)?;
        expect_ok(Cmd::XferHdr, s)?;
        let s = self.chunks(Cmd::XferData, body, timeout::REQUEST_MS)?;
        expect_ok(Cmd::XferData, s)?;
        let r = self.ok(Cmd::XferEnd, &[], timeout::VERIFY_MS)?;
        let digest = payload::parse_digest(r.body()).map_err(|_| detail!("XFER_END: bad body"))?;
        if digest != sha256(body) {
            return Err(detail!("XFER_END: digest mismatch"));
        }
        let ms = (self.ch.now_ms() - t0) as u32;
        let limit = timeout::transfer_ms(raw.len() as u32, self.link);
        if ms > limit {
            return Err(detail!("transfer took {} ms, limit {}", ms, limit));
        }
        Ok(ms)
    }

    fn set_pending(&mut self, slot: u8) -> Step<()> {
        let mut p = [0u8; 2];
        SetPending {
            target: target::APP,
            slot,
        }
        .encode(&mut p)
        .map_err(|_| detail!("encode"))?;
        self.ok(Cmd::SetPending, &p, timeout::VERIFY_MS).map(|_| ())
    }

    /// REBOOT, then polls HELLO until `want` holds. Returns the elapsed ms
    /// and the HELLO that matched.
    fn reboot_until(&mut self, want: impl Fn(&HelloResp) -> bool) -> Step<(u32, HelloResp)> {
        self.ok(Cmd::Reboot, &[reboot_mode::NORMAL], timeout::REQUEST_MS)?;
        let t0 = self.ch.now_ms();
        let mut last: Option<HelloResp> = None;
        loop {
            let ms = (self.ch.now_ms() - t0) as u32;
            if ms > timeout::REBOOT_WAIT_MS {
                return Err(match last {
                    None => detail!("no HELLO {} ms after REBOOT", ms),
                    Some(h) => {
                        detail!("after REBOOT: {} trial {}", h.active_version, h.trial as u8)
                    }
                });
            }
            if let Ok(h) = self.hello() {
                if want(&h) {
                    self.hello = h;
                    return Ok((ms, h));
                }
                last = Some(h);
            }
            self.ch.sleep_ms(timeout::POLL_MS);
        }
    }

    // ---- tests ----------------------------------------------------------

    fn ct04(&mut self) -> Step<u32> {
        let (st, active, _) = self.slots()?;
        let mut targets = [Some(active), None];
        if st.confirmed <= 1 && st.confirmed != active {
            targets[1] = Some(st.confirmed);
        }
        let len = self.good[active as usize].raw.len() as u32;
        let mut rejected = 0;
        for slot in targets.into_iter().flatten() {
            let s = self.begin(slot, len)?.status;
            if s != Status::State {
                return Err(detail!("XFER_BEGIN slot {}: {:?}, want State", slot, s));
            }
            rejected += 1;
        }
        Ok(rejected)
    }

    fn ct20(&mut self) -> Step<u32> {
        let (_, _, idle) = self.slots()?;
        let img = self.good[idle as usize];
        let mut rejected = 0;
        for field in ["mcu_id", "target_tag"] {
            let mut h = img.hdr;
            match field {
                "mcu_id" => h[0x00A] ^= 0x80,
                _ => h[0x050] ^= 0x01,
            }
            let sig = (self.sign)(&h[..SIGNED_LEN]);
            h[SIGNED_LEN..].copy_from_slice(&sig);
            let s = self.begin(idle, img.raw.len() as u32)?.status;
            expect_ok(Cmd::XferBegin, s)?;
            let erase = timeout::erase_ms(self.hello.mcu_id);
            let s = self.chunks(Cmd::XferHdr, &h, erase)?;
            if s != Status::Compat {
                return Err(detail!("wrong {}: {:?}, want Compat", field, s));
            }
            rejected += 1;
        }
        Ok(rejected)
    }

    fn ct09(&mut self) -> Step<u32> {
        for i in 0..HELLO_ROUNDS {
            self.hello()
                .map_err(|d| detail!("HELLO {}: {:?}", i + 1, d))?;
        }
        let (_, _, idle) = self.slots()?;
        let raw = self.good[idle as usize].raw;
        if raw.len() - HEADER_SIZE < self.chunk_max() {
            return Err(detail!(
                "image body below one full XFER_DATA ({} B)",
                self.chunk_max()
            ));
        }
        let ms = self.transfer(idle, raw)?.max(1);
        Ok((raw.len() as u64 * 1000 / ms as u64) as u32)
    }

    fn ct10(&mut self) -> Step<u32> {
        let before = self.hello()?;
        let (ms, h) = self.reboot_until(|_| true)?;
        if h.active_version != before.active_version || h.trial != before.trial {
            return Err(detail!(
                "after REBOOT {} trial {}, before {}",
                h.active_version,
                h.trial as u8,
                before.active_version
            ));
        }
        Ok(ms)
    }

    fn ct11(&mut self) -> Step<u32> {
        self.after_ct11 = None;
        let (st, prev, slot) = self.slots()?;
        let img = self.good[slot as usize];
        self.transfer(slot, img.raw)?;
        self.set_pending(slot)?;
        let v = img.parsed.version;
        let (ms, _) = self.reboot_until(|h| h.active_version == v && h.trial)?;
        let st2 = self.status()?;
        if st2.active != slot || st2.slots[slot as usize].state != SlotState::Trying {
            return Err(detail!(
                "on trial: active {} state {:?}",
                st2.active,
                st2.slots[slot as usize].state
            ));
        }
        self.ok(Cmd::Confirm, &[target::APP], timeout::VERIFY_MS)?;
        let st3 = self.status()?;
        if st3.confirmed != slot || st3.slots[slot as usize].state != SlotState::Confirmed {
            return Err(detail!(
                "after CONFIRM: confirmed {} state {:?}",
                st3.confirmed,
                st3.slots[slot as usize].state
            ));
        }
        self.reboot_until(|h| h.active_version == v && !h.trial)?;
        let exp = Expected {
            active: slot,
            active_version: v,
            active_digest: img.digest16(),
            other_state: SlotState::Written,
            other_version: st.slots[prev as usize].version,
            other_digest: st.slots[prev as usize].digest16,
            last_reason: None,
        };
        let check = self.check_state(&exp);
        self.after_ct11 = Some(check);
        check.map_err(|d| detail!("after reboot: {:?}", d))?;
        Ok(ms)
    }

    fn ct12(&mut self) -> Step<u32> {
        self.after_ct12 = None;
        let (st, prev, slot) = self.slots()?;
        let img = self.no_confirm[slot as usize];
        let prev_version = st.slots[prev as usize].version;
        if img.parsed.version == prev_version {
            return Err(detail!("no_confirm image has the running version"));
        }
        self.transfer(slot, img.raw)?;
        self.set_pending(slot)?;
        let (ms, _) = self.reboot_until(|h| h.active_version == prev_version && !h.trial)?;
        let st2 = self.status()?;
        if st2.active != prev {
            return Err(detail!("rolled back to slot {}, want {}", st2.active, prev));
        }
        if st2.last_reason != reason::TRIAL_EXHAUSTED {
            return Err(detail!(
                "last_reason {}, want TRIAL_EXHAUSTED",
                st2.last_reason
            ));
        }
        if st2.slots[slot as usize].state != SlotState::Reverted {
            return Err(detail!(
                "tried slot {:?}, want Reverted",
                st2.slots[slot as usize].state
            ));
        }
        self.after_ct12 = Some(Expected {
            active: prev,
            active_version: prev_version,
            active_digest: st.slots[prev as usize].digest16,
            other_state: SlotState::Reverted,
            other_version: img.parsed.version,
            other_digest: img.digest16(),
            last_reason: Some(reason::TRIAL_EXHAUSTED),
        });
        Ok(ms)
    }

    fn ct18(&mut self) -> Step<u32> {
        let after11 = match self.after_ct11 {
            Some(Ok(n)) => n,
            Some(Err(d)) => return Err(detail!("after CT-11: {:?}", d)),
            None => return Err(detail!("CT-11 did not complete")),
        };
        let exp = self
            .after_ct12
            .ok_or_else(|| detail!("CT-12 did not complete"))?;
        let after12 = self
            .check_state(&exp)
            .map_err(|d| detail!("after CT-12: {:?}", d))?;
        Ok(after11 + after12)
    }

    /// Compares a fresh HELLO + STATUS with `exp`; returns the field count.
    fn check_state(&mut self, exp: &Expected) -> Step<u32> {
        let h = self.hello()?;
        let st = self.status()?;
        // The good image of the running slot carries this node's identity.
        let id = self.good[exp.active as usize].parsed;
        let other = 1 - exp.active as usize;
        let a = st.slots[exp.active as usize];
        let o = st.slots[other];
        let link_max = link_max_payload(self.link) as u16;
        let checks: [(&str, bool); 17] = [
            (
                "proto_ver",
                h.proto_ver == dagon_proto::PROTO_VERSION as u32,
            ),
            ("role", h.role == role::NODE),
            ("port_kind", h.port_kind == id.port_kind),
            ("mcu_id", h.mcu_id == id.mcu_id),
            ("mode", h.mode == payload::mode::APP),
            ("active_version", h.active_version == exp.active_version),
            ("trial", !h.trial),
            ("key_id", h.key_id == id.key_id),
            ("max_payload", h.max_payload == link_max),
            ("active", st.active == exp.active),
            ("confirmed", st.confirmed == exp.active),
            ("slot state", a.state == SlotState::Confirmed),
            ("slot version", a.version == exp.active_version),
            ("slot digest", a.digest16 == exp.active_digest),
            ("other state", o.state == exp.other_state),
            (
                "other version/digest",
                o.version == exp.other_version && o.digest16 == exp.other_digest,
            ),
            (
                "last_reason",
                exp.last_reason.is_none_or(|r| st.last_reason == r),
            ),
        ];
        match checks.iter().find(|(_, ok)| !ok) {
            Some((name, _)) => Err(detail!("{} mismatch", name)),
            None => Ok(checks.len() as u32),
        }
    }
}

struct Resp {
    buf: [u8; RESP_MAX],
    len: usize,
    status: Status,
}

impl Resp {
    fn body(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

fn expect_ok(cmd: Cmd, s: Status) -> Step<()> {
    if s == Status::Ok {
        Ok(())
    } else {
        Err(detail!("{:?}: {:?}", cmd, s))
    }
}

/// The payload limit an agent on `link` announces in HELLO.
pub const fn link_max_payload(link: LinkKind) -> usize {
    match link {
        LinkKind::I2cTarget => dagon_proto::max_payload::I2C,
        LinkKind::Uart => dagon_proto::max_payload::UART,
        LinkKind::UsbCdc => dagon_proto::max_payload::USB,
    }
}

fn hello_req() -> [u8; 6] {
    let mut p = [0u8; 6];
    let _ = HelloReq {
        proto_ver: dagon_proto::PROTO_VERSION as u32,
        max_payload: dagon_proto::max_payload::USB as u16,
    }
    .encode(&mut p);
    p
}

fn hello_once<C: Channel>(ch: &mut C) -> Result<HelloResp, NoAnswer> {
    let mut buf = [0u8; RESP_MAX];
    let n = ch.request(Cmd::Hello, &hello_req(), timeout::REQUEST_MS, &mut buf)?;
    match split_status(&buf[..n]) {
        Some((Status::Ok, body)) => HelloResp::parse(body).map_err(|_| NoAnswer),
        _ => Err(NoAnswer),
    }
}
