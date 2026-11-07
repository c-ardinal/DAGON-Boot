//! `BootSelectV` over otadata, driven through esp-bootloader-esp-idf's `Ota`.
//!
//! The running slot comes from the MMU (what the 2nd stage actually booted);
//! otadata's selection can differ from it after a rollback.
//!
//! The partition table is only read with raw type/subtype accessors:
//! `PartitionEntry::partition_type()` (and so `OtaUpdater::new`) panics on the
//! custom type 0x40 of the dagon partition.

use core::cell::RefCell;

use dagon_port::{BootSelectError, BootSelectV};
use esp_bootloader_esp_idf::{
    ota::{Ota, OtaImageState},
    partitions::{
        read_partition_table, AppPartitionSubType, FlashStorage, PartitionEntry,
        PARTITION_TABLE_MAX_LEN,
    },
};

use crate::layout::{slot_at, OTADATA, PARTITIONS, SLOT_A, SLOT_B, SLOT_FACTORY};
use crate::otadata;

const OTA_SLOTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitError {
    /// The partition table could not be read or failed its MD5 check.
    PartitionTable,
    /// The flashed table differs from [`crate::layout::PARTITIONS`].
    Layout,
    /// The booted image is not in factory, ota_0 or ota_1.
    BootedPartition,
}

/// otadata as the 2nd stage will see it on the next boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Slot the 2nd stage tries first; `SLOT_FACTORY` when otadata is blank.
    pub slot: u8,
    /// Image state of the selected entry; `None` when otadata is blank.
    pub state: Option<OtaImageState>,
}

pub struct EspBootSelect<'a, 'd> {
    storage: &'a RefCell<FlashStorage<'d>>,
    otadata: PartitionEntry,
    running: u8,
}

fn subtype_of(slot: u8) -> Option<AppPartitionSubType> {
    match slot {
        SLOT_A => Some(AppPartitionSubType::Ota0),
        SLOT_B => Some(AppPartitionSubType::Ota1),
        SLOT_FACTORY => Some(AppPartitionSubType::Factory),
        _ => None,
    }
}

fn slot_of(subtype: AppPartitionSubType) -> u8 {
    match subtype {
        AppPartitionSubType::Ota0 => SLOT_A,
        AppPartitionSubType::Ota1 => SLOT_B,
        _ => SLOT_FACTORY,
    }
}

fn same(e: &PartitionEntry, p: &crate::layout::Partition) -> bool {
    e.raw_type() == p.ptype
        && e.raw_subtype() == p.subtype
        && e.offset() == p.offset
        && e.len() == p.size
}

impl<'a, 'd> EspBootSelect<'a, 'd> {
    /// Checks the flashed partition table against the layout and records the
    /// booted slot.
    pub fn new(storage: &'a RefCell<FlashStorage<'d>>) -> Result<Self, InitError> {
        let mut buf = [0u8; PARTITION_TABLE_MAX_LEN];
        let mut fs = storage.borrow_mut();
        let table =
            read_partition_table(&mut fs, &mut buf).map_err(|_| InitError::PartitionTable)?;
        let mut otadata = None;
        for p in &PARTITIONS {
            let entry = table.iter().find(|e| same(e, p)).ok_or(InitError::Layout)?;
            if *p == OTADATA {
                otadata = Some(entry);
            }
        }
        let booted = table
            .booted_partition()
            .ok()
            .flatten()
            .ok_or(InitError::BootedPartition)?;
        let running = slot_at(booted.offset()).ok_or(InitError::BootedPartition)?;
        drop(fs);
        Ok(Self {
            storage,
            otadata: otadata.ok_or(InitError::Layout)?,
            running,
        })
    }

    fn with_ota<T>(
        &self,
        f: impl FnOnce(&mut Ota<'_, 'd>) -> Result<T, BootSelectError>,
    ) -> Result<T, BootSelectError> {
        let mut fs = self.storage.borrow_mut();
        let mut ota = Ota::new(self.otadata.as_flash_region(&mut fs), OTA_SLOTS)
            .map_err(|_| BootSelectError::Write)?;
        f(&mut ota)
    }

    /// Reads otadata.
    pub fn selection(&self) -> Result<Selection, BootSelectError> {
        self.with_ota(|ota| {
            let slot = slot_of(
                ota.current_app_partition()
                    .map_err(|_| BootSelectError::Write)?,
            );
            // Blank otadata has no current entry: InvalidState.
            let state = ota.current_ota_state().ok();
            Ok(Selection { slot, state })
        })
    }

    /// Both raw otadata entries (sector 0, sector 1).
    pub fn entries(&self) -> Result<[[u8; otadata::ENTRY_LEN]; 2], BootSelectError> {
        let mut fs = self.storage.borrow_mut();
        let mut current = [[0u8; otadata::ENTRY_LEN]; 2];
        for (i, e) in current.iter_mut().enumerate() {
            fs.read(OTADATA.offset + i as u32 * otadata::SECTOR_LEN, e)
                .map_err(|_| BootSelectError::Write)?;
        }
        Ok(current)
    }

    /// Marks the running OTA slot valid (ends its trial) and reads it back.
    /// Nothing to do when running factory, already valid, or booted as the
    /// fallback after otadata's selection was aborted.
    /// The entry is found from the raw entries ([`otadata::confirm_entry`]),
    /// so a corrupt other sector does not block it.
    pub fn confirm(&mut self) -> Result<(), BootSelectError> {
        match otadata::confirm_entry(&self.entries()?, self.running) {
            otadata::ConfirmStep::Done => Ok(()),
            otadata::ConfirmStep::Refused => Err(BootSelectError::Slot),
            otadata::ConfirmStep::Write(sector, entry) => self.write_entry(sector, &entry),
        }
    }

    /// Takes OTA `slot` (not the running one) out of the selection: erases
    /// every counted entry selecting it ([`otadata::selecting`]) and reads
    /// the sector back erased. A cut erase leaves an entry that does not
    /// count, as the 2nd stage reads it.
    pub fn withdraw(&mut self, slot: u8) -> Result<(), BootSelectError> {
        if slot == self.running {
            return Err(BootSelectError::Slot);
        }
        let hit = otadata::selecting(&self.entries()?, slot);
        for sector in (0..hit.len()).filter(|&i| hit[i]) {
            let at = OTADATA.offset + sector as u32 * otadata::SECTOR_LEN;
            let mut fs = self.storage.borrow_mut();
            fs.erase(at, at + otadata::SECTOR_LEN)
                .map_err(|_| BootSelectError::Write)?;
            let mut back = [0u8; otadata::ENTRY_LEN];
            fs.read(at, &mut back).map_err(|_| BootSelectError::Write)?;
            if back != [0xFF; otadata::ENTRY_LEN] {
                return Err(BootSelectError::Write);
            }
        }
        Ok(())
    }

    /// Erases otadata sector `sector`, programs `entry` at its start in one
    /// write and reads it back.
    fn write_entry(
        &mut self,
        sector: usize,
        entry: &[u8; otadata::ENTRY_LEN],
    ) -> Result<(), BootSelectError> {
        let at = OTADATA.offset + sector as u32 * otadata::SECTOR_LEN;
        let mut fs = self.storage.borrow_mut();
        fs.erase(at, at + otadata::SECTOR_LEN)
            .map_err(|_| BootSelectError::Write)?;
        fs.write_nor(at, entry)
            .map_err(|_| BootSelectError::Write)?;
        let mut readback = [0u8; otadata::ENTRY_LEN];
        fs.read(at, &mut readback)
            .map_err(|_| BootSelectError::Write)?;
        if readback != *entry {
            return Err(BootSelectError::Write);
        }
        Ok(())
    }
}

impl BootSelectV for EspBootSelect<'_, '_> {
    fn current_slot(&self) -> u8 {
        self.running
    }

    /// Selects an OTA slot for one trial boot (state New; the 2nd stage turns
    /// it into PendingVerify and aborts it if the app resets unconfirmed).
    /// The new entry is one erase plus one 32-byte program, see
    /// [`otadata::trial_entry`].
    fn set_next(&mut self, slot: u8) -> Result<(), BootSelectError> {
        subtype_of(slot).ok_or(BootSelectError::Slot)?;
        if slot == SLOT_FACTORY {
            return self.select_factory();
        }
        let current = self.entries()?;
        // `None`: the seq space is used up; otadata stays as it is.
        let (sector, entry) =
            otadata::trial_entry(&current, slot, self.running).ok_or(BootSelectError::Write)?;
        self.write_entry(sector, &entry)?;
        // What the 2nd stage will pick. Counted as the 2nd stage does: the
        // crate's reader refuses both entries when the other one is corrupt.
        if otadata::selects_trial(&self.entries()?, sector, slot) {
            Ok(())
        } else {
            Err(BootSelectError::Write)
        }
    }

    /// Blanks both otadata entries, so the 2nd stage boots factory.
    fn select_factory(&mut self) -> Result<(), BootSelectError> {
        self.with_ota(|ota| {
            ota.set_current_app_partition(AppPartitionSubType::Factory)
                .map_err(|_| BootSelectError::Write)?;
            match ota.current_app_partition() {
                Ok(AppPartitionSubType::Factory) => Ok(()),
                _ => Err(BootSelectError::Write),
            }
        })
    }
}
