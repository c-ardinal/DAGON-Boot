use crate::{Config, Hooks, Progress, SlotStore, Stage, StoreError};
use dagon_format::header::{self, image_kind, slot_binding, ImageHeader, HEADER_SIZE};
use dagon_format::{FormatError, Sha256};
use dagon_port::{Link, LinkError, Reset};
use dagon_proto::payload::{
    self, reboot_mode, target, HelloReq, HelloResp, SetPending, SlotState, StatusResp, XferBegin,
    XferChunk, IMAGE_HDR_LEN,
};
use dagon_proto::{flags, frame_len, Cmd, Frame, Status, HEADER_LEN, MAX_FRAME};

/// A transfer with no request for this long returns to idle.
pub const IDLE_TIMEOUT_MS: u32 = 10_000;

const RESP_BUF: usize = frame_len(1 + StatusResp::LEN);
const BODY_AT: usize = HEADER_LEN + 1;
const READ_CHUNK: usize = 256;
const SLOT_A: u8 = 0;
const SLOT_B: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    Idle,
    Header {
        slot: u8,
        body_len: u32,
        got: u16,
    },
    Body {
        target: u8,
        slot: u8,
        len: u32,
        next: u32,
    },
}

/// Response to send back, and whether to reboot after sending it.
#[derive(Debug)]
pub struct Reply<'a> {
    pub frame: &'a [u8],
    pub reboot: bool,
}

pub struct Agent<S, H> {
    cfg: Config,
    key_id: [u8; 8],
    store: S,
    hooks: H,
    session: Session,
    hdr: [u8; HEADER_SIZE],
    last: Option<(u8, u8)>,
    resp: [u8; RESP_BUF],
    resp_len: usize,
    resp_reboot: bool,
}

type Handled = Result<(usize, bool), Status>;

impl<S: SlotStore, H: Hooks> Agent<S, H> {
    pub fn new(cfg: Config, store: S, hooks: H) -> Self {
        Agent {
            key_id: dagon_format::key_id(&cfg.pubkey),
            cfg,
            store,
            hooks,
            session: Session::Idle,
            hdr: [0; HEADER_SIZE],
            last: None,
            resp: [0; RESP_BUF],
            resp_len: 0,
            resp_reboot: false,
        }
    }

    pub fn store(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn hooks(&mut self) -> &mut H {
        &mut self.hooks
    }

    /// Drops any transfer in progress (call after [`IDLE_TIMEOUT_MS`] of silence).
    pub fn abort(&mut self) {
        self.session = Session::Idle;
    }

    /// Serves frames on `link` forever. On REBOOT it sends the reply, runs
    /// [`Hooks::before_reboot`] and resets.
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
                self.hooks.before_reboot();
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
                Err(s) => (s, 0, false),
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
            reboot: self.resp_reboot,
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

    fn hello(&mut self, p: &[u8]) -> Handled {
        HelloReq::parse(p)?;
        let st = self.store.status();
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
        Ok((resp.encode(self.body()).expect("fits"), false))
    }

    fn status(&mut self, p: &[u8]) -> Handled {
        if !p.is_empty() {
            return Err(Status::Arg);
        }
        let st = self.store.status();
        Ok((st.encode(self.body()).expect("fits"), false))
    }

    fn xfer_begin(&mut self, req: XferBegin) -> Handled {
        self.session = Session::Idle;
        let body_len = req.body_len().ok_or(Status::Arg)?;
        match req.target {
            target::APP if req.hdr_len == IMAGE_HDR_LEN && matches!(req.slot, SLOT_A | SLOT_B) => {}
            target::TOPOLOGY if req.hdr_len == 0 && req.slot == 0 => {}
            _ => return Err(Status::Arg),
        }
        let cap = self.store.capacity(req.target, req.slot);
        if cap == 0 {
            return Err(Status::Unsupported);
        }
        if body_len > cap {
            return Err(Status::Range);
        }
        if req.target == target::APP {
            let st = self.store.status();
            if req.slot == st.active || req.slot == st.confirmed {
                return Err(Status::State);
            }
        }
        if !self.hooks.accept_update(&req) {
            return Err(Status::State);
        }
        if req.target == target::APP {
            self.session = Session::Header {
                slot: req.slot,
                body_len,
                got: 0,
            };
        } else {
            self.erase(req.target, req.slot, body_len)?;
        }
        Ok((0, false))
    }

    fn erase(&mut self, target: u8, slot: u8, len: u32) -> Result<(), Status> {
        self.hooks.on_progress(Progress {
            stage: Stage::Erase,
            done: 0,
            total: len,
        });
        self.store.erase(target, slot, len).map_err(store_status)?;
        self.session = Session::Body {
            target,
            slot,
            len,
            next: 0,
        };
        Ok(())
    }

    fn xfer_hdr(&mut self, chunk: XferChunk<'_>) -> Handled {
        let Session::Header {
            slot,
            body_len,
            got,
        } = self.session
        else {
            return Err(Status::State);
        };
        let end = got as usize + chunk.data.len();
        if chunk.offset != got as u32 || end > HEADER_SIZE {
            return Err(Status::Range);
        }
        self.hdr[got as usize..end].copy_from_slice(chunk.data);
        self.session = Session::Header {
            slot,
            body_len,
            got: end as u16,
        };
        if end == HEADER_SIZE {
            self.session = Session::Idle;
            let h = self.check_header(slot)?;
            if h.payload_size != body_len {
                return Err(Status::Arg);
            }
            self.erase(target::APP, slot, body_len)?;
        }
        Ok((put_u32(self.body(), end as u32), false))
    }

    /// Signature, key and compatibility of the header in `self.hdr` for `slot`.
    fn check_header(&self, slot: u8) -> Result<ImageHeader, Status> {
        let h = header::verify_header(&self.hdr, &self.cfg.pubkey).map_err(format_status)?;
        let compatible = h.image_kind == image_kind::APP
            && h.mcu_id == self.cfg.mcu_id
            && h.port_kind == self.cfg.port_kind
            && h.target_tag == self.cfg.target_tag
            && (h.slot_binding == slot || h.slot_binding == slot_binding::ANY);
        if !compatible {
            return Err(Status::Compat);
        }
        Ok(h)
    }

    fn xfer_data(&mut self, chunk: XferChunk<'_>) -> Handled {
        let Session::Body {
            target,
            slot,
            len,
            next,
        } = self.session
        else {
            return Err(Status::State);
        };
        let end = next as u64 + chunk.data.len() as u64;
        if chunk.offset != next || end > len as u64 {
            return Err(Status::Range);
        }
        if let Err(e) = self.store.write(target, slot, next, chunk.data) {
            self.session = Session::Idle;
            return Err(store_status(e));
        }
        let end = end as u32;
        self.session = Session::Body {
            target,
            slot,
            len,
            next: end,
        };
        self.hooks.on_progress(Progress {
            stage: Stage::Body,
            done: end,
            total: len,
        });
        Ok((put_u32(self.body(), end), false))
    }

    fn xfer_end(&mut self, p: &[u8]) -> Handled {
        if !p.is_empty() {
            return Err(Status::Arg);
        }
        let Session::Body {
            target,
            slot,
            len,
            next,
        } = self.session
        else {
            return Err(Status::State);
        };
        if next != len {
            return Err(Status::Range);
        }
        self.session = Session::Idle;
        self.hooks.on_progress(Progress {
            stage: Stage::Verify,
            done: len,
            total: len,
        });
        let digest = self.hash_body(target, slot, len)?;
        if target == target::APP {
            self.commit_header(slot, &digest)?;
        }
        self.body()[..32].copy_from_slice(&digest);
        Ok((32, false))
    }

    /// Writes the held header last, then re-verifies what reads back.
    fn commit_header(&mut self, slot: u8, digest: &[u8; 32]) -> Result<(), Status> {
        let h = ImageHeader::parse(&self.hdr).map_err(format_status)?;
        if h.payload_sha256 != *digest {
            return Err(Status::Digest);
        }
        let hdr = self.hdr;
        self.store.write_header(slot, &hdr).map_err(store_status)?;
        self.store
            .read_header(slot, &mut self.hdr)
            .map_err(store_status)?;
        let ok = self.hdr == hdr && self.check_header(slot).is_ok();
        if !ok {
            return Err(Status::Flash);
        }
        Ok(())
    }

    fn hash_body(&mut self, target: u8, slot: u8, len: u32) -> Result<[u8; 32], Status> {
        let mut h = Sha256::new();
        let mut buf = [0u8; READ_CHUNK];
        let mut off = 0u32;
        while off < len {
            let n = (len - off).min(READ_CHUNK as u32) as usize;
            self.store
                .read(target, slot, off, &mut buf[..n])
                .map_err(store_status)?;
            h.update(&buf[..n]);
            off += n as u32;
        }
        Ok(h.finalize())
    }

    fn set_pending(&mut self, req: SetPending) -> Handled {
        if req.target != target::APP || !matches!(req.slot, SLOT_A | SLOT_B) {
            return Err(Status::Arg);
        }
        self.session = Session::Idle;
        if req.slot == self.store.status().active {
            return Err(Status::State);
        }
        self.store
            .read_header(req.slot, &mut self.hdr)
            .map_err(store_status)?;
        // An erased or partial header means "no image here".
        ImageHeader::parse(&self.hdr).map_err(|_| Status::State)?;
        let h = self.check_header(req.slot)?;
        if self.hash_body(target::APP, req.slot, h.payload_size)? != h.payload_sha256 {
            return Err(Status::Digest);
        }
        let hdr = self.hdr;
        self.store
            .set_pending(req.slot, &hdr)
            .map_err(store_status)?;
        Ok((0, false))
    }

    fn reboot(&mut self, mode: u8) -> Handled {
        if mode != reboot_mode::NORMAL {
            return Err(Status::Arg);
        }
        self.session = Session::Idle;
        Ok((0, true))
    }

    fn confirm(&mut self, t: u8) -> Handled {
        if t != target::APP {
            return Err(Status::Arg);
        }
        let st = self.store.status();
        let active = st.slots.get(st.active as usize).ok_or(Status::State)?;
        if active.state != SlotState::Trying {
            return Ok((0, false));
        }
        if !self.hooks.self_check() {
            return Err(Status::State);
        }
        let mut hdr = [0u8; HEADER_SIZE];
        self.store
            .read_header(st.active, &mut hdr)
            .map_err(store_status)?;
        self.store.confirm(st.active, &hdr).map_err(store_status)?;
        Ok((0, false))
    }
}

fn put_u32(out: &mut [u8], v: u32) -> usize {
    out[..4].copy_from_slice(&v.to_le_bytes());
    4
}

fn store_status(e: StoreError) -> Status {
    match e {
        StoreError::Flash => Status::Flash,
        StoreError::Range => Status::Range,
        StoreError::Unsupported => Status::Unsupported,
    }
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
