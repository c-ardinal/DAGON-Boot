//! DAGON agent: embedded in an application, it answers HELLO/STATUS/VERSION, receives
//! images into the inactive slot, switches the boot target and confirms.
//!
//! Every state change is a transition of one of the machines declared
//! below, and every transition delivers its event to the app layer's
//! composed chain ([`AppChain`], made by the MCU package's start with
//! `dagon_events::compose`):
//!
//! - [`image`]: the lifecycle of each image slot. It wraps the board's
//!   [`SlotStore`]: the store's mutators take a [`Transition`], which only
//!   this machine makes, so no persistent image state changes without its
//!   transition and event.
//! - [`session`]: the update session, driven by protocol-neutral triggers;
//!   the DAGON wire protocol ([`Agent::handle`]) maps its requests onto them.
//! - [`requests`]: what the agent answered or refused, and the decision
//!   points it asked.
//! - [`node`]: the node's start and its links, reported by the MCU
//!   package's start and serving loop.
//!
//! The layer's publisher folds the events into [`snapshot::AppSnapshot`]
//! (phase, version, progress, trial) for display and log tasks.
//!
//! Applications read the slots through [`Agent::images`] and confirm their
//! own trial with [`Agent::confirm_own`]; extensions answer the decision
//! points (`accept_update`, `allow_reboot_now`, `self_check`) through the
//! chain. The slot store stays the agent's: the MCU package keeps the
//! board's other resources (a flash reader, the watchdog) in handles of
//! its own.
#![no_std]
#![forbid(unsafe_code)]

mod agent;
mod images;
pub mod snapshot;

pub use agent::{reply_status, Agent, AppChain, Reply, IDLE_TIMEOUT_MS};
pub use images::{ImageView, Transition};

use dagon_events::Compact;
use dagon_format::header::HEADER_SIZE;
use dagon_format::Version;
use dagon_proto::payload::{self, StatusResp};

// Links the standard library's critical section on hosted targets.
#[cfg(not(target_os = "none"))]
use critical_section as _;

#[cfg(all(feature = "store-tests", target_os = "none"))]
compile_error!("`store-tests` lets any code change image state; it is for host tests only");

dagon_events::state_machine! {
    /// The lifecycle of one image slot (the two APP slots, and the hub's
    /// topology copy). The slot store's mutators run only inside these
    /// transitions. `trial_started` and `rolled_back` are the boot core's
    /// changes, replayed at start from the slot store.
    pub machine image in App {
        initial Empty;
        states { Empty, Written, Pending, Trial, Confirmed, Reverted }
        transitions {
            /// The slot was erased for a new image (never the running or
            /// the confirmed slot).
            erase(target: u8, slot: u8, len: u32): Empty | Written | Pending | Reverted => Empty
                emits TransferStarted = 0x20 { target: u8, slot: u8, len: u32 }
                observe(live, host);
            /// The body verified and the header written and read back: the
            /// slot holds an image.
            written(target: u8, slot: u8, version: Version): Empty => Written
                emits Written = 0x21 { target: u8, slot: u8, version: Version }
                observe(live, host);
            /// The slot is the one tried on the next boot.
            pending_set(slot: u8, version: Version): Written | Pending | Confirmed | Reverted => Pending
                emits PendingSet = 0x22 { slot: u8, version: Version }
                observe(live, host);
            /// Another slot was set pending; this one no longer is.
            pending_moved(from: u8, to: u8): Pending | Trial => Written
                emits PendingMoved = 0x23 { from: u8, to: u8 }
                observe(live, host);
            /// The boot core started the pending slot on trial (`attempt`
            /// boots so far, of the `max` its boot core allows).
            trial_started(slot: u8, version: Version, attempt: u8, max: u8): Pending | Trial => Trial
                emits TrialStarted = 0x24 { slot: u8, version: Version, attempt: u8, max: u8 }
                observe(replay, host);
            /// The slot on trial was confirmed after its self-check.
            confirmed(slot: u8, version: Version): Trial => Confirmed
                emits Confirmed = 0x25 { slot: u8, version: Version }
                observe(live, host);
            /// The other slot was confirmed; this one is no longer.
            demoted(slot: u8, version: Version): Confirmed => Written
                emits Demoted = 0x26 { slot: u8, version: Version }
                observe(live, host);
            /// The boot core rolled the slot back (attempts exhausted, or
            /// the pending image failed its check) and started `to`.
            rolled_back(from: u8, to: u8, reason: u8): Pending | Trial => Reverted
                emits RolledBack = 0x27 { from: u8, to: u8, reason: u8 }
                observe(replay, host);
            /// The confirmed slot had been set pending again; another slot
            /// was set pending, or the boot core ended its trial: it is the
            /// confirmed slot again.
            pending_withdrawn(slot: u8, version: Version): Pending | Trial => Confirmed
                emits PendingWithdrawn = 0x28 { slot: u8, version: Version }
                observe(live, replay, host);
        }
    }

    /// The lifecycle of one boot core slot of a self-boot MCU with a
    /// stage0 (its definition's `bootcore` slots), kept in the boot core
    /// records of the state log. The agent writes a new core into the idle
    /// slot and sets it pending; stage0 tries it and the core settles the
    /// trial itself: `confirmed` by the new core after its own check,
    /// `rolled_back` by the old one when stage0 gave the new one up. The
    /// app sees those at its next start.
    pub machine core_image in App {
        initial Empty;
        states { Empty, Written, Pending, Confirmed, Reverted }
        transitions {
            /// The idle boot core slot was erased for a new core (never the
            /// running or the confirmed core's slot).
            erase(slot: u8, len: u32): Empty | Written | Pending | Reverted => Empty
                emits CoreTransferStarted = 0x40 { slot: u8, len: u32 }
                observe(live, host);
            /// The core's body verified and its header written and read
            /// back.
            written(slot: u8, version: Version): Empty => Written
                emits CoreWritten = 0x41 { slot: u8, version: Version }
                observe(live, host);
            /// The core is the one stage0 tries at the next reset.
            pending_set(slot: u8, version: Version): Written | Pending | Reverted => Pending
                emits CorePendingSet = 0x42 { slot: u8, version: Version }
                observe(live, host);
            /// The new core passed its own check and confirmed itself.
            confirmed(slot: u8, version: Version): Pending => Confirmed
                emits CoreConfirmed = 0x43 { slot: u8, version: Version }
                observe(host);
            /// Another core confirmed itself; this one is no longer.
            demoted(slot: u8, version: Version): Confirmed => Written
                emits CoreDemoted = 0x44 { slot: u8, version: Version }
                observe(host);
            /// Stage0 gave the pending core up (its starts ran out, or its
            /// image failed the check) and the core in `to` recorded it.
            rolled_back(from: u8, to: u8, reason: u8): Pending => Reverted
                emits CoreRolledBack = 0x45 { from: u8, to: u8, reason: u8 }
                observe(replay, host);
        }
    }

    /// The update session: an offer, the header, the body, the end.
    pub machine session in App {
        initial Idle;
        states {
            Idle,
            Header { target: u8, slot: u8, body_len: u32, got: u16 },
            Body { target: u8, slot: u8, len: u32, next: u32 },
        }
        transitions {
            /// An update passed every DAGON guard; `accept_update` is asked
            /// next.
            update_offered(target: u8, slot: u8, body_len: u32): Idle
                => Header { target, slot, body_len, got: 0 }
                emits UpdateOffered = 0x10 { target: u8, slot: u8, body_len: u32 }
                observe(live, host);
            /// Header bytes received so far.
            header_received(got: u16): Header { target, slot, body_len }
                => Header { target, slot, body_len, got }
                emits HeaderReceived = 0x11 { got: u16 }
                observe(live);
            /// The slot is erased and the header (where the target has one)
            /// accepted: body chunks are taken from here.
            body_opened(): Header { target, slot, body_len }
                => Body { target, slot, len: body_len, next: 0 }
                emits BodyOpened = 0x12 { target: u8, slot: u8, len: u32 = body_len }
                observe(live);
            /// A body chunk was written.
            data_written(end: u32): Body { target, slot, len }
                => Body { target, slot, len, next: end }
                emits TransferProgress = 0x13 { done: u32 = end, total: u32 = len }
                observe(live, host);
            /// The whole body arrived; it is hashed and read back next.
            verify_started(): Body { target, slot, len } => Idle
                emits VerifyStarted = 0x14 { target: u8, slot: u8, len: u32 }
                observe(live);
            /// The transfer was dropped.
            aborted(cause: AbortCause): Header | Body => Idle
                emits TransferAborted = 0x15 { cause: AbortCause }
                observe(live, host);
        }
    }

    /// The node's start and its DAGON links: what the MCU package's start
    /// and serving loop report through the agent.
    pub machine node in App {
        initial Off;
        states { Off, Up }
        transitions {
            /// DAGON's start began (the agent was made, or restarted); the
            /// slot store is read next. `tag`: the node's image target tag
            /// as a name digest (`NameDigest`: the compact form "digest
            /// prefix", made as MCU identities are); a host resolves it
            /// from the topology's node tags.
            starting(tag: NameDigest, role: u8, mode: u8): * => Up
                emits Starting = 0x01 { tag: NameDigest, role: u8, mode: u8 }
                observe(live);
            /// The start took over the boot core's watchdog: its period, and
            /// what the guard does with it while the image is on trial.
            watchdog_taken(period_ms: u32, trial_policy: TrialPolicy): Up => Up
                emits WatchdogTaken = 0x02 { period_ms: u32, trial_policy: TrialPolicy }
                observe(live);
            /// The board's bring-up ran. `board`: the board definition's id
            /// as a name digest (`NameDigest`); a host resolves it from the
            /// board definitions it knows.
            bring_up(board: NameDigest): Up => Up
                emits BringUp = 0x07 { board: NameDigest }
                observe(live);
            /// A DAGON link carries frames: the first one arrived, at
            /// start or after the link went down. `link`: its kind's code
            /// (`dagon_format::with_link_kinds!`); `addr`: the node's own
            /// address on it (`NO_ADDR` where a link has none).
            link_up(link: u8, addr: u8): Up => Up
                emits LinkUp = 0x03 { link: u8, addr: u8 }
                observe(live);
            /// A DAGON link stopped carrying frames: a reply could not be
            /// sent, or the link closed.
            link_down(link: u8, addr: u8): Up => Up
                emits LinkDown = 0x04 { link: u8, addr: u8 }
                observe(live);
            /// A request arrived on link `via`.
            request(protocol: u8, cmd: u8, seq: u8, via: u8): Up => Up
                emits Request = 0x05 { protocol: u8, cmd: u8, seq: u8, via: u8 }
                observe(live);
            /// The reply to the last request left, `ms` after it arrived.
            replied(status: u8, ms: u32): Up => Up
                emits Replied = 0x06 { status: u8, ms: u32 }
                observe(live);
        }
    }

    /// Requests answered or refused, and the decision points asked.
    pub machine requests in App {
        initial Ready;
        states { Ready }
        transitions {
            /// The slot store was read at start.
            store_opened(active: u8, trial: bool, version: Version, attempts: u8, last_reason: u8): Ready => Ready
                emits StoreOpened = 0x30 {
                    active: u8, trial: bool, version: Version, attempts: u8, last_reason: u8
                }
                observe(live);
            /// Identity answered.
            hello(): Ready => Ready emits Hello = 0x31 {} observe(live, host);
            /// Status answered.
            status_asked(): Ready => Ready emits StatusAsked = 0x32 {} observe(live, host);
            /// An update was refused, by a DAGON guard or by an extension's
            /// `accept_update`.
            update_refused(status: u8, by_extension: bool): Ready => Ready
                emits UpdateRefused = 0x33 { status: u8, by_extension: bool }
                observe(live, host);
            /// The header's signature, key and compatibility checked out.
            header_verified(slot: u8, version: Version): Ready => Ready
                emits HeaderVerified = 0x34 { slot: u8, version: Version }
                observe(live, host);
            /// The header was refused (signature, key, compatibility, size).
            header_rejected(status: u8): Ready => Ready
                emits HeaderRejected = 0x35 { status: u8 }
                observe(live, host);
            /// The body read back matches its digest.
            verify_ok(target: u8, slot: u8, len: u32): Ready => Ready
                emits VerifyOk = 0x36 { target: u8, slot: u8, len: u32 }
                observe(live, host);
            /// The body or its header did not verify, or could not be stored.
            verify_failed(status: u8): Ready => Ready
                emits VerifyFailed = 0x37 { status: u8 }
                observe(live, host);
            /// A switch of the boot target was refused.
            pending_refused(status: u8): Ready => Ready
                emits PendingRefused = 0x38 { status: u8 }
                observe(live, host);
            /// A confirm of the slot on trial asks `self_check`.
            self_check(slot: u8, version: Version): Ready => Ready
                emits SelfCheck = 0x39 { slot: u8, version: Version }
                observe(live);
            /// A confirm was refused (not on trial, self-check, store).
            confirm_refused(status: u8): Ready => Ready
                emits ConfirmRefused = 0x3A { status: u8 }
                observe(live, host);
            /// A reboot was accepted, after `allow_reboot_now`.
            reboot_requested(mode: u8, delay_ms: u32): Ready => Ready
                emits RebootRequested = 0x3B { mode: u8, delay_ms: u32 }
                observe(live, host);
            /// The reply has left; the reset follows.
            before_reboot(): Ready => Ready emits BeforeReboot = 0x3C {} observe(live);
        }
    }
}

/// Why a transfer was dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AbortCause {
    /// Another request ended it.
    #[default]
    NewRequest,
    /// No request for [`IDLE_TIMEOUT_MS`].
    Timeout,
    /// The slot store failed.
    Store,
    /// The header was refused.
    HeaderRejected,
    /// An extension refused the update.
    Refused,
}

impl Compact for AbortCause {
    const SIZE: usize = 1;
    fn put(&self, out: &mut [u8]) {
        out[0] = *self as u8;
    }
    fn get(b: &[u8]) -> Option<Self> {
        [
            AbortCause::NewRequest,
            AbortCause::Timeout,
            AbortCause::Store,
            AbortCause::HeaderRejected,
            AbortCause::Refused,
        ]
        .into_iter()
        .find(|c| [*c as u8] == *b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// Erase, program or read-back failed.
    Flash,
    /// Offset or length outside the region.
    Range,
    /// This board does not store that target.
    Unsupported,
}

/// Receiver side of a board: where images go and how the boot target
/// changes. A port (`SlotStore`, trusted only): the board's MCU package
/// provides it.
///
/// `target` is the XFER target (`payload::target::APP`, or
/// `payload::target::TOPOLOGY` with `slot = 0` on the hub); `slot` is 0 = A,
/// 1 = B. Offsets are relative to the start of the image body. The agent never
/// asks to erase or write the running slot or the confirmed slot.
///
/// The mutators take a [`Transition`]: only the agent's image machine calls
/// them, inside the transition that emits the change's event. Methods added
/// later come with default implementations.
pub trait SlotStore {
    /// Current STATUS: kind S folds the state log, kind V maps the vendor
    /// selection data (ESP: otadata).
    fn status(&mut self) -> StatusResp;

    /// Largest body (bytes, header excluded) the region accepts; 0 if the
    /// region does not exist on this board.
    fn capacity(&self, target: u8, slot: u8) -> u32;

    /// Erases what a new body of `len` bytes occupies (rounded up to the erase
    /// unit, not the whole slot) and, for APP, the slot's header place, which
    /// must read back as erased until [`SlotStore::write_header`].
    fn erase(&mut self, target: u8, slot: u8, len: u32, t: &Transition) -> Result<(), StoreError>;

    /// Writes body bytes. Offsets arrive in order without gaps; chunks are
    /// not aligned to the flash write unit or page, so an implementation may
    /// buffer a tail. It must have flushed everything to flash (padding the
    /// last unit with the erased value) before returning from the call whose
    /// `offset + data.len()` equals the `len` given to [`SlotStore::erase`]:
    /// that is the last fragment, and the agent reads the body back right
    /// after it to verify the SHA-256.
    fn write(
        &mut self,
        target: u8,
        slot: u8,
        offset: u32,
        data: &[u8],
        t: &Transition,
    ) -> Result<(), StoreError>;

    /// Reads back body bytes from flash (used for SHA-256 verification).
    fn read(&mut self, target: u8, slot: u8, offset: u32, buf: &mut [u8])
        -> Result<(), StoreError>;

    /// Header placement: stores the verified 512-byte header where this board
    /// keeps it (inline at slot start, trailer, or a detached sector). Called
    /// last, after the body is verified; a slot "has an image" from here on.
    fn write_header(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError>;

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError>;

    /// Boot-target switch: makes `slot` the one tried on the next boot (kind S:
    /// append SLOT_PENDING; kind V: vendor selection). The agent has already
    /// verified the header and the body.
    fn set_pending(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError>;

    /// Confirms the running `slot` (kind S: append CONFIRMED; kind V: mark the
    /// vendor selection valid, read it back, then append CONFIRMED). Only
    /// called while STATUS shows the slot as trying; must tolerate repeats.
    fn confirm(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError>;

    /// Restores the room the state logs keep for trials (a record torn by
    /// a power cut leaves none), compacting now, while the app runs and no
    /// trial watchdog is waiting on the boot stage: the boot stage itself
    /// never erases. Called when the agent starts.
    fn keep_room(&mut self) -> Result<(), StoreError> {
        Ok(())
    }

    /// STATUS of `target`'s slots: `target::APP` is [`SlotStore::status`];
    /// `target::BOOTCORE` the boot core slots (`active` the core that
    /// started this app), folded from the boot core records. `None`: this
    /// board keeps no such slots (its MCU definition declares none).
    fn target_status(&mut self, target: u8) -> Option<StatusResp> {
        (target == payload::target::APP).then(|| self.status())
    }

    /// The version mark of the stage0 this board boots behind
    /// (`dagon_format::stage0::find` over its stage0 region). `None`: no
    /// mark (a stage0 built before it) or no stage0.
    fn stage0_version(&mut self) -> Option<Version> {
        None
    }

    /// [`SlotStore::write_header`] for a slot of `target` (APP, or a
    /// `target_status` target this board keeps).
    fn write_target_header(
        &mut self,
        target: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        match target {
            payload::target::APP => self.write_header(slot, header, t),
            _ => Err(StoreError::Unsupported),
        }
    }

    /// [`SlotStore::read_header`] for a slot of `target`.
    fn read_target_header(
        &mut self,
        target: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        match target {
            payload::target::APP => self.read_header(slot, out),
            _ => Err(StoreError::Unsupported),
        }
    }

    /// [`SlotStore::set_pending`] for a slot of `target` (for BOOTCORE:
    /// append SLOT_PENDING with target BOOTCORE).
    fn set_target_pending(
        &mut self,
        target: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        match target {
            payload::target::APP => self.set_pending(slot, header, t),
            _ => Err(StoreError::Unsupported),
        }
    }
}

/// Identity of the node the agent runs on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Ed25519 public key embedded at build time.
    pub pubkey: [u8; 32],
    /// Image header `mcu_id` this node accepts.
    pub mcu_id: u16,
    /// Image header `port_kind` this node accepts.
    pub port_kind: u8,
    /// Image header `target_tag` this node accepts.
    pub target_tag: [u8; 16],
    /// HELLO `role` (`payload::role`).
    pub role: u8,
    /// HELLO `mode` (`payload::mode`).
    pub mode: u8,
    /// Max payload of the link the agent serves (`dagon_proto::max_payload`).
    pub max_payload: u16,
    /// Boots the boot core gives an image on trial before it rolls back
    /// (the MCU package's boot settings; `TrialInfo::max`).
    pub trial_attempts: u8,
}

/// `app.link_up` / `app.link_down` `addr` of a link without an address.
pub const NO_ADDR: u8 = 0xFF;

/// A name in an event's owned form: the first 8 bytes of the SHA-256 of a
/// domain prefix and the name (as MCU identities are formed). A host maps
/// it back through the names it knows (topology tags, board definitions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NameDigest(pub [u8; 8]);

impl NameDigest {
    /// Domain of a node's image target tag.
    pub const TAG: &'static str = "dagon-tag/";
    /// Domain of a board definition id.
    pub const BOARD: &'static str = "dagon-board/";

    /// The digest of `name` (trailing NULs left out) in `domain`.
    pub fn of(domain: &str, name: &[u8]) -> Self {
        let end = name.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        let mut h = dagon_format::Sha256::new();
        h.update(domain.as_bytes());
        h.update(&name[..end]);
        let d = h.finalize();
        let mut out = [0u8; 8];
        out.copy_from_slice(&d[..8]);
        NameDigest(out)
    }
}

impl Compact for NameDigest {
    const SIZE: usize = 8;
    fn put(&self, out: &mut [u8]) {
        out.copy_from_slice(&self.0);
    }
    fn get(b: &[u8]) -> Option<Self> {
        Some(NameDigest(b.try_into().ok()?))
    }
}

/// What a watchdog guard does while the running image is on trial.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrialPolicy {
    /// Never feeds: an image not confirmed in time is reset and rolled back.
    #[default]
    NeverFeed,
    /// Feeds (gated by liveness) until this many ms after boot, then stops.
    FeedUntilMs(u32),
}

impl Compact for TrialPolicy {
    /// A tag byte (0 never, 1 until), then the ms (0 for never).
    const SIZE: usize = 5;
    fn put(&self, out: &mut [u8]) {
        let (tag, ms) = match *self {
            TrialPolicy::NeverFeed => (0u8, 0u32),
            TrialPolicy::FeedUntilMs(ms) => (1, ms),
        };
        out[0] = tag;
        out[1..].copy_from_slice(&ms.to_le_bytes());
    }
    fn get(b: &[u8]) -> Option<Self> {
        let ms = u32::from_le_bytes(b.get(1..5)?.try_into().ok()?);
        match b.first()? {
            0 => Some(TrialPolicy::NeverFeed),
            1 => Some(TrialPolicy::FeedUntilMs(ms)),
            _ => None,
        }
    }
}

/// `app.request` `protocol` codes.
pub mod protocol {
    /// The DAGON wire protocol (`dagon_proto` frames).
    pub const DAGON: u8 = 1;
}

/// A test bench's slot store, shared with the bench code that plays the
/// boot core or inspects the flash. Host test builds only (feature
/// `store-tests`).
#[cfg(feature = "store-tests")]
impl<S: SlotStore + ?Sized> SlotStore for &core::cell::RefCell<S> {
    fn status(&mut self) -> StatusResp {
        self.borrow_mut().status()
    }
    fn capacity(&self, target: u8, slot: u8) -> u32 {
        self.borrow().capacity(target, slot)
    }
    fn erase(&mut self, target: u8, slot: u8, len: u32, t: &Transition) -> Result<(), StoreError> {
        self.borrow_mut().erase(target, slot, len, t)
    }
    fn write(
        &mut self,
        target: u8,
        slot: u8,
        offset: u32,
        data: &[u8],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().write(target, slot, offset, data, t)
    }
    fn read(
        &mut self,
        target: u8,
        slot: u8,
        offset: u32,
        buf: &mut [u8],
    ) -> Result<(), StoreError> {
        self.borrow_mut().read(target, slot, offset, buf)
    }
    fn write_header(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().write_header(slot, header, t)
    }
    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError> {
        self.borrow_mut().read_header(slot, out)
    }
    fn set_pending(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().set_pending(slot, header, t)
    }
    fn confirm(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().confirm(slot, header, t)
    }
    fn target_status(&mut self, target: u8) -> Option<StatusResp> {
        self.borrow_mut().target_status(target)
    }
    fn keep_room(&mut self) -> Result<(), StoreError> {
        self.borrow_mut().keep_room()
    }
    fn stage0_version(&mut self) -> Option<Version> {
        self.borrow_mut().stage0_version()
    }
    fn write_target_header(
        &mut self,
        target: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut()
            .write_target_header(target, slot, header, t)
    }
    fn read_target_header(
        &mut self,
        target: u8,
        slot: u8,
        out: &mut [u8; HEADER_SIZE],
    ) -> Result<(), StoreError> {
        self.borrow_mut().read_target_header(target, slot, out)
    }
    fn set_target_pending(
        &mut self,
        target: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut()
            .set_target_pending(target, slot, header, t)
    }
}
