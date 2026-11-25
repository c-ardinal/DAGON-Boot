//! PC side: the conformance sequence, driven through the hub.
//!
//! [`Runner::run_all`] runs [`CTS`](crate::CTS) in order against one node.
//! Start with the node running a confirmed conformance build. The run leaves
//! the node running `good` from the other slot, confirmed.
//!
//! | Test | Procedure | Passes when |
//! |---|---|---|
//! | CT-01..03, 07, 08 | CONF_LIST once, then CONF_RUN `test_id` (param 0) and CONF_RESULT every [`timeout::CONF_POLL_MS`] | the run is done with verdict PASS within [`NodeTimings::conf_deadline_s`] |
//! | CT-04 | XFER_BEGIN to the running slot (and the confirmed slot if different) | each answers E_STATE |
//! | CT-20 | header re-signed with another `mcu_id`, then another `target_tag`, sent as XFER_BEGIN + XFER_HDR to the idle slot | the last XFER_HDR answers E_COMPAT, each time |
//! | CT-09 | 20 HELLO, then `good` to the idle slot with full-size XFER_DATA | all OK, XFER_END digest matches, within [`timeout::transfer_ms`] |
//! | CT-10 | REBOOT | HELLO within [`timeout::REBOOT_WAIT_MS`], same version, not on trial |
//! | CT-11 | `good` to the idle slot, SET_PENDING, REBOOT, CONFIRM, REBOOT | new slot on trial, then confirmed, still confirmed after the second reboot |
//! | CT-12 | `no_confirm` to the idle slot, SET_PENDING, REBOOT, never CONFIRM | back on the previous slot within [`timeout::REBOOT_WAIT_MS`], STATUS `last_reason` = TRIAL_EXHAUSTED, tried slot REVERTED |
//! | CT-18 | HELLO + STATUS after CT-11 and after CT-12 | every field matches the state those tests produced |

use crate::report::{CtResult, Report, Verdict};
use crate::{ct_info, detail, CtInfo, Detail, CTS};
use dagon_format::header::{self, ImageHeader, McuId, HEADER_SIZE, SIGNED_LEN};
use dagon_format::statelog::reason;
use dagon_format::{sha256, Version};
use dagon_port::timeout::{self, EraseLayout, EraseRegion, NodeTimings, SlotLayout};
use dagon_port::LinkKind;
use dagon_proto::payload::{
    self, reboot_mode, role, target, ConfListResp, ConfResultResp, ConfRunReq, ConfRunResp,
    HelloReq, HelloResp, SetPending, SlotState, StatusResp, XferBegin, XferChunk, IMAGE_HDR_LEN,
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

/// Images the run sends, one per app slot the node has, index = slot (0 A,
/// 1 B). Each is a full signed image (512-byte header + body); boards with
/// slot-independent images pass the same image for each slot. With one app
/// slot, the tests that write a slot other than the running one are not run.
#[derive(Clone, Copy, Debug)]
pub struct Images<'a> {
    /// A conformance build that confirms normally.
    pub good: &'a [&'a [u8]],
    /// A build that never confirms, as many as `good`. Its version must
    /// differ from `good`.
    pub no_confirm: &'a [&'a [u8]],
}

/// Room for the node's app slots: STATUS's A and B entries.
const ROOM: usize = StatusResp::IMAGE_SLOTS;

/// The tests that write a slot other than the running one: a node with one
/// app slot has none, and they are not run.
const NEED_AN_IDLE_SLOT: [u8; 5] = [9, 11, 12, 18, 20];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupError {
    /// An image is not a parsable header + body of the declared size and
    /// digest for its slot; index into `good` then `no_confirm`.
    BadImage(usize),
    /// `good` and `no_confirm` do not hold one image each per app slot (at
    /// least one, at most STATUS's A and B, as many of each).
    Slots,
    /// `no_confirm` has the same version as `good`.
    SameVersion,
    /// The first HELLO / STATUS did not answer.
    Unreachable,
    /// The first HELLO announced a `max_payload` too small for any XFER chunk.
    BadHello,
    /// The run has no limits for this link kind ([`timeout::link_limits`])
    /// and does not judge it.
    UnsupportedLink(LinkKind),
}

impl core::fmt::Display for SetupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SetupError::BadImage(i) => {
                write!(f, "image {i} is not a valid signed image for its slot")
            }
            SetupError::Slots => f.write_str(
                "give one good and one no-confirm image per app slot of the node (one or two)",
            ),
            SetupError::SameVersion => {
                f.write_str("no-confirm images have the same version as good")
            }
            SetupError::Unreachable => f.write_str("the node did not answer HELLO"),
            SetupError::BadHello => {
                f.write_str("HELLO announced a max_payload too small to transfer")
            }
            SetupError::UnsupportedLink(k) => {
                write!(
                    f,
                    "conform has no limits for link kind {}; it is not judged",
                    k.name()
                )
            }
        }
    }
}

/// Smallest usable HELLO `max_payload`: a chunk's offset and delimiter + 1 data byte.
const MIN_MAX_PAYLOAD: u16 = XferChunk::OVERHEAD as u16 + 1;

/// Room for the longest request without bulk these steps send (XFER_BEGIN, 12 bytes).
const REQ_MAX: usize = 16;

const RESP_MAX: usize = 1 + StatusResp::MAX_LEN;
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
    mcu_id: McuId,
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
    limits: timeout::LinkLimits,
    timings: Timings<'a>,
    /// The node's app slots (the first `slots` entries of the images).
    slots: u8,
    good: [Option<Img<'a>>; ROOM],
    no_confirm: [Option<Img<'a>>; ROOM],
    sign: Signer<'s>,
    hello: HelloResp,
    after_ct11: Option<Result<u32, Detail>>,
    after_ct12: Option<Expected>,
    /// The tests the node named in this run's CONF_LIST, and how many.
    listed: Option<([u8; LISTED_MAX], usize)>,
}

/// The most tests a CONF_LIST answer names.
const LISTED_MAX: usize = dagon_proto::table::conf_list::TESTS.max as usize;

type Step<T> = Result<T, Detail>;

/// The node's own limits, borrowed for the run.
pub type Timings<'a> = NodeTimings<EraseLayout<&'a [EraseRegion], &'a [SlotLayout]>>;

impl<'a, 's, C: Channel> Runner<'a, 's, C> {
    /// Checks the link kind and the images, and greets the node. `timings`
    /// are the node's own limits (its MCU definition's, or
    /// [`NodeTimings::FALLBACK`]).
    pub fn new(
        mut ch: C,
        link: LinkKind,
        images: Images<'a>,
        sign: Signer<'s>,
        timings: Timings<'a>,
    ) -> Result<Self, SetupError> {
        let limits = timeout::link_limits(link).ok_or(SetupError::UnsupportedLink(link))?;
        let n = images.good.len();
        if n == 0 || n > ROOM || images.no_confirm.len() != n {
            return Err(SetupError::Slots);
        }
        let mut good = [None; ROOM];
        let mut no_confirm = [None; ROOM];
        for (i, (s, raw)) in images
            .good
            .iter()
            .enumerate()
            .chain(images.no_confirm.iter().enumerate())
            .enumerate()
        {
            let img = Img::new(raw, s as u8).ok_or(SetupError::BadImage(i))?;
            let set = if i < n { &mut good } else { &mut no_confirm };
            set[s] = Some(img);
        }
        let version = |i: &Option<Img>| i.map(|i| i.parsed.version);
        if (0..n).any(|s| version(&no_confirm[s]) == version(&good[s])) {
            return Err(SetupError::SameVersion);
        }
        let hello = hello_once(&mut ch).map_err(|_| SetupError::Unreachable)?;
        if hello.max_payload < MIN_MAX_PAYLOAD {
            return Err(SetupError::BadHello);
        }
        Ok(Runner {
            ch,
            link,
            limits,
            timings,
            slots: n as u8,
            good,
            no_confirm,
            sign,
            hello,
            after_ct11: None,
            after_ct12: None,
            listed: None,
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
        if self.slots < 2 && NEED_AN_IDLE_SLOT.contains(&id) {
            return CtResult::not_run(id);
        }
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

    /// CONF_LIST, once a run: the node forgets its results and names the
    /// tests it runs.
    fn conf_list(&mut self) -> Step<()> {
        if self.listed.is_none() {
            let r = self.ok(Cmd::ConfList, &[], timeout::REQUEST_MS)?;
            let l = ConfListResp::parse(r.body()).map_err(|_| detail!("CONF_LIST: bad body"))?;
            let mut tests = [0u8; LISTED_MAX];
            let n = l.tests.len().min(LISTED_MAX);
            tests[..n].copy_from_slice(&l.tests[..n]);
            self.listed = Some((tests, n));
        }
        Ok(())
    }

    /// One node-side test: CONF_RUN, which the node answers before it runs
    /// the test, then CONF_RESULT every [`timeout::CONF_POLL_MS`] until the
    /// run is done or its deadline ([`NodeTimings::conf_deadline_s`]) has
    /// passed. A poll the node does not answer (it is running the test) is
    /// not a failure before the deadline.
    fn conf_run(&mut self, id: u8) -> Step<(u32, Detail)> {
        self.conf_list()?;
        let listed = self.listed.is_some_and(|(t, n)| t[..n].contains(&id));
        if !listed {
            return Err(detail!("the node does not list this test"));
        }
        let deadline_s = self
            .timings
            .conf_deadline_s(id, self.limits)
            .ok_or_else(|| detail!("no deadline"))?;
        let mut p = [0u8; REQ_MAX];
        let n = ConfRunReq {
            test_id: id,
            param: 0,
        }
        .encode(&mut p)
        .map_err(|_| detail!("encode"))?;
        let t0 = self.ch.now_ms();
        let r = self.ok(Cmd::ConfRun, &p[..n], timeout::REQUEST_MS)?;
        let taken = ConfRunResp::parse(r.body()).map_err(|_| detail!("CONF_RUN: bad body"))?;
        if taken.verdict != payload::verdict::PENDING || taken.run_id == 0 {
            return Err(detail!("CONF_RUN: verdict {}, no run", taken.verdict));
        }
        let n = payload::encode_conf_result(id, &mut p).map_err(|_| detail!("encode"))?;
        loop {
            if let Ok(r) = self.call(Cmd::ConfResult, &p[..n], timeout::REQUEST_MS) {
                match r.status {
                    Status::Ok => {
                        let c = ConfResultResp::parse(r.body())
                            .map_err(|_| detail!("CONF_RESULT: bad body"))?;
                        if c.run_id != taken.run_id {
                            return Err(detail!("run {} lost (the node reset)", taken.run_id));
                        }
                        if c.state == payload::conf_state::DONE {
                            let d = Detail::from_bytes(c.detail);
                            return match c.verdict {
                                payload::verdict::PASS => Ok((c.measured, d)),
                                payload::verdict::FAIL => Err(d),
                                v => Err(detail!("CONF_RESULT: verdict {}", v)),
                            };
                        }
                    }
                    Status::Timeout => {}
                    s => return Err(detail!("CONF_RESULT: {:?}", s)),
                }
            }
            if self.ch.now_ms() - t0 >= u64::from(deadline_s) * 1000 {
                return Err(detail!("no result in {} s", deadline_s));
            }
            self.ch.sleep_ms(timeout::CONF_POLL_MS);
        }
    }

    /// The good image for `slot`.
    fn good(&self, slot: u8) -> Step<Img<'a>> {
        let img = self.good.get(usize::from(slot)).copied().flatten();
        img.ok_or_else(|| detail!("no image for slot {}", slot))
    }

    /// The no-confirm image for `slot`.
    fn no_confirm(&self, slot: u8) -> Step<Img<'a>> {
        let img = self.no_confirm.get(usize::from(slot)).copied().flatten();
        img.ok_or_else(|| detail!("no image for slot {}", slot))
    }

    /// An app slot of the node other than `slot`.
    fn other_than(&self, slot: u8) -> Step<u8> {
        (0..self.slots)
            .find(|&s| s != slot)
            .ok_or_else(|| detail!("no app slot besides slot {}", slot))
    }

    /// The running slot and another app slot of the node.
    fn slots(&mut self) -> Step<(StatusResp, u8, u8)> {
        let st = self.status()?;
        if st.active >= self.slots {
            return Err(detail!(
                "running slot {} is not one of the node's {} app slots",
                st.active,
                self.slots
            ));
        }
        Ok((st, st.active, self.other_than(st.active)?))
    }

    /// The running slot.
    fn running(&mut self) -> Step<(StatusResp, u8)> {
        let st = self.status()?;
        if st.active >= self.slots {
            return Err(detail!(
                "running slot {} is not one of the node's {} app slots",
                st.active,
                self.slots
            ));
        }
        Ok((st, st.active))
    }

    fn begin(&mut self, slot: u8, total_len: u32) -> Step<Resp> {
        let mut p = [0u8; REQ_MAX];
        let n = XferBegin {
            target: target::APP,
            slot,
            total_len,
            hdr_len: IMAGE_HDR_LEN,
        }
        .encode(&mut p)
        .map_err(|_| detail!("encode"))?;
        self.call(Cmd::XferBegin, &p[..n], timeout::REQUEST_MS)
    }

    /// Data bytes per XFER chunk. Clamped so that a bad HELLO seen later (for
    /// example after a reboot) cannot underflow or overrun the frame buffer.
    fn chunk_max(&self) -> usize {
        (self.hello.max_payload as usize)
            .min(dagon_proto::max_payload::USB)
            .saturating_sub(XferChunk::OVERHEAD)
            .max(1)
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

    /// Limit for the last XFER_HDR of a `body`-byte image to APP `slot`
    /// (the node erases before it answers).
    fn erase_wait_ms(&self, slot: u8, body: usize) -> u32 {
        let body = u32::try_from(body).unwrap_or(u32::MAX);
        self.timings
            .erase_limit_ms(target::APP, slot, IMAGE_HDR_LEN.into(), body)
    }

    /// Full transfer of `raw` to `slot`; returns the elapsed ms.
    fn transfer(&mut self, slot: u8, raw: &[u8]) -> Step<u32> {
        let t0 = self.ch.now_ms();
        let (hdr, body) = raw.split_at(HEADER_SIZE);
        let s = self.begin(slot, raw.len() as u32)?.status;
        expect_ok(Cmd::XferBegin, s)?;
        let erase = self.erase_wait_ms(slot, body.len());
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
        let limit = timeout::transfer_ms(raw.len() as u32, self.limits);
        if ms > limit {
            return Err(detail!("transfer took {} ms, limit {}", ms, limit));
        }
        Ok(ms)
    }

    fn set_pending(&mut self, slot: u8) -> Step<()> {
        let mut p = [0u8; REQ_MAX];
        let n = SetPending {
            target: target::APP,
            slot,
        }
        .encode(&mut p)
        .map_err(|_| detail!("encode"))?;
        self.ok(Cmd::SetPending, &p[..n], timeout::VERIFY_MS)
            .map(|_| ())
    }

    /// REBOOT, then polls HELLO until `want` holds. Returns the elapsed ms
    /// and the HELLO that matched.
    fn reboot_until(&mut self, want: impl Fn(&HelloResp) -> bool) -> Step<(u32, HelloResp)> {
        let mut p = [0u8; REQ_MAX];
        let n = payload::encode_reboot(reboot_mode::NORMAL, &mut p).unwrap_or(0);
        self.ok(Cmd::Reboot, &p[..n], timeout::REQUEST_MS)?;
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
        let (st, active) = self.running()?;
        let mut targets = [Some(active), None];
        if st.confirmed < self.slots && st.confirmed != active {
            targets[1] = Some(st.confirmed);
        }
        let len = self.good(active)?.raw.len() as u32;
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
        let img = self.good(idle)?;
        let mut rejected = 0;
        for field in ["mcu_id", "target_tag"] {
            let mut h = img.hdr;
            match field {
                "mcu_id" => h[0x068] ^= 0x80,
                _ => h[0x050] ^= 0x01,
            }
            let sig = (self.sign)(&h[..SIGNED_LEN]);
            h[SIGNED_LEN..].copy_from_slice(&sig);
            let s = self.begin(idle, img.raw.len() as u32)?.status;
            expect_ok(Cmd::XferBegin, s)?;
            let erase = self.erase_wait_ms(idle, img.raw.len() - HEADER_SIZE);
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
        let raw = self.good(idle)?.raw;
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
        let img = self.good(slot)?;
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
        let mut p = [0u8; REQ_MAX];
        let n = payload::encode_confirm(target::APP, &mut p).unwrap_or(0);
        self.ok(Cmd::Confirm, &p[..n], timeout::VERIFY_MS)?;
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
        let img = self.no_confirm(slot)?;
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
        let id = self.good(exp.active)?.parsed;
        let other = usize::from(self.other_than(exp.active)?);
        let a = st.slots[exp.active as usize];
        let o = st.slots[other];
        let link_max = self.limits.max_payload as u16;
        let checks: [(&str, bool); 17] = [
            (
                "proto_ver",
                h.proto_ver == dagon_proto::PROTO_VERSION as u32,
            ),
            ("role", h.role == role::NODE),
            ("port_kind", h.port_kind == id.port_kind),
            ("mcu_id", h.mcu_id == id.mcu_id.low16()),
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
