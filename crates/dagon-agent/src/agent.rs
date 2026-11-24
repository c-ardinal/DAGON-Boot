//! The DAGON wire protocol mapped onto the agent's machines.

use dagon_events::{Accept, Delay, Health, RebootRequest, Refused, TrialInfo, UpdateOffer};
use dagon_format::header::{self, image_kind, slot_binding, ImageHeader, HEADER_SIZE};
use dagon_format::{FormatError, Sha256, SigVerifier};
use dagon_proto::handler::{self, Ceiling, Handler};
use dagon_proto::payload::{
    self, mode, phase, reboot_mode, target, CapsReq, CapsResp, CmdSet, HelloReq, HelloResp,
    SetPending, Show, SlotState, StatusResp, VersionResp, VersionSlots, XferBegin, XferChunk,
    IMAGE_HDR_LEN,
};
use dagon_proto::{
    flags, frame_len, max_payload, split_status, Cmd, Frame, FrameError, Status, HEADER_LEN,
};

use crate::chain::AppChain;
use crate::images::{ImageStore, ImageView};
use crate::{
    node, protocol, relay, requests, session, writer, AbortCause, Config, NameDigest, SlotStore,
    TrialPolicy, Writer,
};
use core::marker::PhantomData;

/// A transfer with no request for this long returns to idle.
pub const IDLE_TIMEOUT_MS: u32 = 10_000;

const RESP_BUF: usize = frame_len(1 + StatusResp::MAX_LEN);
// The largest reply body is STATUS's; VERSION's and HELLO's fit the same buffer.
const _: () = assert!(VersionResp::MAX_LEN <= StatusResp::MAX_LEN);
const _: () = assert!(HelloResp::MAX_LEN <= StatusResp::MAX_LEN);
const _: () = assert!(CapsResp::MAX_LEN <= StatusResp::MAX_LEN);
/// The answer to a frame of another wire version: a status and one field.
const REFUSAL_BUF: usize = frame_len(4);
const BODY_AT: usize = HEADER_LEN + 1;
const READ_CHUNK: usize = 256;
const SLOT_A: u8 = 0;
const SLOT_B: u8 = 1;
/// The recovery agent's slot (STATUS slot 2).
const FACTORY: u8 = 2;

/// Response to send back, and whether to reboot after sending it.
#[derive(Debug)]
pub struct Reply<'a> {
    pub frame: &'a [u8],
    pub reboot: bool,
    /// With `reboot`: how long to keep serving the watchdog before the
    /// reset (`allow_reboot_now`, capped by the layer); 0 is now.
    pub reboot_delay_ms: u32,
}

/// What the session keeps of one of the links its requests come over: a
/// requester on one link shares none of it with a requester on another.
struct Requester {
    /// Seq and command of the last request answered, for a repeat.
    last: Option<(u8, u8)>,
    /// The reply to it.
    resp: [u8; RESP_BUF],
    resp_len: usize,
    resp_reboot: Option<u32>,
    /// HELLO `max_payload` of the requester; none before its HELLO.
    peer_max: Option<u16>,
}

impl Requester {
    const NEW: Self = Requester {
        last: None,
        resp: [0; RESP_BUF],
        resp_len: 0,
        resp_reboot: None,
        peer_max: None,
    };
}

/// The update session of writer `W` (the app's by default), for a node
/// whose requests come over `L` links (one by default). The links are
/// numbered from 0 by the node that serves them ([`Agent::handle_on`]).
pub struct Agent<S, X, V, W = writer::App, const L: usize = 1> {
    cfg: Config,
    writer: PhantomData<W>,
    verifier: V,
    images: ImageStore<S>,
    chain: X,
    session: session::Machine,
    requests: requests::Machine,
    node: node::Machine,
    /// The relay's own work; it moves only on a node that relays.
    relay: relay::Machine,
    hdr: [u8; HEADER_SIZE],
    /// One per link.
    links: [Requester; L],
    /// The link of the request being handled.
    on: usize,
    /// The link the transfer in progress was begun over: its chunks and
    /// its end are taken from that link only.
    xfer_link: usize,
    /// The node's clock at its last [`Agent::tick`], and when the link of
    /// the transfer last brought a request ([`IDLE_TIMEOUT_MS`] runs from it).
    now_us: u64,
    xfer_heard_us: u64,
    /// Kept apart from a link's reply, which a repeated request is
    /// answered from.
    refusal: [u8; REFUSAL_BUF],
    /// The commands the node serves in front of this session.
    front: CmdSet,
    /// STATUS `reset_cause`: what reset the node before this start
    /// ([`Agent::note_reset`]; `payload::reset_cause::NONE` until then).
    reset_cause: u8,
}

/// A handled request: the length of the reply's body, and the delay of
/// the reboot that follows the reply.
type Done = (usize, Option<u32>);
type Handled = Result<Done, Status>;

/// A notification of [`requests`] leaves from its only state.
fn noted(r: Result<(), Refused>) {
    debug_assert!(r.is_ok(), "{r:?}");
}

impl<S: SlotStore, X: AppChain, V: SigVerifier> Agent<S, X, V, writer::App> {
    /// The app's session ([`Agent::for_writer`] with the app's writer).
    pub fn new(cfg: Config, verifier: V, store: S, chain: X) -> Self {
        Self::for_writer(cfg, verifier, store, chain)
    }
}

impl<S: SlotStore, X: AppChain, V: SigVerifier, W: Writer> Agent<S, X, V, W> {
    /// The session of writer `W` for a node served over one link
    /// ([`Agent::with_links`]).
    pub fn for_writer(cfg: Config, verifier: V, store: S, chain: X) -> Self {
        Self::with_links(cfg, verifier, store, chain)
    }
}

impl<S: SlotStore, X: AppChain, V: SigVerifier, W: Writer, const L: usize> Agent<S, X, V, W, L> {
    /// The session of writer `W` for a node served over `L` links, named
    /// by the type it is made for. DAGON's start: `app.starting`, then the slot store is opened
    /// (`app.store_opened`), and in the app's session the store makes its
    /// log room and the boot core's changes it shows are replayed
    /// (`app.trial_started`, `app.rolled_back`). `verifier` checks every
    /// image header under the node's key; HELLO reports its key id.
    pub fn with_links(cfg: Config, verifier: V, store: S, mut chain: X) -> Self {
        let mut node = node::Machine::new();
        let tag = NameDigest::of(NameDigest::TAG, &cfg.target_tag);
        noted(node.starting(&mut chain, tag, cfg.role, cfg.mode));
        let mut agent = Agent {
            verifier,
            writer: PhantomData,
            images: ImageStore::open(store, cfg.trial_attempts, W::APP),
            cfg,
            chain,
            session: session::Machine::new(),
            relay: relay::Machine::new(),
            requests: requests::Machine::new(),
            node,
            hdr: [0; HEADER_SIZE],
            links: [Requester::NEW; L],
            on: 0,
            xfer_link: 0,
            now_us: 0,
            xfer_heard_us: 0,
            refusal: [0; REFUSAL_BUF],
            front: CmdSet::EMPTY,
            reset_cause: payload::reset_cause::NONE,
        };
        agent.opened();
        agent
    }

    /// The app started again over the same store and chain, as after a
    /// reset (a simulator, a test bench): what the agent held in RAM is
    /// gone, the slots are read again, and `app.starting`,
    /// `app.store_opened` and the boot replay follow as in [`Agent::new`].
    pub fn restart(&mut self) {
        self.node = node::Machine::new();
        let tag = NameDigest::of(NameDigest::TAG, &self.cfg.target_tag);
        let (role, mode) = (self.cfg.role, self.cfg.mode);
        noted(self.node.starting(&mut self.chain, tag, role, mode));
        self.images.reopen();
        self.session = session::Machine::new();
        self.requests = requests::Machine::new();
        self.links = [Requester::NEW; L];
        self.relay = relay::Machine::new();
        self.reset_cause = payload::reset_cause::NONE;
        self.opened();
    }

    /// What reset the node before this start, as the node read (and
    /// cleared) its reset flags: STATUS reports it from now on.
    pub fn note_reset(&mut self, cause: dagon_port::ResetCause) {
        self.reset_cause = cause.code();
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
        if W::APP {
            self.images.replay_boot(&mut self.chain);
        }
    }

    /// The node answers the commands of `front` itself, before a request
    /// reaches this session (a relay's commands, a ROM entry, a
    /// conformance build's): HELLO and CAPS say so with the session's own.
    pub fn also_serves(&mut self, front: CmdSet) {
        self.front = self.front.union(front);
    }

    /// The slots, read-only.
    pub fn images(&self) -> &ImageView {
        self.images.view()
    }

    /// A boot core slot's lifecycle state (`Empty` on a board that keeps
    /// no boot core slots; `None` for a slot number past B).
    pub fn core_state(&self, slot: u8) -> Option<crate::core_image::State> {
        self.images.core_state(slot)
    }

    /// The start took over the boot core's watchdog with `period_ms` and
    /// the guard's trial `policy` (`app.watchdog_taken`).
    pub fn watchdog_taken(&mut self, period_ms: u32, policy: TrialPolicy) {
        noted(self.node.watchdog_taken(&mut self.chain, period_ms, policy));
    }

    /// The board's bring-up of board `board_id` ran (`app.bring_up`).
    pub fn bring_up(&mut self, board_id: &str) {
        let board = NameDigest::of(NameDigest::BOARD, board_id.as_bytes());
        noted(self.node.bring_up(&mut self.chain, board));
    }

    /// A DAGON link of kind `link` carries frames (`app.link_up`).
    pub fn link_up(&mut self, link: u8, addr: u8) {
        noted(self.node.link_up(&mut self.chain, link, addr));
    }

    /// The node's link 0, of kind `link`, stopped carrying frames
    /// ([`Agent::link_down_on`]).
    pub fn link_down(&mut self, link: u8, addr: u8) {
        self.link_down_on(0, link, addr);
    }

    /// The node's link `on`, of kind `link`, stopped carrying frames
    /// (`app.link_down`). Whoever asks over it next is another requester:
    /// what the session kept of this one (the payload its HELLO named, its
    /// last reply) is dropped, and so is the transfer it began. The other
    /// links keep theirs.
    pub fn link_down_on(&mut self, on: usize, link: u8, addr: u8) {
        if let Some(requester) = self.links.get_mut(on) {
            *requester = Requester::NEW;
        }
        if on == self.xfer_link {
            self.end_session(AbortCause::Timeout);
        }
        noted(self.node.link_down(&mut self.chain, link, addr));
    }

    /// A frame arrived on link `via` (`app.request` when it is a DAGON
    /// request, whoever answers it).
    pub fn request(&mut self, frame: &[u8], via: u8) {
        let Ok((f, _)) = Frame::decode(frame, max_payload::USB) else {
            return;
        };
        if f.flags & (flags::RESP | flags::EVENT) == 0 {
            let p = protocol::DAGON;
            noted(self.node.request(&mut self.chain, p, f.cmd, f.seq, via));
        }
    }

    /// A reply with `status` left, `ms` after its request arrived
    /// (`app.replied`; [`reply_status`] reads it from the frame).
    pub fn replied(&mut self, status: u8, ms: u32) {
        noted(self.node.replied(&mut self.chain, status, ms));
    }

    /// A relay's PC link of kind `link` carries frames (`relay.pc_up`).
    pub fn pc_up(&mut self, link: u8, addr: u8) {
        noted(self.relay.pc_up(&mut self.chain, link, addr));
    }

    /// A relay's PC link of kind `link` is gone (`relay.pc_down`).
    pub fn pc_down(&mut self, link: u8, addr: u8) {
        noted(self.relay.pc_down(&mut self.chain, link, addr));
    }

    /// A relay passes an inner frame of command `cmd` to the node at
    /// `addr` over its link `link` (`relay.relay_request`).
    pub fn relayed(&mut self, link: u8, addr: u8, cmd: u8) {
        noted(self.relay.relay_request(&mut self.chain, link, addr, cmd));
    }

    /// A relay drove the lines of the recovery target on link `link`
    /// (`relay.child_control`).
    pub fn child_control(&mut self, link: u8, nrst: u8, boot0: u8) {
        noted(self.relay.child_control(&mut self.chain, link, nrst, boot0));
    }

    /// A relay reached the ROM loader at `addr` with `len` bytes written
    /// and `read_len` asked back (`relay.child_raw`).
    pub fn child_raw(&mut self, addr: u8, len: u16, read_len: u16) {
        noted(self.relay.child_raw(&mut self.chain, addr, len, read_len));
    }

    /// Confirms the running image while it is on trial (an app that
    /// confirms itself at start): `self_check` first, then the `confirmed`
    /// transition; `app.confirm_refused` when it is not on trial or not
    /// healthy, or when the session runs in another writer than the app.
    pub fn confirm_own(&mut self) -> Result<(), Status> {
        let st = self.images.status();
        if !W::APP || !self.images.view().on_trial() {
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

    /// The node's clock reads `now_us` (microseconds, monotonic): a
    /// transfer whose own link had no request served (answered OK, or a
    /// repeat of one) for [`IDLE_TIMEOUT_MS`] is dropped; refused requests
    /// and requests over the node's other links do not keep it.
    /// A node that serves more than one link calls this at every wake and
    /// before every request it hands to [`Agent::handle_on`]; a node that
    /// never does keeps its own timer and calls [`Agent::abort`].
    pub fn tick(&mut self, now_us: u64) {
        self.now_us = now_us;
        if now_us.saturating_sub(self.xfer_heard_us) >= u64::from(IDLE_TIMEOUT_MS) * 1_000 {
            self.end_session(AbortCause::Timeout);
        }
    }

    /// The reply to a reboot has left: `app.before_reboot`. Call right
    /// before the reset.
    pub fn before_reboot(&mut self) {
        noted(self.requests.before_reboot(&mut self.chain));
    }

    /// Handles one request frame that came over the node's link 0
    /// ([`Agent::handle_on`]).
    pub fn handle(&mut self, request: &[u8]) -> Option<Reply<'_>> {
        self.handle_on(0, request)
    }

    /// Handles one request frame that came over the node's link `on` (0
    /// to `L - 1`, as the node numbers the links it serves). `None`: not a
    /// valid request (dropped silently), or not one of the links. A
    /// request frame of another wire version is refused with an answer
    /// (`dagon_proto::version_refusal`). A repeat of the previous seq and
    /// cmd over the same link returns that link's previous reply without
    /// executing again; the same seq and cmd over another link is another
    /// requester's request.
    pub fn handle_on(&mut self, on: usize, request: &[u8]) -> Option<Reply<'_>> {
        if on >= L {
            return None;
        }
        self.on = on;
        let frame = match Frame::decode(request, self.cfg.max_payload as usize) {
            Ok((frame, _)) => frame,
            Err(FrameError::BadVersion) => {
                let n = dagon_proto::version_refusal(request, &mut self.refusal)?;
                return Some(Reply {
                    frame: self.refusal.get(..n)?,
                    reboot: false,
                    reboot_delay_ms: 0,
                });
            }
            Err(_) => return None,
        };
        if frame.flags & (flags::RESP | flags::EVENT) != 0 {
            return None;
        }
        if self.links[on].last != Some((frame.seq, frame.cmd)) {
            let handled = self.dispatch(frame.cmd, frame.payload);
            let link = &mut self.links[on];
            let (len, reboot) = match handled {
                Ok((body, reboot)) => {
                    link.resp[HEADER_LEN] = Status::Ok as u8;
                    (1 + body, reboot)
                }
                Err(s) => {
                    let out = &mut link.resp[HEADER_LEN..];
                    (payload::put_refusal(out, s, frame.cmd, frame.payload), None)
                }
            };
            link.resp_len =
                dagon_proto::finish(&mut link.resp, flags::RESP, frame.seq, frame.cmd, len)
                    .expect("response fits RESP_BUF");
            link.resp_reboot = reboot;
            link.last = Some((frame.seq, frame.cmd));
        }
        let link = &self.links[on];
        // Only a request served (or its repeat) keeps the transfer going.
        if on == self.xfer_link && link.resp[HEADER_LEN] == Status::Ok as u8 {
            self.xfer_heard_us = self.now_us;
        }
        Some(Reply {
            frame: &link.resp[..link.resp_len],
            reboot: link.resp_reboot.is_some(),
            reboot_delay_ms: link.resp_reboot.unwrap_or(0),
        })
    }

    /// The commands every writer's session serves.
    const COMMON: [Handler<Self, Done>; 12] = [
        Handler::new(Cmd::Hello, |a, r| a.hello(r.hello()?)),
        Handler::new(Cmd::Status, |a, r| a.status(r.status()?)),
        Handler::new(Cmd::Version, |a, _| a.version()),
        Handler::new(Cmd::Caps, |a, r| a.caps(r.caps()?)),
        Handler::new(Cmd::Stay, |a, _| a.stay()),
        Handler::new(Cmd::XferBegin, |a, r| a.xfer_begin(r.xfer_begin()?)),
        Handler::new(Cmd::XferData, |a, r| a.xfer_data(r.xfer_data()?)),
        Handler::new(Cmd::XferEnd, |a, _| a.xfer_end()),
        Handler::new(Cmd::XferHdr, |a, r| a.xfer_hdr(r.xfer_hdr()?)),
        Handler::new(Cmd::SetPending, |a, r| a.set_pending(r.set_pending()?)),
        Handler::new(Cmd::Reboot, |a, r| a.reboot(r.reboot()?)),
        Handler::new(Cmd::Confirm, |a, r| a.confirm(r.confirm()?)),
    ];

    /// The commands only the app's session serves.
    const APP_ONLY: [Handler<Self, Done>; 1] = [Handler::new(Cmd::Show, |a, r| a.show(r.show()?))];

    /// What this writer's session serves: HELLO's `cmds`, less what the
    /// node serves in front of it.
    const CMDS: CmdSet = {
        let common = handler::cmds(&Self::COMMON);
        if W::APP {
            common.union(handler::cmds(&Self::APP_ONLY))
        } else {
            common
        }
    };

    /// What an answer to the requester on the node's link 0 may be at
    /// most ([`Agent::ceiling_on`]).
    pub fn ceiling(&self) -> Ceiling {
        self.ceiling_on(0)
    }

    /// What an answer to the requester on the node's link `on` may be at
    /// most: the payload the session's links carry, and the one that
    /// requester said it takes in its HELLO.
    pub fn ceiling_on(&self, on: usize) -> Ceiling {
        Ceiling {
            link: self.cfg.max_payload,
            peer: self.links.get(on).and_then(|l| l.peer_max),
        }
    }

    /// The command's row makes the typed request of the whole payload and
    /// holds its answer against the ceiling; only then its action runs.
    fn dispatch(&mut self, cmd: u8, p: &[u8]) -> Handled {
        let app_only: &[Handler<Self, Done>] = if W::APP { &Self::APP_ONLY } else { &[] };
        let row = handler::find(&Self::COMMON, cmd).or_else(|| handler::find(app_only, cmd));
        let ceiling = self.ceiling_on(self.on);
        row.ok_or(Status::Unsupported)?.call(self, p, ceiling)
    }

    /// CAPS: the fields of a command the node serves, as the command
    /// table this session is built with has them. A session forwards over
    /// no link, so a tunnel asked of it is E_ARG.
    fn caps(&mut self, asked: CapsReq) -> Handled {
        let served = Self::CMDS.union(self.front);
        let cmd = Cmd::from_u8(asked.cmd)
            .filter(|c| served.has(*c as u8))
            .ok_or(Status::Unsupported)?;
        if asked.link.is_some() {
            return Err(Status::Arg);
        }
        Ok((CapsResp::of(cmd).encode(self.body()).expect("fits"), None))
    }

    /// SHOW to a node: touches no flash and no session, so it is safe
    /// between the chunks of a transfer. Only a transfer's number is for the
    /// node's app; past the bytes its own phase says where it is.
    fn show(&mut self, s: Show<'_>) -> Handled {
        if s.phase == phase::TRANSFER {
            let percent = s.percent.min(100);
            noted(self.requests.percent_told(&mut self.chain, percent));
        }
        Ok((0, None))
    }

    /// Where the body of the reply to the request being handled goes.
    fn body(&mut self) -> &mut [u8] {
        &mut self.links[self.on].resp[BODY_AT..]
    }

    /// Whether the request being handled came over the link the transfer
    /// in progress was begun over. Another link's requester has no part
    /// in it: its chunks and its end are E_STATE.
    fn owns_transfer(&self) -> Result<(), Status> {
        if self.on == self.xfer_link {
            Ok(())
        } else {
            Err(Status::State)
        }
    }

    /// Ends a transfer in progress, if any.
    fn end_session(&mut self, cause: AbortCause) {
        if self.session.state() != session::State::Idle {
            let ended = self.session.aborted(&mut self.chain, cause);
            debug_assert!(ended.is_ok());
        }
    }

    fn hello(&mut self, req: HelloReq) -> Handled {
        self.links[self.on].peer_max = Some(req.max_payload);
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
            key_id: self.verifier.key_id(),
            max_payload: self.cfg.max_payload,
            cmds: Self::CMDS.union(self.front),
        };
        Ok((resp.encode(self.body()).expect("fits"), None))
    }

    fn status(&mut self, t: u8) -> Handled {
        let st = StatusResp {
            reset_cause: self.reset_cause,
            ..self.images.target_status(t).ok_or(Status::Unsupported)?
        };
        noted(self.requests.status_asked(&mut self.chain));
        Ok((st.encode(self.body()).expect("fits"), None))
    }

    /// VERSION's summary set: what this node runs, read from flash. A
    /// value it cannot read (a stage0 without its mark, no boot core
    /// slots) is left out.
    fn version(&mut self) -> Handled {
        let core = self
            .images
            .target_status(target::BOOTCORE)
            .map(|st| VersionSlots::of(&st));
        let resp = VersionResp {
            wire: Some(dagon_proto::PROTO_VERSION),
            mcu_id: Some(self.cfg.mcu_id),
            stage0: self.images.stage0_version(),
            // Only a core the log confirms and its slot holds runs: none
            // while stage0 started the app for want of one.
            boot_core: core.and_then(|c| {
                let running = c.slots.get(usize::from(c.active))?;
                (running.state == SlotState::Confirmed).then_some(running.version)
            }),
            core_slots: core,
            app_slots: Some(VersionSlots::of(&self.images.status())),
        };
        noted(self.requests.version_asked(&mut self.chain));
        Ok((resp.encode(self.body()).expect("fits"), None))
    }

    fn xfer_begin(&mut self, req: XferBegin) -> Handled {
        self.end_session(AbortCause::NewRequest);
        let body_len = match self
            .offer_guards(&req)
            .and_then(|n| self.room_for_write().map(|()| n))
        {
            Ok(n) => n,
            Err(s) => {
                noted(
                    self.requests
                        .update_refused(&mut self.chain, s as u8, false),
                );
                return Err(s);
            }
        };
        self.xfer_link = self.on;
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
        Ok((0, None))
    }

    /// The boot core makes the log's room for what a write records when
    /// the write starts, before anything is erased: the store's shared
    /// reservation, which compacts into the other face and never erases
    /// the only valid one. Its session makes none when it is made, and the
    /// boot stages make none outside update mode. The app's room was made
    /// at its start.
    fn room_for_write(&mut self) -> Result<(), Status> {
        if W::APP {
            return Ok(());
        }
        self.images.keep_room()
    }

    /// Whether this writer writes image kind `t` ([`Writer::WRITES`]); a
    /// target that names no image kind is left to the target checks.
    fn writer_writes(&self, t: u8) -> bool {
        !matches!(t, target::APP | target::BOOTCORE | target::AGENT) || W::WRITES.contains(&t)
    }

    /// Whether this session writes: the app's always, the boot core's only
    /// in update or recovery mode. Outside them the boot core erases and
    /// writes nothing, its log included, and refuses with E_STATE.
    fn writes(&self) -> bool {
        W::APP || matches!(self.cfg.mode, mode::UPDATE | mode::RECOVERY)
    }

    /// DAGON's guards on an update offer; the body length when it passes.
    fn offer_guards(&mut self, req: &XferBegin) -> Result<u32, Status> {
        if !self.writes() {
            return Err(Status::State);
        }
        if !self.writer_writes(req.target) {
            return Err(Status::Unsupported);
        }
        let body_len = req.body_len().ok_or(Status::Arg)?;
        let ab = matches!(req.slot, SLOT_A | SLOT_B);
        let factory = req.slot == FACTORY;
        match req.target {
            target::APP | target::BOOTCORE if req.hdr_len == IMAGE_HDR_LEN && ab => {}
            target::AGENT if W::APP && req.hdr_len == IMAGE_HDR_LEN && factory => {}
            _ => return Err(Status::Arg),
        }
        let cap = self.images.capacity(req.target, req.slot);
        if cap == 0 {
            return Err(Status::Unsupported);
        }
        if body_len > cap {
            return Err(Status::Range);
        }
        if W::APP && req.target == target::AGENT {
            // The recovery agent's slot is rewritten only by a confirmed
            // app running from another slot: that app's agent is the way
            // back if the write is cut, and the factory is then neither
            // running nor the fallback.
            let st = self.images.status();
            let running = st.slots.get(usize::from(st.active)).map(|s| s.state);
            let fallback = req.slot == st.active || req.slot == st.confirmed;
            if fallback || running != Some(SlotState::Confirmed) {
                return Err(Status::State);
            }
        } else {
            let st = self
                .images
                .target_status(req.target)
                .ok_or(Status::Unsupported)?;
            if self.keeps(req.target) && (req.slot == st.active || req.slot == st.confirmed) {
                return Err(Status::State);
            }
            if req.target == target::BOOTCORE && self.trials_overlap(req.target) {
                return Err(Status::State);
            }
        }
        Ok(body_len)
    }

    /// With one boot core slot, a boot core trial and an app trial never
    /// overlap: the app's agent neither writes nor sets pending the only
    /// core while an app is pending or on trial, nor sets an app pending
    /// unless that core is confirmed: not while it is pending, nor while
    /// a cut write left it empty, as no core would then start the app's
    /// trial and the held-back core write could never repair it. The core
    /// is written in place, so stage0
    /// has no other core to fall back to, and it starts the confirmed app
    /// only while no APP trial is open. Each start of a new core that
    /// fails would also start a pending app's trial (the core decides the
    /// app before it settles its own trial), so once stage0 gave the core
    /// up the open app trial would leave nothing to start. The app is
    /// confirmed or rolled back first, or the core settles first. The boot
    /// core's writes are not held back: it writes the app in update mode
    /// because that app does not answer.
    fn trials_overlap(&mut self, t: u8) -> bool {
        if !W::APP || self.images.slot_count(target::BOOTCORE) != 1 {
            return false;
        }
        let open = |st: &StatusResp| {
            st.slots
                .iter()
                .any(|s| matches!(s.state, SlotState::Pending | SlotState::Trying))
        };
        match t {
            target::BOOTCORE => open(&self.images.status()),
            target::APP => self
                .images
                .target_status(target::BOOTCORE)
                .is_some_and(|st| !st.slots.iter().any(|s| s.state == SlotState::Confirmed)),
            _ => false,
        }
    }

    /// Whether the running and the confirmed slot of `t` are kept from
    /// being written (the running one also from being set pending). Kept:
    /// the slots of the image kind the writer runs from, as it runs from
    /// one and goes back to the other; and boot core slots while the MCU
    /// has another one, which takes the new core so the old one stays for
    /// stage0 to fall back to. A writer runs from no slot of any other
    /// kind and writes each of them: the app writes the only boot core
    /// slot in place (stage0 starts the confirmed app when no core may
    /// start), and the boot core any APP slot, the confirmed one too (the
    /// boot core itself receives again when a cut write leaves no app to
    /// start).
    fn keeps(&self, t: u8) -> bool {
        t == W::TARGET || (t == target::BOOTCORE && self.images.slot_count(t) > 1)
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
        self.owns_transfer()?;
        let session::State::Header {
            target,
            slot,
            body_len,
            got,
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
                .check_header(target, slot)
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
            self.open_body(target, slot, body_len)?;
        }
        let n = payload::encode_next_offset(end as u32, self.body());
        Ok((n.expect("fits"), None))
    }

    /// Signature, key and compatibility of the header in `self.hdr` for a
    /// slot of `target`.
    fn check_header(&self, target: u8, slot: u8) -> Result<ImageHeader, Status> {
        check_header((&self.cfg, &self.verifier), &self.hdr, target, slot)
    }

    fn xfer_data(&mut self, chunk: XferChunk<'_>) -> Handled {
        self.owns_transfer()?;
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
        let n = payload::encode_next_offset(end, self.body());
        Ok((n.expect("fits"), None))
    }

    fn xfer_end(&mut self) -> Handled {
        self.owns_transfer()?;
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
                let n = payload::encode_digest(&digest, self.body());
                Ok((n.expect("fits"), None))
            }
            Err(s) => {
                noted(self.requests.verify_failed(&mut self.chain, s as u8));
                Err(s)
            }
        }
    }

    /// Hashes the body read back; for a signed image checks it against the
    /// held header and writes the header last (`written`).
    fn verify_and_commit(&mut self, target: u8, slot: u8, len: u32) -> Result<[u8; 32], Status> {
        let digest = self.hash_body(target, slot, len)?;
        let h = ImageHeader::parse(&self.hdr).map_err(format_status)?;
        if h.payload_sha256 != digest {
            return Err(Status::Digest);
        }
        noted(self.requests.verify_ok(&mut self.chain, target, slot, len));
        let hdr = self.hdr;
        let node = (&self.cfg, &self.verifier);
        self.images.write_header(
            (target, slot),
            &hdr,
            (h.version, h.payload_size),
            |back| check_header(node, back, target, slot).is_ok(),
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
        if !self.writes() {
            return Err(Status::State);
        }
        if !self.writer_writes(req.target) {
            return Err(Status::Unsupported);
        }
        let t = req.target;
        if !matches!(t, target::APP | target::BOOTCORE) || !matches!(req.slot, SLOT_A | SLOT_B) {
            return Err(Status::Arg);
        }
        self.end_session(AbortCause::NewRequest);
        let st = self.images.target_status(t).ok_or(Status::Unsupported)?;
        if (self.keeps(t) && req.slot == st.active) || self.trials_overlap(t) {
            return Err(Status::State);
        }
        self.images.read_header(t, req.slot, &mut self.hdr)?;
        // An erased or partial header means "no image here".
        ImageHeader::parse(&self.hdr).map_err(|_| Status::State)?;
        let h = self.check_header(t, req.slot)?;
        if self.hash_body(t, req.slot, h.payload_size)? != h.payload_sha256 {
            return Err(Status::Digest);
        }
        let hdr = self.hdr;
        if t == target::BOOTCORE {
            self.images
                .set_core_pending(req.slot, &hdr, h.version, &mut self.chain)?;
        } else {
            self.images
                .set_pending(req.slot, &hdr, h.version, &mut self.chain)?;
        }
        Ok((0, None))
    }

    /// STAY: the boot core in update mode stays there (the listen window
    /// that takes the first STAY is the boot core's own loop); a running
    /// app refuses it.
    fn stay(&mut self) -> Handled {
        if W::APP {
            return Err(Status::State);
        }
        Ok((0, None))
    }

    fn reboot(&mut self, mode: u8) -> Handled {
        match mode {
            reboot_mode::NORMAL => {}
            // The app records the hand-over, then resets like mode 0.
            reboot_mode::UPDATE if W::APP => {
                self.end_session(AbortCause::NewRequest);
                self.images.request_update_mode()?;
            }
            // The boot core is in update mode: it stays.
            reboot_mode::UPDATE if self.writes() => return Ok((0, None)),
            reboot_mode::UPDATE => return Err(Status::State),
            _ => return Err(Status::Arg),
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
        // An app confirms the trial it runs; under another writer no app
        // runs, so nothing here may be confirmed.
        if !W::APP {
            let refused = Status::State;
            noted(
                self.requests
                    .confirm_refused(&mut self.chain, refused as u8),
            );
            return Err(refused);
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
        let trial = TrialInfo::new(slot, version, st.attempts, self.cfg.trial_attempts);
        if self.chain.self_check(&trial) != Health::Healthy {
            return Err(Status::State);
        }
        let mut hdr = [0u8; HEADER_SIZE];
        self.images.read_header(target::APP, slot, &mut hdr)?;
        self.images.confirm(slot, &hdr, version, &mut self.chain)
    }
}

/// The status a DAGON reply frame carries; `None` for anything else.
pub fn reply_status(frame: &[u8]) -> Option<u8> {
    let (f, _) = Frame::decode(frame, max_payload::USB).ok()?;
    if f.flags & flags::RESP == 0 {
        return None;
    }
    split_status(f.payload).map(|(s, _)| s as u8)
}

/// The image kind a slot of `target` holds.
fn kind_of(target: u8) -> Option<u8> {
    match target {
        target::APP => Some(image_kind::APP),
        target::BOOTCORE => Some(image_kind::BOOTCORE),
        target::AGENT => Some(image_kind::AGENT),
        _ => None,
    }
}

/// Signature and key of `hdr` under `verifier`, and its compatibility for
/// `slot` of `target`: the kind that target holds, this node's MCU, port
/// and tag, and for a verifying image (boot core, agent) the capability
/// rule.
fn check_header<V: SigVerifier>(
    (cfg, verifier): (&Config, &V),
    hdr: &[u8; HEADER_SIZE],
    target: u8,
    slot: u8,
) -> Result<ImageHeader, Status> {
    let h = header::verify_header(hdr, verifier).map_err(format_status)?;
    let compatible = Some(h.image_kind) == kind_of(target)
        && header::capabilities_ok(h.image_kind, h.build_flags, h.capabilities)
        && h.mcu_id == cfg.mcu_id
        && h.port_kind == cfg.port_kind
        && h.target_tag == cfg.target_tag
        && (h.slot_binding == slot || h.slot_binding == slot_binding::ANY);
    if !compatible {
        return Err(Status::Compat);
    }
    Ok(h)
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
