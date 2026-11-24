//! DAGON agent: a node's update session, the same in its application and
//! in its boot core. It answers HELLO/STATUS/VERSION, receives images into
//! a slot its writer does not run from, checks them with the verifier it is
//! built with, switches the boot target and confirms.
//!
//! The session's type names the image it runs in ([`Writer`]): what only
//! the app serves is compiled into the app's session alone.
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
mod chain;
mod images;
pub mod snapshot;
pub mod update_mode;

pub use agent::{reply_status, Agent, Reply, IDLE_TIMEOUT_MS};
pub use chain::AppChain;
pub use images::{ImageView, Transition};

use dagon_events::Compact;
use dagon_format::header::{McuId, HEADER_SIZE};
use dagon_format::Version;
use dagon_port::timeout::{EraseLayout, Plan, PlanError};
use dagon_port::StoreError;

// Links the standard library's critical section on hosted targets.
#[cfg(not(target_os = "none"))]
use critical_section as _;

#[cfg(all(feature = "store-tests", target_os = "none"))]
compile_error!("`store-tests` lets any code change image state; it is for host tests only");

dagon_events::state_machine! {
    /// The lifecycle of one image slot (each APP slot the MCU definition
    /// gives). The slot store's mutators run only inside these
    /// transitions. `trial_started` and `rolled_back` are the boot core's
    /// changes, replayed at start from the slot store.
    pub machine image in App {
        initial Empty;
        states { Empty, Written, Pending, Trial, Confirmed, Reverted }
        transitions {
            /// The slot was erased for a new image (never the slot the
            /// writer runs from; the app never erases its confirmed slot
            /// either).
            erase(target: u8, slot: u8, len: u32): Empty | Written | Pending | Trial | Confirmed | Reverted => Empty
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
            /// `version` names the image rolled back: the marker is that
            /// image's (a new image in the slot clears it), so a consumer
            /// tells a new rollback from the one replayed at every start.
            rolled_back(from: u8, to: u8, reason: u8, version: Version): Pending | Trial => Reverted
                emits RolledBack = 0x27 { from: u8, to: u8, reason: u8, version: Version }
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
    /// slot (into the only one when the MCU has one) and sets it pending;
    /// stage0 tries it and the core settles the trial itself: `confirmed`
    /// by the new core after its own check, `rolled_back` by the old one
    /// when stage0 gave the new one up. The app sees those at its next
    /// start.
    pub machine core_image in App {
        initial Empty;
        states { Empty, Written, Pending, Confirmed, Reverted }
        transitions {
            /// The idle boot core slot was erased for a new core; the
            /// running and confirmed core's slot only when it is the MCU's
            /// one boot core slot.
            erase(slot: u8, len: u32): Empty | Written | Pending | Confirmed | Reverted => Empty
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
            /// `version` names the core given up, as in `image`.
            rolled_back(from: u8, to: u8, reason: u8, version: Version): Pending => Reverted
                emits CoreRolledBack = 0x45 { from: u8, to: u8, reason: u8, version: Version }
                observe(replay, host);
        }
    }

    /// The write session: an offer, the header, the body, the end.
    pub machine session in App {
        initial Idle;
        states {
            Idle,
            Header { target: u8, slot: u8, body_len: u32, got: u16 },
            Body { target: u8, slot: u8, len: u32, next: u32 },
        }
        transitions {
            /// A write passed every DAGON guard; `accept_update` is asked
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

    /// A relay's own work (a node whose MCU package runs `dagon-relay`):
    /// its PC link, the requests it relays and the child lines and raw
    /// bus it drives for a ROM recovery. Its own agent's `app.*` events
    /// are emitted as on any node.
    pub machine relay in Relay {
        initial Down;
        states { Down, Up }
        transitions {
            /// The PC link carries frames: the first one arrived, at start
            /// or after the link went down. `link`: its kind's code;
            /// `addr`: the relay's own address on it.
            pc_up(link: u8, addr: u8): * => Up
                emits PcUp = 0x01 { link: u8, addr: u8 }
                observe(live);
            /// A reply to the PC could not be sent: the PC is gone.
            pc_down(link: u8, addr: u8): * => Down
                emits PcDown = 0x02 { link: u8, addr: u8 }
                observe(live);
            /// A RELAY request was admitted and goes to the node at `addr`
            /// over the link `link` (its id); `cmd`: the inner frame's
            /// command.
            relay_request(link: u8, addr: u8, cmd: u8): Up => Up
                emits RelayRequest = 0x03 { link: u8, addr: u8, cmd: u8 }
                observe(live);
            /// SUBCTL drove the reset and boot-mode lines of the recovery
            /// target on link `link` (its id): `nrst` and `boot0` as the
            /// request named them (`payload::nrst`, `payload::boot0`).
            child_control(link: u8, nrst: u8, boot0: u8): Up => Up
                emits ChildControl = 0x04 { link: u8, nrst: u8, boot0: u8 }
                observe(live);
            /// I2C_RAW reached the ROM loader at `addr`: `len` bytes
            /// written and `read_len` asked back.
            child_raw(addr: u8, len: u16, read_len: u16): Up => Up
                emits ChildRaw = 0x05 { addr: u8, len: u16, read_len: u16 }
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
            /// VERSION answered.
            version_asked(): Ready => Ready emits VersionAsked = 0x3E {} observe(live, host);
            /// A write was refused, by a DAGON guard or by an extension's
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
            /// The host told the node its write's one progress number, the
            /// one the host's caller shows for this node.
            percent_told(percent: u8): Ready => Ready
                emits PercentTold = 0x3D { percent: u8 }
                observe(live);
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

/// The erase a new body of `len` bytes into `slot` of `target` takes under
/// `layout`: the shared planner ([`EraseLayout::plan`], the plan a host
/// waits for) with the image header every transfer carries, as XFER_BEGIN
/// requires. A target and slot the layout has no plan for is
/// `Unsupported`; a body larger than the slot, or flash outside the erase
/// regions, `Range`.
pub fn erase_plan<
    'l,
    R: AsRef<[dagon_port::timeout::EraseRegion]>,
    S: AsRef<[dagon_port::timeout::SlotLayout]>,
>(
    layout: &'l EraseLayout<R, S>,
    target: u8,
    slot: u8,
    len: u32,
) -> Result<Plan<'l>, StoreError> {
    layout
        .plan(target, slot, HEADER_SIZE as u32, len)
        .map_err(|e| match e {
            PlanError::NoSlot => StoreError::Unsupported,
            PlanError::TooLong | PlanError::Uncovered { .. } => StoreError::Range,
        })
}

/// Identity of the node the agent runs on, and the image it runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Image header `mcu_id` this node accepts.
    pub mcu_id: McuId,
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

/// The image a session runs in, as the MCU definition's `[writers]` names
/// the kinds that write: the session's type parameter ([`Agent`]).
pub mod writer {
    use dagon_proto::payload::target;

    /// The image a session runs in. A writer never writes the slot it runs
    /// from, nor the one it goes back to (the session's guards). Only the
    /// app's session serves the app's own trial (CONFIRM and its
    /// self-check), replays the boot stage's changes at its start, takes the
    /// recovery agent's target and SHOW, makes log room when it starts and
    /// hands over to the boot core (REBOOT mode 1); the other writers'
    /// sessions are built without them. The boot core's session takes STAY
    /// and makes log room when a write starts.
    pub trait Writer: sealed::Sealed {
        /// The XFER target of the image kind it runs from.
        const TARGET: u8;
        /// Whether it is the app.
        const APP: bool;
        /// The XFER targets it writes: every image kind the MCU
        /// definitions' `[writers]` list it for (checked against them by
        /// a test). Any other target is refused before anything is erased.
        const WRITES: &'static [u8];
    }

    /// The app, running from an APP slot.
    pub struct App;

    /// The boot core, running from a boot core slot.
    pub struct BootCore;

    impl Writer for App {
        const TARGET: u8 = target::APP;
        const APP: bool = true;
        const WRITES: &'static [u8] = &[target::APP, target::BOOTCORE, target::AGENT];
    }

    impl Writer for BootCore {
        const TARGET: u8 = target::BOOTCORE;
        const APP: bool = false;
        const WRITES: &'static [u8] = &[target::APP];
    }

    mod sealed {
        pub trait Sealed {}
        impl Sealed for super::App {}
        impl Sealed for super::BootCore {}
    }
}

pub use writer::Writer;

pub use dagon_port::NO_ADDR;

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
        use dagon_format::{CryptoProvider, Digest};
        let mut h = dagon_format::Software.sha256();
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
