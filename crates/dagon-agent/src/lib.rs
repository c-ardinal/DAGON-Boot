//! DAGON agent: embedded in an application, it answers HELLO/STATUS, receives
//! images into the inactive slot, switches the boot target and confirms.
//!
//! Board support implements [`SlotStore`]; application authors optionally
//! implement [`Hooks`] (every method has a default). Drive the agent either
//! with [`Agent::run`] over a [`dagon_port::Link`], or by feeding frames to
//! [`Agent::handle`] from your own loop (for example to serve extra commands
//! before falling back to the agent).
#![no_std]
#![forbid(unsafe_code)]

mod agent;

pub use agent::{Agent, Reply, IDLE_TIMEOUT_MS};

use dagon_format::header::HEADER_SIZE;
use dagon_proto::payload::{StatusResp, XferBegin};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// Erase, program or read-back failed.
    Flash,
    /// Offset or length outside the region.
    Range,
    /// This board does not store that target.
    Unsupported,
}

/// Receiver side of a board: where images go and how the boot target changes.
///
/// `target` is the XFER target (`payload::target::APP`, or
/// `payload::target::TOPOLOGY` with `slot = 0` on the hub); `slot` is 0 = A,
/// 1 = B. Offsets are relative to the start of the image body. The agent never
/// asks to erase or write the running slot or the confirmed slot. Methods
/// added after the freeze come with default implementations.
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
    fn erase(&mut self, target: u8, slot: u8, len: u32) -> Result<(), StoreError>;

    /// Writes body bytes. Offsets arrive in order without gaps; chunks are
    /// not aligned to the flash write unit or page, so an implementation may
    /// buffer a tail. It must have flushed everything to flash (padding the
    /// last unit with the erased value) before returning from the call whose
    /// `offset + data.len()` equals the `len` given to [`SlotStore::erase`]:
    /// that is the last fragment, and the agent reads the body back right
    /// after it to verify the SHA-256.
    fn write(&mut self, target: u8, slot: u8, offset: u32, data: &[u8]) -> Result<(), StoreError>;

    /// Reads back body bytes from flash (used for SHA-256 verification).
    fn read(&mut self, target: u8, slot: u8, offset: u32, buf: &mut [u8])
        -> Result<(), StoreError>;

    /// Header placement: stores the verified 512-byte header where this board
    /// keeps it (inline at slot start, trailer, or a detached sector). Called
    /// last, after the body is verified; a slot "has an image" from here on.
    fn write_header(&mut self, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError>;

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError>;

    /// Boot-target switch: makes `slot` the one tried on the next boot (kind S:
    /// append SLOT_PENDING; kind V: vendor selection). The agent has already
    /// verified the header and the body.
    fn set_pending(&mut self, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError>;

    /// Confirms the running `slot` (kind S: append CONFIRMED; kind V: mark the
    /// vendor selection valid, read it back, then append CONFIRMED). Only
    /// called while STATUS shows the slot as trying; must tolerate repeats.
    fn confirm(&mut self, slot: u8, header: &[u8; HEADER_SIZE]) -> Result<(), StoreError>;
}

/// Receive stage reported to [`Hooks::on_progress`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Erase,
    Body,
    Verify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub stage: Stage,
    /// Body bytes done so far.
    pub done: u32,
    /// Body bytes in total.
    pub total: u32,
}

/// Application hooks. Every method has a default, and methods added after
/// the freeze will too, so implementing only what you need keeps compiling.
pub trait Hooks {
    /// Self-check on CONFIRM while on trial. `false` keeps the slot
    /// unconfirmed and answers E_STATE. Default: confirm.
    fn self_check(&mut self) -> bool {
        true
    }

    /// Called on XFER_BEGIN. `false` refuses the update with E_STATE, nothing
    /// is erased. Default: accept.
    fn accept_update(&mut self, _request: &XferBegin) -> bool {
        true
    }

    /// Called at each receive stage. Default: nothing.
    fn on_progress(&mut self, _progress: Progress) {}

    /// Called right before the reboot requested by REBOOT. Keep it shorter
    /// than the trial watchdog. Default: nothing.
    fn before_reboot(&mut self) {}
}

/// Hooks with every default.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultHooks;

impl Hooks for DefaultHooks {}

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
}
