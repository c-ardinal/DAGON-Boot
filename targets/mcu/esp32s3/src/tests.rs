//! `EspStore` over a fake flash and a fake otadata that boots like the
//! rollback-enabled 2nd stage.

use crate::layout::{
    region_for, MIN_FLASH_SIZE, OTA_0, OTA_1, REGIONS, SLOT_A, SLOT_B, SLOT_FACTORY,
};
use crate::otadata::{raw, seq_crc, trial_entry, Counted, ENTRY_LEN};
use crate::store::{header_addr, EspStore, Otadata, HEADER_SECTOR};
use dagon_agent::{SlotStore, StoreError, Transition};
use dagon_format::header::{self, image_kind, mcu_id, placement, port_kind, slot_binding};
use dagon_format::header::{ImageHeader, HEADER_SIZE};
use dagon_format::statelog::reason;
use dagon_format::Version;
use dagon_port::{Flash, FlashError, FlashRegion, Watchdog};
use dagon_proto::payload::{target::APP, SlotState};

/// What the agent's image machine hands the store's mutators.
fn t() -> Transition {
    Transition::for_store_tests()
}

/// NOR flash: erase sets 0xFF, a write needs aligned, erased bytes.
pub(crate) struct FakeFlash(pub(crate) Vec<u8>);

impl Flash for FakeFlash {
    fn geometry(&self) -> &[FlashRegion] {
        &REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        region_for(addr, buf.len() as u32).ok_or(FlashError::Range)?;
        buf.copy_from_slice(&self.0[addr as usize..][..buf.len()]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        region_for(addr, len).ok_or(FlashError::Range)?;
        if !addr.is_multiple_of(4096) || !len.is_multiple_of(4096) {
            return Err(FlashError::Align);
        }
        self.0[addr as usize..][..len as usize].fill(0xFF);
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        region_for(addr, data.len() as u32).ok_or(FlashError::Range)?;
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        let dst = &mut self.0[addr as usize..][..data.len()];
        if dst.iter().any(|&b| b != 0xFF) {
            return Err(FlashError::Hw);
        }
        dst.copy_from_slice(data);
        Ok(())
    }
}

/// otadata plus the 2nd stage's choice at reset.
pub(crate) struct FakeOta {
    pub(crate) entries: [[u8; ENTRY_LEN]; 2],
    pub(crate) running: u8,
}

fn set_state(e: &mut [u8; ENTRY_LEN], state: u32) {
    e[24..28].copy_from_slice(&state.to_le_bytes());
}

impl FakeOta {
    fn blank() -> Self {
        Self {
            entries: [[0xFF; ENTRY_LEN]; 2],
            running: SLOT_FACTORY,
        }
    }

    /// Rollback-enabled 2nd stage: highest bootable seq; New becomes
    /// PendingVerify (trial), PendingVerify left over becomes Aborted and the
    /// next one is tried; nothing bootable boots factory.
    pub(crate) fn boot(&mut self) {
        loop {
            let pick = (0..2)
                .filter_map(|i| Counted::of(&self.entries[i]).map(|c| (i, c)))
                .filter(|(_, c)| c.bootable())
                .max_by_key(|(_, c)| c.seq);
            let Some((i, c)) = pick else {
                self.running = SLOT_FACTORY;
                return;
            };
            match c.state {
                raw::NEW => set_state(&mut self.entries[i], raw::PENDING_VERIFY),
                raw::PENDING_VERIFY => {
                    set_state(&mut self.entries[i], raw::ABORTED);
                    continue;
                }
                _ => {}
            }
            self.running = c.slot;
            return;
        }
    }
}

impl Otadata for FakeOta {
    fn running(&self) -> u8 {
        self.running
    }
    fn entries(&self) -> Result<[[u8; ENTRY_LEN]; 2], StoreError> {
        Ok(self.entries)
    }
    fn set_next(&mut self, slot: u8) -> Result<(), StoreError> {
        let (sector, e) =
            trial_entry(&self.entries, slot, self.running).ok_or(StoreError::Flash)?;
        self.entries[sector] = e;
        Ok(())
    }
    fn confirm(&mut self) -> Result<(), StoreError> {
        let i = (0..2)
            .filter_map(|i| Counted::of(&self.entries[i]).map(|c| (i, c)))
            .max_by_key(|(_, c)| c.seq)
            .filter(|(_, c)| c.slot == self.running)
            .ok_or(StoreError::Flash)?
            .0;
        set_state(&mut self.entries[i], raw::VALID);
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Feeds(u32);

impl Watchdog for Feeds {
    fn start(&mut self, _: u32) {}
    fn feed(&mut self) {
        self.0 += 1;
    }
}

pub(crate) type Store = EspStore<FakeFlash, FakeOta, Feeds>;

pub(crate) fn store() -> Store {
    EspStore::new(
        FakeFlash(vec![0xFF; MIN_FLASH_SIZE as usize]),
        FakeOta::blank(),
        Feeds::default(),
    )
}

fn header(minor: u8, len: u32) -> [u8; HEADER_SIZE] {
    ImageHeader {
        image_kind: image_kind::APP,
        port_kind: port_kind::VENDOR_STAGE2,
        mcu_id: mcu_id::ESP32S3,
        slot_binding: slot_binding::ANY,
        placement: placement::DETACHED,
        load_addr: OTA_0.offset,
        payload_size: len,
        version: Version::new(1, minor, 0),
        build_flags: 0,
        release_seq: 0,
        min_bootcore_version: Version::default(),
        payload_sha256: [0; 32],
        key_id: [0; 8],
        target_tag: *b"watch-s3\0\0\0\0\0\0\0\0",
        proto_version: 1,
        capabilities: 0,
        signature: [0; 64],
    }
    .to_bytes()
}

fn body(len: u32, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ seed)
        .collect()
}

/// What the agent does for one image: erase, odd-sized chunks, read-back,
/// header last.
fn receive(s: &mut Store, slot: u8, minor: u8) -> [u8; HEADER_SIZE] {
    let len = 5003;
    let b = body(len, minor);
    s.erase(APP, slot, len, &t()).unwrap();
    let mut off = 0usize;
    for n in [1usize, 3, 5, 1024, 7, 1024, 2, 1024].iter().cycle() {
        let end = (off + n).min(b.len());
        s.write(APP, slot, off as u32, &b[off..end], &t()).unwrap();
        off = end;
        if off == b.len() {
            break;
        }
    }
    let mut back = vec![0u8; len as usize];
    for (i, c) in back.chunks_mut(256).enumerate() {
        s.read(APP, slot, i as u32 * 256, c).unwrap();
    }
    assert_eq!(back, b);
    let h = header(minor, len);
    s.write_header(slot, &h, &t()).unwrap();
    h
}

fn digest(h: &[u8; HEADER_SIZE]) -> [u8; 16] {
    header::header_digest16(h)
}

#[test]
fn receive_pads_the_last_unit_and_erases_the_header_sector_first() {
    let mut s = store();
    s.write_header(SLOT_B, &header(0, 1), &t()).unwrap();
    let h = receive(&mut s, SLOT_B, 1);
    let tail = (OTA_1.offset + 5003) as usize;
    assert_eq!(s.flash.0[tail], 0xFF, "pad byte");
    assert!(s.flash.0[tail + 1..tail + 4096].iter().all(|&b| b == 0xFF));
    let mut back = [0u8; HEADER_SIZE];
    s.read_header(SLOT_B, &mut back).unwrap();
    assert_eq!(back, h);
    // Header sector of B only: A's and factory's stay erased.
    let a = header_addr(SLOT_A).unwrap() as usize;
    assert!(s.flash.0[a..a + HEADER_SECTOR as usize]
        .iter()
        .all(|&b| b == 0xFF));
    assert!(s.wdt.0 > 0, "fed while erasing and writing");

    s.erase(APP, SLOT_B, 10, &t()).unwrap();
    s.read_header(SLOT_B, &mut back).unwrap();
    assert_eq!(back, [0xFF; HEADER_SIZE], "no image after erase");
    assert_eq!(s.status().slots[1].state, SlotState::Empty);
}

#[test]
fn refuses_running_slot_gaps_and_other_targets() {
    let mut s = store();
    assert_eq!(s.capacity(APP, SLOT_A), OTA_0.size);
    assert_eq!(s.capacity(APP, SLOT_FACTORY), 0);
    assert_eq!(s.capacity(3, 0), 0);
    s.ota.running = SLOT_A;
    assert_eq!(s.erase(APP, SLOT_A, 16, &t()), Err(StoreError::Unsupported));
    assert_eq!(
        s.erase(APP, SLOT_B, OTA_1.size + 1, &t()),
        Err(StoreError::Range)
    );
    assert_eq!(s.write(APP, SLOT_B, 0, &[1], &t()), Err(StoreError::Range));
    s.erase(APP, SLOT_B, 16, &t()).unwrap();
    assert_eq!(
        s.write(APP, SLOT_B, 4, &[1; 4], &t()),
        Err(StoreError::Range)
    );
    assert_eq!(
        s.write(APP, SLOT_B, 0, &[1; 17], &t()),
        Err(StoreError::Range)
    );
    assert_eq!(s.erase(3, 0, 16, &t()), Err(StoreError::Unsupported));
}

#[test]
fn trial_confirm_and_rollback_map_to_status() {
    let mut s = store();
    // Factory runs over blank otadata.
    let st = s.status();
    assert_eq!((st.active, st.confirmed), (SLOT_FACTORY, SLOT_FACTORY));

    let ha = receive(&mut s, SLOT_A, 0);
    assert_eq!(s.status().slots[0].state, SlotState::Written);
    s.set_pending(SLOT_A, &ha, &t()).unwrap();
    assert_eq!(s.status().slots[0].state, SlotState::Pending);
    s.ota.boot();
    let st = s.status();
    assert_eq!(st.slots[0].state, SlotState::Trying);
    assert_eq!((st.active, st.confirmed, st.attempts), (0, SLOT_FACTORY, 1));
    s.confirm(SLOT_A, &ha, &t()).unwrap();
    s.ota.boot();
    let st = s.status();
    assert_eq!(st.slots[0].state, SlotState::Confirmed);
    assert_eq!((st.active, st.confirmed, st.attempts), (0, 0, 0));

    // Update to B, confirm: A stays Written as the previous version.
    let hb = receive(&mut s, SLOT_B, 1);
    s.set_pending(SLOT_B, &hb, &t()).unwrap();
    s.ota.boot();
    let st = s.status();
    assert_eq!((st.active, st.confirmed), (1, 0), "falls back to A");
    assert_eq!(st.slots[1].state, SlotState::Trying);
    s.confirm(SLOT_B, &hb, &t()).unwrap();
    s.ota.boot();
    let st = s.status();
    assert_eq!((st.active, st.confirmed), (1, 1));
    assert_eq!(st.slots[1].state, SlotState::Confirmed);
    assert_eq!(st.slots[0].state, SlotState::Written);
    assert_eq!(st.slots[1].version, Version::new(1, 1, 0));
    assert_eq!(st.slots[1].digest16, digest(&hb));
    assert_eq!(st.last_reason, reason::NONE);

    // An image that never confirms: one trial, then back to B.
    let hn = receive(&mut s, SLOT_A, 2);
    s.set_pending(SLOT_A, &hn, &t()).unwrap();
    s.ota.boot();
    assert_eq!(s.status().slots[0].state, SlotState::Trying);
    s.ota.boot();
    let st = s.status();
    assert_eq!((st.active, st.confirmed), (1, 1));
    assert_eq!(st.slots[1].state, SlotState::Confirmed);
    assert_eq!(st.slots[0].state, SlotState::Reverted);
    assert_eq!(st.slots[0].version, Version::new(1, 2, 0));
    assert_eq!(st.slots[0].digest16, digest(&hn));
    assert_eq!(st.last_reason, reason::TRIAL_EXHAUSTED);
    // Still so after a plain reboot.
    s.ota.boot();
    assert_eq!(s.status(), st);
}

#[test]
fn set_pending_back_to_the_previously_confirmed_slot() {
    let mut s = store();
    for (slot, minor) in [(SLOT_A, 0), (SLOT_B, 1)] {
        let h = receive(&mut s, slot, minor);
        s.set_pending(slot, &h, &t()).unwrap();
        s.ota.boot();
        s.confirm(slot, &h, &t()).unwrap();
    }
    assert_eq!(s.status().active, SLOT_B);
    // Back to the 1.0.0 still in A: no transfer, only the trial entry.
    let mut ha = [0u8; HEADER_SIZE];
    s.read_header(SLOT_A, &mut ha).unwrap();
    s.set_pending(SLOT_A, &ha, &t()).unwrap();
    assert_eq!(s.status().slots[0].state, SlotState::Pending);
    s.ota.boot();
    let st = s.status();
    assert_eq!((st.active, st.confirmed), (0, 1), "B is the fallback");
    assert_eq!(st.slots[0].state, SlotState::Trying);
    s.confirm(SLOT_A, &ha, &t()).unwrap();
    s.ota.boot();
    let st = s.status();
    assert_eq!((st.active, st.confirmed), (0, 0));
    assert_eq!(st.slots[0].version, Version::new(1, 0, 0));
    assert_eq!(st.slots[1].state, SlotState::Written);
}

fn set_pending_frame(slot: u8) -> Vec<u8> {
    use dagon_proto::payload::SetPending;
    let mut f = vec![0u8; dagon_proto::frame_len(2)];
    SetPending { target: APP, slot }
        .encode(&mut f[dagon_proto::HEADER_LEN..])
        .unwrap();
    let n = dagon_proto::finish(&mut f, 0, 7, dagon_proto::Cmd::SetPending as u8, 2).unwrap();
    f.truncate(n);
    f
}

#[test]
fn set_pending_is_refused_while_on_trial() {
    use crate::{refuse_set_pending_on_trial, REFUSAL_LEN};
    use dagon_proto::{split_status, Frame, Status};
    let mut s = store();
    let mut out = [0u8; REFUSAL_LEN];
    let ha = receive(&mut s, SLOT_A, 0);
    s.set_pending(SLOT_A, &ha, &t()).unwrap();
    s.ota.boot();
    s.confirm(SLOT_A, &ha, &t()).unwrap();
    // Confirmed: SET_PENDING goes through to the agent.
    assert_eq!(
        refuse_set_pending_on_trial(&mut s, &set_pending_frame(1), &mut out),
        None
    );
    let hb = receive(&mut s, SLOT_B, 1);
    s.set_pending(SLOT_B, &hb, &t()).unwrap();
    s.ota.boot();
    let before = s.ota.entries;
    // On trial in B: refused with E_STATE, otadata untouched.
    let n = refuse_set_pending_on_trial(&mut s, &set_pending_frame(0), &mut out).unwrap();
    let (f, _) = Frame::decode(&out[..n], 1024).unwrap();
    assert_eq!((f.seq, f.cmd), (7, dagon_proto::Cmd::SetPending as u8));
    assert_eq!(split_status(f.payload).unwrap().0, Status::State);
    assert_eq!(s.ota.entries, before);
    assert_eq!(s.status().slots[1].state, SlotState::Trying);
    // Other commands are not touched.
    let mut hello = set_pending_frame(0);
    dagon_proto::finish(&mut hello, 0, 7, dagon_proto::Cmd::Hello as u8, 2).unwrap();
    assert!(Frame::decode(&hello, 1024).is_ok());
    assert_eq!(refuse_set_pending_on_trial(&mut s, &hello, &mut out), None);
}

#[test]
fn trial_from_blank_otadata_rolls_back_to_factory() {
    let mut s = store();
    let hb = receive(&mut s, SLOT_B, 3);
    s.set_pending(SLOT_B, &hb, &t()).unwrap();
    s.ota.boot();
    assert_eq!(s.status().confirmed, SLOT_FACTORY);
    s.ota.boot();
    let st = s.status();
    assert_eq!(st.active, SLOT_FACTORY);
    assert_eq!(st.slots[1].state, SlotState::Reverted);
    assert_eq!(st.last_reason, reason::TRIAL_EXHAUSTED);
    // The entry written for ota_1 from blank otadata had seq 2.
    assert!(s
        .ota
        .entries
        .iter()
        .any(|e| e[..4] == 2u32.to_le_bytes() && e[28..] == seq_crc(2).to_le_bytes()));
}
