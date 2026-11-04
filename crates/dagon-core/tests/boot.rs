//! Boot decision against an in-memory flash with the F401 layout. Each
//! "boot" loads the log from flash and decides, like a reset would.

mod bench;

use bench::*;

#[test]
fn confirmed_boot_writes_nothing() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    for _ in 0..3 {
        let d = boot(&mut m);
        assert_eq!(
            d,
            Decision::Boot {
                slot: A,
                entry: entry(A),
                trial: false
            }
        );
    }
    assert_eq!(m.writes, 0);
}

#[test]
fn three_trials_then_rollback() {
    let mut m = with_pending_b();
    for n in 1..=3 {
        let d = boot(&mut m);
        assert_eq!(
            d,
            Decision::Boot {
                slot: B,
                entry: entry(B),
                trial: true
            }
        );
        assert_eq!(state(&mut m).attempts, n);
    }
    // Never confirmed: the fourth boot goes back to A.
    let d = boot(&mut m);
    assert_eq!(
        d,
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    let s = state(&mut m);
    assert_eq!(s.pending, None);
    assert_eq!(s.reverted.map(|r| r.slot), Some(B));
    assert_eq!(s.last_reason, reason::TRIAL_EXHAUSTED);
    let writes = m.writes;
    assert_eq!(
        boot(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    assert_eq!(m.writes, writes, "boot after rollback writes nothing");
}

#[test]
fn confirm_on_trial_makes_slot_the_confirmed_one() {
    let mut m = with_pending_b();
    assert!(matches!(
        boot(&mut m),
        Decision::Boot {
            slot: B,
            trial: true,
            ..
        }
    ));
    // The running app confirms, as the agent does on CONFIRM.
    let mut log = Log::load(&mut m, &LOG).unwrap();
    assert!(log.state.on_trial());
    let b = log.state.pending.unwrap();
    log.append(&mut m, kind::CONFIRMED, b, reason::NONE)
        .unwrap();
    for _ in 0..4 {
        assert_eq!(
            boot(&mut m),
            Decision::Boot {
                slot: B,
                entry: entry(B),
                trial: false
            }
        );
    }
    assert_eq!(state(&mut m).confirmed.map(|c| c.slot), Some(B));
}

#[test]
fn tampered_pending_body_rolls_back_without_trial() {
    let mut m = with_pending_b();
    let addr = entry(B) + 100;
    let byte = m.bytes[(addr - BASE) as usize];
    m.put(addr, &[byte ^ 1]);
    assert_eq!(
        boot(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    let s = state(&mut m);
    assert_eq!((s.pending, s.attempts), (None, 0));
    assert_eq!(s.last_reason, reason::HASH_MISMATCH);
}

#[test]
fn bad_signature_rolls_back() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let (mut hdr, body) = image(B, 1);
    hdr[0x1C0] ^= 1;
    let b = install(&mut m, B, &(hdr, body));
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, b),
        ],
    );
    assert_eq!(
        boot(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    assert_eq!(state(&mut m).last_reason, reason::SIG_INVALID);
}

#[test]
fn image_bound_to_other_slot_is_refused() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    // Slot-A image placed in slot B.
    let b = install(&mut m, B, &image(A, 1));
    let b = SlotRef { slot: B, ..b };
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, b),
        ],
    );
    assert_eq!(
        boot(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    assert_eq!(state(&mut m).last_reason, reason::SIG_INVALID);
}

#[test]
fn corrupt_confirmed_slot_has_no_image() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    let addr = entry(A) + 7;
    let byte = m.bytes[(addr - BASE) as usize];
    m.put(addr, &[byte ^ 0x80]);
    assert_eq!(boot(&mut m), Decision::NoImage);
}

#[test]
fn header_not_the_confirmed_one_has_no_image() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    install(&mut m, A, &image(A, 5));
    assert_eq!(boot(&mut m), Decision::NoImage);
}

#[test]
fn erased_log_has_no_valid_face() {
    let mut m = Mem::new();
    assert_eq!(Log::load(&mut m, &LOG), Err(LogError::NoValidFace));
}

#[test]
fn newer_face_wins() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let b = install(&mut m, B, &image(B, 1));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a)]);
    write_log(&mut m, 1, 2, &[(kind::PROVISION, b)]);
    let log = Log::load(&mut m, &LOG).unwrap();
    assert_eq!((log.generation, log.state.active()), (2, B));
}

#[test]
fn load_addr_must_be_slot_body() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let b = install(&mut m, B, &image_at(B, 1, SLOTS.slots[B as usize]));
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, b),
        ],
    );
    assert_eq!(
        boot(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
    assert_eq!(state(&mut m).last_reason, reason::SIG_INVALID);
}

#[test]
fn rollback_without_confirmed_slot_writes_nothing() {
    let mut m = Mem::new();
    let b = install(&mut m, B, &image(B, 1));
    let mut bad = b;
    bad.digest16[0] ^= 1;
    write_log(&mut m, 0, 1, &[(kind::SLOT_PENDING, bad)]);
    assert_eq!(boot(&mut m), Decision::NoImage);
    assert_eq!(m.writes, 0);
}

/// Hub-sized faces: 4 KiB, 127 records each.
const SMALL: LogLayout = LogLayout {
    faces: [0x0800_8000, 0x0800_9000],
    face_len: 0x1000,
};

#[test]
fn full_face_compacts_into_other_face() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let b = install(&mut m, B, &image(B, 1));
    assert_eq!(Log::load(&mut m, &SMALL), Err(LogError::NoValidFace));
    write_face(
        &mut m,
        &SMALL,
        0,
        1,
        &[(kind::PROVISION, a), (kind::CONFIRMED, a)],
    );
    // Fill face 0 with pending/attempt cycles, then keep going through two
    // more compactions; the state must survive each switch.
    for i in 0..300u32 {
        let mut log = Log::load(&mut m, &SMALL).unwrap();
        let k = if i % 3 == 0 {
            kind::SLOT_PENDING
        } else {
            kind::BOOT_ATTEMPT
        };
        log.append(&mut m, k, b, reason::NONE).unwrap();
        let again = Log::load(&mut m, &SMALL).unwrap();
        assert_eq!(again.state, log.state, "record {i}");
        assert_eq!(again.state.attempts, (i % 3) as u8);
        assert_eq!(again.state.confirmed, Some(a));
    }
    let log = Log::load(&mut m, &SMALL).unwrap();
    assert!(
        log.generation >= 3,
        "two compactions at least, got {}",
        log.generation
    );
}

#[test]
fn compaction_keeps_last_reason_and_attempts() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let b = install(&mut m, B, &image(B, 1));
    let recs = [
        (kind::PROVISION, a),
        (kind::SLOT_PENDING, b),
        (kind::BOOT_ATTEMPT, b),
    ];
    write_face(&mut m, &SMALL, 0, 1, &recs);
    let mut log = Log::load(&mut m, &SMALL).unwrap();
    log.append(&mut m, kind::ROLLBACK, a, reason::TRIAL_EXHAUSTED)
        .unwrap();
    log.append(&mut m, kind::SLOT_PENDING, b, reason::NONE)
        .unwrap();
    log.append(&mut m, kind::BOOT_ATTEMPT, b, reason::NONE)
        .unwrap();
    log.append(&mut m, kind::BOOT_ATTEMPT, b, reason::NONE)
        .unwrap();
    // Pad the face to the end with records for another target.
    let used = 32 + 7 * 32;
    let mut addr = SMALL.faces[0] + used;
    while addr + 32 <= SMALL.faces[0] + SMALL.face_len {
        let r = Record {
            kind: kind::CONFIRMED,
            target: target::BOOTCORE,
            slot: 0,
            reason: 0,
            seq: 0,
            version: Version::new(0, 0, 0),
            digest16: [0; 16],
        };
        m.put(addr, &r.to_bytes());
        addr += 32;
    }
    let mut log = Log::load(&mut m, &SMALL).unwrap();
    let before = log.state;
    log.append(&mut m, kind::BOOT_ATTEMPT, b, reason::NONE)
        .unwrap();
    let after = Log::load(&mut m, &SMALL).unwrap();
    assert_eq!(after.generation, 2);
    assert_eq!(after.state.attempts, before.attempts + 1);
    assert_eq!(after.state.pending, before.pending);
    assert_eq!(after.state.confirmed, Some(a));
    assert_eq!(after.state.last_reason, reason::TRIAL_EXHAUSTED);
}

#[test]
fn torn_tail_compacts_on_next_append() {
    let mut m = with_pending_b();
    // Third record (SLOT_PENDING B) torn: the fold keeps the prefix.
    let rec3 = LOG.faces[0] + 32 + 2 * 32;
    let byte = m.bytes[(rec3 + 8 - BASE) as usize];
    m.put(rec3 + 8, &[byte ^ 1]);
    let mut log = Log::load(&mut m, &LOG).unwrap();
    assert_eq!(log.state.pending, None);
    let a = log.state.confirmed.unwrap();
    let b = install(&mut m, B, &image(B, 1));
    log.append(&mut m, kind::SLOT_PENDING, b, reason::NONE)
        .unwrap();
    let again = Log::load(&mut m, &LOG).unwrap();
    assert_eq!(again.generation, 2);
    assert_eq!(again.state.confirmed, Some(a));
    assert_eq!(again.state.pending, Some(b));
    // The torn face is no longer the active one, and a trial proceeds.
    assert!(matches!(
        boot(&mut m),
        Decision::Boot {
            slot: B,
            trial: true,
            ..
        }
    ));
}

#[test]
fn cut_compaction_leaves_old_face_valid() {
    let mut m = with_pending_b();
    let log = Log::load(&mut m, &LOG).unwrap();
    // Other face erased and partly written, header never written.
    m.erase(LOG.faces[1], LOG.face_len).unwrap();
    m.put(LOG.faces[1] + 32, &[0u8; 32]);
    let again = Log::load(&mut m, &LOG).unwrap();
    assert_eq!((again.generation, again.state), (log.generation, log.state));
}

/// Flash whose next erase fails after leaving garbage in the erased range.
struct FailingErase {
    mem: Mem,
    fail_next: bool,
}

impl Flash for FailingErase {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.mem.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.mem.erase(addr, len)?;
        if std::mem::take(&mut self.fail_next) {
            self.mem.put(addr, &[0u8; 32]);
            return Err(FlashError::Hw);
        }
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.mem.write(addr, data)
    }
}

#[test]
fn failed_compaction_erase_keeps_the_active_face() {
    let mut m = with_pending_b();
    // Torn third record: the next append must compact first.
    let rec3 = LOG.faces[0] + 32 + 2 * 32;
    let byte = m.bytes[(rec3 + 8 - BASE) as usize];
    m.put(rec3 + 8, &[byte ^ 1]);
    let mut f = FailingErase {
        mem: m,
        fail_next: true,
    };
    let mut log = Log::load(&mut f, &LOG).unwrap();
    let before = (log.generation, log.state);
    let a = log.state.confirmed.unwrap();
    assert!(log
        .append(&mut f, kind::CONFIRMED, a, reason::NONE)
        .is_err());
    f.fail_next = true;
    assert!(log
        .append(&mut f, kind::CONFIRMED, a, reason::NONE)
        .is_err());
    let again = Log::load(&mut f, &LOG).unwrap();
    assert_eq!((again.generation, again.state), before);
    // Once erasing works, the retry compacts normally.
    log.append(&mut f, kind::CONFIRMED, a, reason::NONE)
        .unwrap();
    assert_eq!(Log::load(&mut f, &LOG).unwrap().generation, before.0 + 1);
}

/// Flash whose `n`-th write from now fails (0 = the next one).
struct FailingWrite {
    mem: Mem,
    fail_in: Option<usize>,
}

impl Flash for FailingWrite {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.mem.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.mem.erase(addr, len)
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        match self.fail_in {
            Some(0) => {
                self.fail_in = None;
                Err(FlashError::Hw)
            }
            Some(n) => {
                self.fail_in = Some(n - 1);
                self.mem.write(addr, data)
            }
            None => self.mem.write(addr, data),
        }
    }
}

#[test]
fn failed_compaction_write_keeps_the_active_face() {
    // The torn tail leaves A confirmed only: compaction writes SNAPSHOT, then
    // the face header.
    for fail_in in 0..2 {
        let mut m = with_pending_b();
        let rec3 = LOG.faces[0] + 32 + 2 * 32;
        let byte = m.bytes[(rec3 + 8 - BASE) as usize];
        m.put(rec3 + 8, &[byte ^ 1]);
        let mut f = FailingWrite {
            mem: m,
            fail_in: Some(fail_in),
        };
        let mut log = Log::load(&mut f, &LOG).unwrap();
        let before = (log.generation, log.state);
        let a = log.state.confirmed.unwrap();
        assert!(log
            .append(&mut f, kind::CONFIRMED, a, reason::NONE)
            .is_err());
        assert_eq!((log.generation, log.state), before, "fail_in {fail_in}");
        let again = Log::load(&mut f, &LOG).unwrap();
        assert_eq!((again.generation, again.state), before);
        log.append(&mut f, kind::CONFIRMED, a, reason::NONE)
            .unwrap();
        assert_eq!(Log::load(&mut f, &LOG).unwrap().generation, before.0 + 1);
    }
}
