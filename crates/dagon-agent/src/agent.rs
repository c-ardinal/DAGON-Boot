//! The DAGON wire protocol mapped onto the agent's machines.

use dagon_events::{
    Accept, Catalog, Clock, Composed, Delay, EventRecord, Extension, Health, Published,
    RebootRequest, Refused, Sink, Snapshot, TrialInfo, Unreadable, UpdateOffer,
};
use dagon_format::header::{self, image_kind, slot_binding, ImageHeader, HEADER_SIZE};
use dagon_format::{FormatError, Sha256};
use dagon_port::{Link, LinkError, Reset};
use dagon_proto::payload::{
    self, reboot_mode, target, HelloReq, HelloResp, SetPending, SlotState, StatusResp, XferBegin,
    XferChunk, IMAGE_HDR_LEN,
};
use dagon_proto::{flags, frame_len, Cmd, Frame, Status, HEADER_LEN, MAX_FRAME};

use crate::images::{ImageStore, ImageView};
use crate::{requests, session, AbortCause, Config, SlotStore};

/// A transfer with no request for this long returns to idle.
pub const IDLE_TIMEOUT_MS: u32 = 10_000;

const RESP_BUF: usize = frame_len(1 + StatusResp::LEN);
const BODY_AT: usize = HEADER_LEN + 1;
const READ_CHUNK: usize = 256;
const SLOT_A: u8 = 0;
const SLOT_B: u8 = 1;

/// The app layer's composed chain, as the agent uses it: where its
/// transitions deliver, the decision points, and replay. Implemented only
/// by `dagon_events::compose`'s chain (its supertrait is sealed), which the
/// MCU package's start makes and hands to [`Agent::new`].
pub trait AppChain: Sink {
    fn accept_update(&mut self, o: &UpdateOffer) -> Accept;
    fn allow_reboot_now(&mut self, r: &RebootRequest) -> Delay;
    fn self_check(&mut self, t: &TrialInfo) -> Health;
    fn replay(&mut self, machines: &[Catalog], rec: &EventRecord) -> Result<(), Unreadable>;
}

impl<P: Snapshot, const N: usize, B: Extension, U: Extension, C: Clock> AppChain
    for Composed<&'static Published<P, N>, B, U, C>
{
    fn accept_update(&mut self, o: &UpdateOffer) -> Accept {
        Composed::accept_update(self, o)
    }
    fn allow_reboot_now(&mut self, r: &RebootRequest) -> Delay {
        Composed::allow_reboot_now(self, r)
    }
    fn self_check(&mut self, t: &TrialInfo) -> Health {
        Composed::self_check(self, t)
    }
    fn replay(&mut self, machines: &[Catalog], rec: &EventRecord) -> Result<(), Unreadable> {
        Composed::replay(self, machines, rec)
    }
}

/// Response to send back, and whether to reboot after sending it.
#[derive(Debug)]
pub struct Reply<'a> {
    pub frame: &'a [u8],
    pub reboot: bool,
    /// With `reboot`: how long to keep serving the watchdog before the
    /// reset (`allow_reboot_now`, capped by the layer); 0 is now.
    pub reboot_delay_ms: u32,
}

pub struct Agent<S, X> {
    cfg: Config,
    key_id: [u8; 8],
    images: ImageStore<S>,
    chain: X,
    session: session::Machine,
    requests: requests::Machine,
    hdr: [u8; HEADER_SIZE],
    last: Option<(u8, u8)>,
    resp: [u8; RESP_BUF],
    resp_len: usize,
    resp_reboot: Option<u32>,
}

type Handled = Result<(usize, Option<u32>), Status>;

/// A notification of [`requests`] leaves from its only state.
fn noted(r: Result<(), Refused>) {
    debug_assert!(r.is_ok(), "{r:?}");
}

impl<S: SlotStore, X: AppChain> Agent<S, X> {
    /// Opens the slot store: `app.store_opened`, then the boot core's
    /// changes it shows replayed (`app.trial_started`, `app.rolled_back`).
    pub fn new(cfg: Config, store: S, chain: X) -> Self {
        let mut agent = Agent {
            key_id: dagon_format::key_id(&cfg.pubkey),
            cfg,
            images: ImageStore::open(store),
            chain,
            session: session::Machine::new(),
            requests: requests::Machine::new(),
            hdr: [0; HEADER_SIZE],
            last: None,
            resp: [0; RESP_BUF],
            resp_len: 0,
            resp_reboot: None,
        };
        agent.opened();
        agent
    }

    /// The app started again over the same store and chain, as after a
    /// reset (a simulator, a test bench): what the agent held in RAM is
    /// gone, the slots are read again, and `app.store_opened` and the boot
    /// replay follow as in [`Agent::new`].
    pub fn restart(&mut self) {
        self.images.reopen();
        self.session = session::Machine::new();
        self.requests = requests::Machine::new();
        self.last = None;
        self.resp_reboot = None;
        self.opened();
    }

    fn opened(&mut self) {
        let view = self.images.view();
        let st = view.status();
        noted(self.requests.store_opened(
            &mut self.chain,
            st.active,
            view.on_trial(),
            view.own_version().unwrap_or_default(),
            st.attempts,
            st.last_reason,
        ));
        self.images.replay_boot(&mut self.chain);
    }

    /// The slots, read-only.
    pub fn images(&self) -> &ImageView {
        self.images.view()
    }

    /// The board's slot store, for its other resources (flash reads, a
    /// watchdog it holds). Its [`SlotStore`] mutators need a
    /// [`crate::Transition`], so they cannot be called through it. What a
    /// store type offers beyond the port (a raw flash handle, a
    /// provisioning helper of a simulator) is outside that guarantee: it
    /// is trusted code of the board or bench, as anything with flash
    /// access in the same image.
    pub fn store(&mut self) -> &mut S {
        self.images.store()
    }

    /// Confirms the running image while it is on trial (an app that
    /// confirms itself at start): `self_check` first, then the `confirmed`
    /// transition; `app.confirm_refused` when it is not on trial or not
    /// healthy.
    pub fn confirm_own(&mut self) -> Result<(), Status> {
        let st = self.images.status();
        if !self.images.view().on_trial() {
            noted(
                self.requests
                    .confirm_refused(&mut self.chain, Status::State as u8),
            );
            return Err(Status::State);
        }
        self.confirm_trial(&st)
    }

    /// Drops any transfer in progress (call after [`IDLE_TIMEOUT_MS`] of silence).
    pub fn abort(&mut self) {
        self.end_session(AbortCause::Timeout);
    }

    /// The reply to a reboot has left: `app.before_reboot`. Call right
    /// before the reset.
    pub fn before_reboot(&mut self) {
        noted(self.requests.before_reboot(&mut self.chain));
    }

    /// Serves frames on `link` forever. On REBOOT it sends the reply, emits
    /// `app.before_reboot` and resets (a delay `allow_reboot_now` asked for
    /// is the caller's to keep: use [`Agent::handle`] in a loop that can
    /// wait).
    pub async fn run<L: Link, R: Reset>(&mut self, link: &mut L, reset: &mut R) {
        let mut buf = [0u8; MAX_FRAME];
        let n = frame_len(self.cfg.max_payload as usize).min(MAX_FRAME);
        loop {
            let len = match link.recv_frame(&mut buf[..n], IDLE_TIMEOUT_MS).await {
                Ok(len) => len,
                Err(LinkError::Timeout) => {
                    self.abort();
                    continue;
                }
                Err(_) => continue,
            };
            let reboot = match self.handle(&buf[..len]) {
                Some(reply) => {
                    let reboot = reply.reboot;
                    // A lost reply is recovered by the sender's retry (same seq).
                    let _ = link.send_frame(reply.frame).await;
                    reboot
                }
                None => false,
            };
            if reboot {
                self.before_reboot();
                reset.reset();
            }
        }
    }

    /// Handles one request frame. `None`: not a valid request (dropped
    /// silently). A repeat of the previous seq and cmd returns the previous
    /// reply without executing again.
    pub fn handle(&mut self, request: &[u8]) -> Option<Reply<'_>> {
        let (frame, _) = Frame::decode(request, self.cfg.max_payload as usize).ok()?;
        if frame.flags & (flags::RESP | flags::EVENT) != 0 {
            return None;
        }
        if self.last != Some((frame.seq, frame.cmd)) {
            let (status, body, reboot) = match self.dispatch(frame.cmd, frame.payload) {
                Ok((body, reboot)) => (Status::Ok, body, reboot),
                Err(s) => (s, 0, None),
            };
            self.resp[HEADER_LEN] = status as u8;
            self.resp_len =
                dagon_proto::finish(&mut self.resp, flags::RESP, frame.seq, frame.cmd, 1 + body)
                    .expect("response fits RESP_BUF");
            self.resp_reboot = reboot;
            self.last = Some((frame.seq, frame.cmd));
        }
        Some(Reply {
            frame: &self.resp[..self.resp_len],
            reboot: self.resp_reboot.is_some(),
            reboot_delay_ms: self.resp_reboot.unwrap_or(0),
        })
    }

    fn dispatch(&mut self, cmd: u8, p: &[u8]) -> Handled {
        match Cmd::from_u8(cmd) {
            Some(Cmd::Hello) => self.hello(p),
            Some(Cmd::Status) => self.status(p),
            Some(Cmd::XferBegin) => self.xfer_begin(XferBegin::parse(p)?),
            Some(Cmd::XferHdr) => self.xfer_hdr(XferChunk::parse(p)?),
            Some(Cmd::XferData) => self.xfer_data(XferChunk::parse(p)?),
            Some(Cmd::XferEnd) => self.xfer_end(p),
            Some(Cmd::SetPending) => self.set_pending(SetPending::parse(p)?),
            Some(Cmd::Reboot) => self.reboot(payload::parse_u8(p)?),
            Some(Cmd::Confirm) => self.confirm(payload::parse_u8(p)?),
            _ => Err(Status::Unsupported),
        }
    }

    fn body(&mut self) -> &mut [u8] {
        &mut self.resp[BODY_AT..]
    }

    /// Ends a transfer in progress, if any.
    fn end_session(&mut self, cause: AbortCause) {
        if self.session.state() != session::State::Idle {
            let ended = self.session.aborted(&mut self.chain, cause);
            debug_assert!(ended.is_ok());
        }
    }

    fn hello(&mut self, p: &[u8]) -> Handled {
        HelloReq::parse(p)?;
        noted(self.requests.hello(&mut self.chain));
        let st = self.images.status();
        let active = st
            .slots
            .get(st.active as usize)
            .copied()
            .unwrap_or_default();
        let resp = HelloResp {
            proto_ver: dagon_proto::PROTO_VERSION as u32,
            role: self.cfg.role,
            port_kind: self.cfg.port_kind,
            mcu_id: self.cfg.mcu_id,
            mode: self.cfg.mode,
            active_version: active.version,
            trial: active.state == SlotState::Trying,
            key_id: self.key_id,
            max_payload: self.cfg.max_payload,
        };
        Ok((resp.encode(self.body()).expect("fits"), None))
    }

    fn status(&mut self, p: &[u8]) -> Handled {
        if !p.is_empty() {
            return Err(Status::Arg);
        }
        noted(self.requests.status_asked(&mut self.chain));
        let st = self.images.status();
        Ok((st.encode(self.body()).expect("fits"), None))
    }

    fn xfer_begin(&mut self, req: XferBegin) -> Handled {
        self.end_session(AbortCause::NewRequest);
        let body_len = match self.offer_guards(&req) {
            Ok(n) => n,
            Err(s) => {
                noted(
                    self.requests
                        .update_refused(&mut self.chain, s as u8, false),
                );
                return Err(s);
            }
        };
        let offered = self
            .session
            .update_offered(&mut self.chain, req.target, req.slot, body_len);
        debug_assert!(offered.is_ok());
        let offer = UpdateOffer::new(req.target, req.slot, body_len);
        if self.chain.accept_update(&offer) == Accept::No {
            self.end_session(AbortCause::Refused);
            noted(
                self.requests
                    .update_refused(&mut self.chain, Status::State as u8, true),
            );
            return Err(Status::State);
        }
        if req.target != target::APP {
            self.open_body(req.target, req.slot, body_len)?;
        }
        Ok((0, None))
    }

    /// DAGON's guards on an update offer; the body length when it passes.
    fn offer_guards(&mut self, req: &XferBegin) -> Result<u32, Status> {
        let body_len = req.body_len().ok_or(Status::Arg)?;
        match req.target {
            target::APP if req.hdr_len == IMAGE_HDR_LEN && matches!(req.slot, SLOT_A | SLOT_B) => {}
            target::TOPOLOGY if req.hdr_len == 0 && req.slot == 0 => {}
            _ => return Err(Status::Arg),
        }
        let cap = self.images.capacity(req.target, req.slot);
        if cap == 0 {
            return Err(Status::Unsupported);
        }
        if body_len > cap {
            return Err(Status::Range);
        }
        if req.target == target::APP {
            let st = self.images.status();
            if req.slot == st.active || req.slot == st.confirmed {
                return Err(Status::State);
            }
        }
        Ok(body_len)
    }

    /// Erases the slot (image `erase`) and opens the session's body.
    fn open_body(&mut self, target: u8, slot: u8, len: u32) -> Result<(), Status> {
        if let Err(s) = self.images.erase(target, slot, len, &mut self.chain) {
            self.end_session(AbortCause::Store);
            return Err(s);
        }
        let opened = self.session.body_opened(&mut self.chain);
        debug_assert!(opened.is_ok());
        Ok(())
    }

    fn xfer_hdr(&mut self, chunk: XferChunk<'_>) -> Handled {
        let session::State::Header {
            slot,
            body_len,
            got,
            ..
        } = self.session.state()
        else {
            return Err(Status::State);
        };
        let end = got as usize + chunk.data.len();
        if chunk.offset != got as u32 || end > HEADER_SIZE {
            return Err(Status::Range);
        }
        self.hdr[got as usize..end].copy_from_slice(chunk.data);
        noted(self.session.header_received(&mut self.chain, end as u16));
        if end == HEADER_SIZE {
            let checked = self
                .check_header(slot)
                .and_then(|h| (h.payload_size == body_len).then_some(h).ok_or(Status::Arg));
            let h = match checked {
                Ok(h) => h,
                Err(s) => {
                    self.end_session(AbortCause::HeaderRejected);
                    noted(self.requests.header_rejected(&mut self.chain, s as u8));
                    return Err(s);
                }
            };
            noted(
                self.requests
                    .header_verified(&mut self.chain, slot, h.version),
            );
            self.open_body(target::APP, slot, body_len)?;
        }
        Ok((put_u32(self.body(), end as u32), None))
    }

    /// Signature, key and compatibility of the header in `self.hdr` for `slot`.
    fn check_header(&self, slot: u8) -> Result<ImageHeader, Status> {
        check_header(&self.cfg, &self.hdr, slot)
    }

    fn xfer_data(&mut self, chunk: XferChunk<'_>) -> Handled {
        let session::State::Body {
            target,
            slot,
            len,
            next,
        } = self.session.state()
        else {
            return Err(Status::State);
        };
        let end = next as u64 + chunk.data.len() as u64;
        if chunk.offset != next || end > len as u64 {
            return Err(Status::Range);
        }
        if let Err(s) = self.images.write_body(target, slot, next, chunk.data) {
            self.end_session(AbortCause::Store);
            return Err(s);
        }
        let end = end as u32;
        noted(self.session.data_written(&mut self.chain, end));
        Ok((put_u32(self.body(), end), None))
    }

    fn xfer_end(&mut self, p: &[u8]) -> Handled {
        if !p.is_empty() {
            return Err(Status::Arg);
        }
        let session::State::Body {
            target,
            slot,
            len,
            next,
        } = self.session.state()
        else {
            return Err(Status::State);
        };
        if next != len {
            return Err(Status::Range);
        }
        noted(self.session.verify_started(&mut self.chain));
        match self.verify_and_commit(target, slot, len) {
            Ok(digest) => {
                self.body()[..32].copy_from_slice(&digest);
                Ok((32, None))
            }
            Err(s) => {
                noted(self.requests.verify_failed(&mut self.chain, s as u8));
                Err(s)
            }
        }
    }

    /// Hashes the body read back; for APP checks it against the held header
    /// and writes the header last (image `written`).
    fn verify_and_commit(&mut self, target: u8, slot: u8, len: u32) -> Result<[u8; 32], Status> {
        let digest = self.hash_body(target, slot, len)?;
        if target != target::APP {
            noted(self.requests.verify_ok(&mut self.chain, target, slot, len));
            self.images.body_written(target, slot, &mut self.chain)?;
            return Ok(digest);
        }
        let h = ImageHeader::parse(&self.hdr).map_err(format_status)?;
        if h.payload_sha256 != digest {
            return Err(Status::Digest);
        }
        noted(self.requests.verify_ok(&mut self.chain, target, slot, len));
        let hdr = self.hdr;
        let cfg = &self.cfg;
        self.images.write_header(
            slot,
            &hdr,
            (h.version, h.payload_size),
            |back| check_header(cfg, back, slot).is_ok(),
            &mut self.chain,
        )?;
        Ok(digest)
    }

    fn hash_body(&mut self, target: u8, slot: u8, len: u32) -> Result<[u8; 32], Status> {
        let mut h = Sha256::new();
        let mut buf = [0u8; READ_CHUNK];
        let mut off = 0u32;
        while off < len {
            let n = (len - off).min(READ_CHUNK as u32) as usize;
            self.images.read(target, slot, off, &mut buf[..n])?;
            h.update(&buf[..n]);
            off += n as u32;
        }
        Ok(h.finalize())
    }

    fn set_pending(&mut self, req: SetPending) -> Handled {
        let r = self.switch(req);
        if let Err(s) = r {
            noted(self.requests.pending_refused(&mut self.chain, s as u8));
        }
        r
    }

    fn switch(&mut self, req: SetPending) -> Handled {
        if req.target != target::APP || !matches!(req.slot, SLOT_A | SLOT_B) {
            return Err(Status::Arg);
        }
        self.end_session(AbortCause::NewRequest);
        if req.slot == self.images.status().active {
            return Err(Status::State);
        }
        self.images.read_header(req.slot, &mut self.hdr)?;
        // An erased or partial header means "no image here".
        ImageHeader::parse(&self.hdr).map_err(|_| Status::State)?;
        let h = self.check_header(req.slot)?;
        if self.hash_body(target::APP, req.slot, h.payload_size)? != h.payload_sha256 {
            return Err(Status::Digest);
        }
        let hdr = self.hdr;
        self.images
            .set_pending(req.slot, &hdr, h.version, &mut self.chain)?;
        Ok((0, None))
    }

    fn reboot(&mut self, mode: u8) -> Handled {
        if mode != reboot_mode::NORMAL {
            return Err(Status::Arg);
        }
        self.end_session(AbortCause::NewRequest);
        let delay = match self.chain.allow_reboot_now(&RebootRequest::new(mode)) {
            Delay::Now => 0,
            Delay::After(ms) => ms,
        };
        noted(self.requests.reboot_requested(&mut self.chain, mode, delay));
        Ok((0, Some(delay)))
    }

    fn confirm(&mut self, t: u8) -> Handled {
        if t != target::APP {
            return Err(Status::Arg);
        }
        let st = self.images.status();
        let active = st.slots.get(st.active as usize).ok_or(Status::State)?;
        if active.state != SlotState::Trying {
            return Ok((0, None));
        }
        self.confirm_trial(&st).map(|()| (0, None))
    }

    /// `self_check`, then the image `confirmed` transition of the running
    /// slot; `app.confirm_refused` on any refusal.
    fn confirm_trial(&mut self, st: &StatusResp) -> Result<(), Status> {
        let r = self.check_and_confirm(st);
        if let Err(s) = r {
            noted(self.requests.confirm_refused(&mut self.chain, s as u8));
        }
        r
    }

    fn check_and_confirm(&mut self, st: &StatusResp) -> Result<(), Status> {
        let slot = st.active;
        let version = st.slots[usize::from(slot)].version;
        noted(self.requests.self_check(&mut self.chain, slot, version));
        let trial = TrialInfo::new(slot, version, st.attempts, 0);
        if self.chain.self_check(&trial) != Health::Healthy {
            return Err(Status::State);
        }
        let mut hdr = [0u8; HEADER_SIZE];
        self.images.read_header(slot, &mut hdr)?;
        self.images.confirm(slot, &hdr, version, &mut self.chain)
    }
}

/// Signature, key and compatibility of `hdr` for `slot`.
fn check_header(cfg: &Config, hdr: &[u8; HEADER_SIZE], slot: u8) -> Result<ImageHeader, Status> {
    let h = header::verify_header(hdr, &cfg.pubkey).map_err(format_status)?;
    let compatible = h.image_kind == image_kind::APP
        && h.mcu_id == cfg.mcu_id
        && h.port_kind == cfg.port_kind
        && h.target_tag == cfg.target_tag
        && (h.slot_binding == slot || h.slot_binding == slot_binding::ANY);
    if !compatible {
        return Err(Status::Compat);
    }
    Ok(h)
}

fn put_u32(out: &mut [u8], v: u32) -> usize {
    out[..4].copy_from_slice(&v.to_le_bytes());
    4
}

fn format_status(e: FormatError) -> Status {
    match e {
        FormatError::UnknownKey => Status::Key,
        FormatError::BadSignature => Status::Signature,
        FormatError::DigestMismatch => Status::Digest,
        FormatError::UnsupportedVersion => Status::Unsupported,
        FormatError::Truncated
        | FormatError::BadMagic
        | FormatError::BadField
        | FormatError::BadCrc => Status::Arg,
    }
}
