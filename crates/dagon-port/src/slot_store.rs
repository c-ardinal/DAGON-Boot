//! The slot store port: where a board keeps its images and how its boot
//! target changes.

use crate::timeout::{EraseLayout, Plan};
use dagon_format::header::HEADER_SIZE;
use dagon_format::Version;
use dagon_proto::payload::{self, Peer, PeerLog, StatusResp};

/// Why a [`SlotStore`] call failed.
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
/// `target` is the XFER target (`payload::target::APP`,
/// `payload::target::BOOTCORE` or `payload::target::AGENT`); `slot` is
/// 0 = A, 1 = B, 2 = factory. Offsets are relative to the start of the
/// image body. The agent never asks to erase or write the slot its
/// writer runs from, nor the confirmed slot of that image kind.
///
/// The mutators take a [`SlotStore::Transition`], the proof that their
/// caller may change image state. The agent takes only a store whose proof
/// is `dagon_agent::Transition`: only the agent's image machine makes one,
/// inside the transition that emits the change's event, so no other code
/// can call them. Methods
/// added later come with default implementations.
pub trait SlotStore {
    /// The proof a mutator's caller holds (the agent's store:
    /// `dagon_agent::Transition`).
    type Transition;

    /// Current STATUS: kind S folds the state log, kind V maps the vendor
    /// selection data (ESP: otadata).
    fn status(&mut self) -> StatusResp;

    /// Largest body (bytes, header excluded) the region accepts; 0 if the
    /// region does not exist on this board.
    fn capacity(&self, target: u8, slot: u8) -> u32;

    /// Erases what a new body of `len` bytes occupies (rounded up to the erase
    /// unit, not the whole slot) and, for APP, the slot's header place, which
    /// must read back as erased until [`SlotStore::write_header`]. Called
    /// only for a store without an [`SlotStore::erase_layout`].
    fn erase(
        &mut self,
        target: u8,
        slot: u8,
        len: u32,
        t: &Self::Transition,
    ) -> Result<(), StoreError>;

    /// The flash this store erases in and the slots it receives into (the
    /// MCU definition's `[execution] erase_regions` and `[[slot_layout]]`,
    /// as the MCU package's build generates them). With one, the agent
    /// plans every erase ([`dagon_port::timeout::EraseLayout::plan`], the
    /// plan a host waits for) and hands it to [`SlotStore::erase_planned`];
    /// a target and slot the layout has no plan for is refused. `None`:
    /// the agent calls [`SlotStore::erase`].
    fn erase_layout(&self) -> Option<EraseLayout> {
        None
    }

    /// Performs exactly the erase operations of `plan` (whole erase units,
    /// in order), for a new body of `len` bytes into `slot` of `target`,
    /// and nothing else; the slot then reads as [`SlotStore::erase`] leaves
    /// it. Without its own implementation: [`SlotStore::erase`].
    fn erase_planned(
        &mut self,
        target: u8,
        slot: u8,
        len: u32,
        plan: Plan<'_>,
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        let _ = plan;
        self.erase(target, slot, len, t)
    }

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
        t: &Self::Transition,
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
        t: &Self::Transition,
    ) -> Result<(), StoreError>;

    fn read_header(&mut self, slot: u8, out: &mut [u8; HEADER_SIZE]) -> Result<(), StoreError>;

    /// Boot-target switch: makes `slot` the one tried on the next boot (kind S:
    /// append SLOT_PENDING; kind V: vendor selection). The agent has already
    /// verified the header and the body.
    fn set_pending(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Self::Transition,
    ) -> Result<(), StoreError>;

    /// Confirms the running `slot` (kind S: append CONFIRMED; kind V: mark the
    /// vendor selection valid, read it back, then append CONFIRMED). Only
    /// called while STATUS shows the slot as trying; must tolerate repeats.
    fn confirm(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Self::Transition,
    ) -> Result<(), StoreError>;

    /// Restores the room the state logs keep for trials (a record torn by
    /// a power cut leaves none), compacting now, while the app runs and no
    /// trial watchdog is waiting on the boot stage: the boot stage itself
    /// never erases. Called when the agent starts.
    fn keep_room(&mut self) -> Result<(), StoreError> {
        Ok(())
    }

    /// The app's hand-over to its boot core's update mode (REBOOT mode
    /// 1): records UPDATE_STAY, after making room for it, for the boot
    /// core's UPDATE_DONE and for the trials
    /// (`dagon_core::log::Log::request_stay`), so the boot core enters
    /// update mode at the next start without erasing. Without its own
    /// implementation: `Unsupported`, a board whose boot core has no
    /// update mode.
    fn request_update_mode(&mut self) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }

    /// STATUS of `target`'s slots: `target::APP` is [`SlotStore::status`];
    /// `target::BOOTCORE` the boot core slots (`active` the core that
    /// started this app), folded from the boot core records. `None`: this
    /// board keeps no such slots (its MCU definition declares none).
    fn target_status(&mut self, target: u8) -> Option<StatusResp> {
        (target == payload::target::APP).then(|| self.status())
    }

    /// Records in the app state log that `peer`, a node this one relays
    /// to, answered (`dagon_core::log::Log::note_peer`): one record for
    /// each peer, kept across resets, and nothing written for a peer
    /// already recorded. Without its own implementation: `Unsupported`, a
    /// board that keeps no such records.
    fn note_peer(&mut self, _peer: Peer) -> Result<(), StoreError> {
        Err(StoreError::Unsupported)
    }

    /// The peers the app state log records ([`SlotStore::note_peer`]).
    /// `None`: the records cannot be read, or this board keeps none.
    fn peers(&mut self) -> Option<PeerLog> {
        None
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
        t: &Self::Transition,
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
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        match target {
            payload::target::APP => self.set_pending(slot, header, t),
            _ => Err(StoreError::Unsupported),
        }
    }
}

/// A slot store shared with other code, such as a test bench that plays
/// the boot core or inspects the flash. Sharing gives that code no way to
/// change image state: each mutator still takes the proof only its caller
/// holds (the agent's machines, for the agent's store).
impl<S: SlotStore + ?Sized> SlotStore for &core::cell::RefCell<S> {
    type Transition = S::Transition;

    fn status(&mut self) -> StatusResp {
        self.borrow_mut().status()
    }
    fn capacity(&self, target: u8, slot: u8) -> u32 {
        self.borrow().capacity(target, slot)
    }
    fn erase(
        &mut self,
        target: u8,
        slot: u8,
        len: u32,
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().erase(target, slot, len, t)
    }
    fn erase_layout(&self) -> Option<EraseLayout> {
        self.borrow().erase_layout()
    }
    fn erase_planned(
        &mut self,
        target: u8,
        slot: u8,
        len: u32,
        plan: Plan<'_>,
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().erase_planned(target, slot, len, plan, t)
    }
    fn write(
        &mut self,
        target: u8,
        slot: u8,
        offset: u32,
        data: &[u8],
        t: &Self::Transition,
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
        t: &Self::Transition,
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
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().set_pending(slot, header, t)
    }
    fn confirm(
        &mut self,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut().confirm(slot, header, t)
    }
    fn target_status(&mut self, target: u8) -> Option<StatusResp> {
        self.borrow_mut().target_status(target)
    }
    fn keep_room(&mut self) -> Result<(), StoreError> {
        self.borrow_mut().keep_room()
    }
    fn request_update_mode(&mut self) -> Result<(), StoreError> {
        self.borrow_mut().request_update_mode()
    }
    fn stage0_version(&mut self) -> Option<Version> {
        self.borrow_mut().stage0_version()
    }
    fn note_peer(&mut self, peer: Peer) -> Result<(), StoreError> {
        self.borrow_mut().note_peer(peer)
    }
    fn peers(&mut self) -> Option<PeerLog> {
        self.borrow_mut().peers()
    }
    fn write_target_header(
        &mut self,
        target: u8,
        slot: u8,
        header: &[u8; HEADER_SIZE],
        t: &Self::Transition,
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
        t: &Self::Transition,
    ) -> Result<(), StoreError> {
        self.borrow_mut()
            .set_target_pending(target, slot, header, t)
    }
}
