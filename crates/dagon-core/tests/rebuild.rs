//! A state log lost on both faces, rebuilt from the APP slots
//! (`dagon_core::boot::rebuild`): which records each set of passing images
//! gives, that the boot then starts what they name, that an image failing
//! the boot checks is never named, and that a power cut at any flash step
//! of the rebuild leaves either no valid face (the next start rebuilds it
//! again) or the whole rebuilt log.

#[path = "bench/mod.rs"]
mod bench;

use bench::*;
use dagon_core::boot::{rebuild, Passing};

/// Both faces garbage.
fn lose_log(m: &mut Mem) {
    for face in LOG.faces {
        m.erase(face, LOG.face_len).unwrap();
        m.put(face, &[0x13; 64]);
    }
}

/// The records of the active face, in order.
fn records(m: &mut Mem) -> Vec<(u8, u8)> {
    let log = Log::load(m, &LOG).unwrap();
    let face = LOG.faces[usize::from(log.generation != 1)];
    let mut out = Vec::new();
    let mut at = face + FACE_HEADER_SIZE as u32;
    loop {
        let mut b = [0u8; 32];
        m.read(at, &mut b).unwrap();
        if Record::is_erased(&b) {
            return out;
        }
        let r = Record::parse(&b).unwrap();
        out.push((r.kind, r.slot));
        at += 32;
    }
}

use dagon_format::statelog::FACE_HEADER_SIZE;

/// `h` signed again under the test key.
fn resign(mut h: ImageHeader) -> [u8; HEADER_SIZE] {
    let kp = KeyPair::from_seed(Seed::new(SEED));
    let msg = [SIGN_DOMAIN, &h.to_bytes()[..SIGNED_LEN]].concat();
    h.signature = *kp.sk.sign(msg, None);
    h.to_bytes()
}

fn rebuilt(m: &mut Mem) -> Result<Log, LogError> {
    rebuild(m, &LOG, &SLOTS, &cfg(), &verifier())
}

/// Two images: the older one is confirmed again, the newer one starts on
/// trial, whichever slot holds it.
#[test]
fn two_images_give_the_older_confirmed_and_the_newer_on_trial() {
    for (va, vb, old, new) in [(0, 1, A, B), (3, 2, B, A)] {
        let mut m = Mem::new();
        let a = install(&mut m, A, &image(A, va));
        let b = install(&mut m, B, &image(B, vb));
        lose_log(&mut m);
        let log = rebuilt(&mut m).unwrap();
        let (o, n) = if old == A { (a, b) } else { (b, a) };
        assert_eq!(
            (log.state.confirmed, log.state.pending, log.state.attempts),
            (Some(o), Some(n), 0)
        );
        assert_eq!(
            records(&mut m),
            [(kind::PROVISION, old), (kind::SLOT_PENDING, new)]
        );
        assert_eq!(
            boot(&mut m),
            Decision::Boot {
                slot: new,
                entry: entry(new),
                trial: true
            }
        );
    }
}

/// One image: on trial alone, with nothing confirmed; once its starts
/// are spent nothing starts (recovery mode receives an app).
#[test]
fn one_image_starts_on_trial_only() {
    let mut m = Mem::new();
    let b = install(&mut m, B, &image(B, 1));
    lose_log(&mut m);
    let log = rebuilt(&mut m).unwrap();
    assert_eq!((log.state.confirmed, log.state.pending), (None, Some(b)));
    for _ in 0..cfg().max_attempts {
        assert!(matches!(
            boot(&mut m),
            Decision::Boot {
                slot: B,
                trial: true,
                ..
            }
        ));
    }
    assert_eq!(boot(&mut m), Decision::NoImage);
}

/// Two images of the same version: the one in the lower slot is
/// confirmed, alone.
#[test]
fn two_images_of_one_version_confirm_the_lower_slot() {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 2));
    install(&mut m, B, &image(B, 2));
    lose_log(&mut m);
    let log = rebuilt(&mut m).unwrap();
    assert_eq!((log.state.confirmed, log.state.pending), (Some(a), None));
    assert_eq!(
        boot(&mut m),
        Decision::Boot {
            slot: A,
            entry: entry(A),
            trial: false
        }
    );
}

/// An image failing a boot check is never named: a corrupt body, a
/// signature under another key, another node's tag, a header bound to
/// the other slot.
#[test]
fn an_image_failing_the_checks_is_not_named() {
    let bad: [fn(&mut Mem); 4] = [
        |m| m.put(SLOTS.slots[B as usize].body, &[0xA5; 32]),
        |m| {
            let (mut h, body) = image(B, 1);
            h[HEADER_SIZE - 1] ^= 1;
            install(m, B, &(h, body));
        },
        |m| {
            let (hdr, body) = image(B, 1);
            let mut h = ImageHeader::parse(&hdr).unwrap();
            h.target_tag = target_tag("another-node");
            install(m, B, &(resign(h), body));
        },
        |m| {
            let (hdr, body) = image(B, 1);
            let mut h = ImageHeader::parse(&hdr).unwrap();
            h.slot_binding = A;
            install(m, B, &(resign(h), body));
        },
    ];
    for (i, spoil) in bad.iter().enumerate() {
        let mut m = Mem::new();
        let a = install(&mut m, A, &image(A, 0));
        install(&mut m, B, &image(B, 1));
        spoil(&mut m);
        lose_log(&mut m);
        let log = rebuilt(&mut m).unwrap();
        assert_eq!(
            (log.state.confirmed, log.state.pending),
            (None, Some(a)),
            "case {i}"
        );
    }
}

/// No image passes: nothing is written and the log stays lost.
#[test]
fn no_image_leaves_the_log_lost() {
    let mut m = Mem::new();
    lose_log(&mut m);
    let before = m.bytes.clone();
    assert_eq!(rebuilt(&mut m).err(), Some(LogError::NoValidFace));
    assert!(m.bytes == before);
}

/// A log with a valid face is never rebuilt, even one naming nothing.
#[test]
fn a_valid_log_is_left_as_it_is() {
    let mut m = Mem::new();
    install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[]);
    let before = m.bytes.clone();
    let log = rebuilt(&mut m).unwrap();
    assert_eq!(log.state, Default::default());
    assert!(m.bytes == before);
}

/// The records for each set of passing images, as the doc gives them.
#[test]
fn the_records_follow_the_versions() {
    let r = |slot, minor| slot_ref(slot, &image(slot, minor).0);
    let of = |refs: &[SlotRef]| {
        refs.iter()
            .fold(Passing::default(), |p, &x| p.with(x))
            .records()
            .collect::<Vec<_>>()
    };
    assert_eq!(of(&[]), []);
    assert_eq!(of(&[r(B, 1)]), [(kind::SLOT_PENDING, r(B, 1))]);
    assert_eq!(
        of(&[r(A, 1), r(B, 0)]),
        [(kind::PROVISION, r(B, 0)), (kind::SLOT_PENDING, r(A, 1))]
    );
    assert_eq!(of(&[r(A, 2), r(B, 2)]), [(kind::PROVISION, r(A, 2))]);
}

/// A flash that stops at its `cut`-th erase or write: that step writes
/// its first half (an erase leaves its first half erased), and it and
/// every later step fail.
struct Cut {
    m: Mem,
    cut: usize,
    steps: usize,
}

impl Flash for Cut {
    fn geometry(&self) -> &[FlashRegion] {
        &[]
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        self.m.read(addr, buf)
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        self.steps += 1;
        match self.steps.cmp(&self.cut) {
            std::cmp::Ordering::Less => self.m.erase(addr, len),
            std::cmp::Ordering::Equal => {
                let _ = self.m.erase(addr, len / 2);
                Err(FlashError::Hw)
            }
            std::cmp::Ordering::Greater => Err(FlashError::Hw),
        }
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        self.steps += 1;
        match self.steps.cmp(&self.cut) {
            std::cmp::Ordering::Less => self.m.write(addr, data),
            std::cmp::Ordering::Equal => {
                let half = (data.len() / 2) & !3;
                let _ = self.m.write(addr, &data[..half]);
                Err(FlashError::Hw)
            }
            std::cmp::Ordering::Greater => Err(FlashError::Hw),
        }
    }
}

/// A cut at every flash step of a rebuild: the log is lost still (and the
/// next rebuild gives the whole log) or whole; never a valid face holding
/// part of it.
#[test]
fn a_power_cut_at_every_step_leaves_no_log_or_the_whole_log() {
    let mut fresh = Mem::new();
    install(&mut fresh, A, &image(A, 0));
    install(&mut fresh, B, &image(B, 1));
    lose_log(&mut fresh);
    let mut whole = Mem {
        bytes: fresh.bytes.clone(),
        writes: 0,
    };
    let want = rebuilt(&mut whole).unwrap().state;
    let mut cut = 1;
    loop {
        let mut f = Cut {
            m: Mem {
                bytes: fresh.bytes.clone(),
                writes: 0,
            },
            cut,
            steps: 0,
        };
        let done = rebuild(&mut f, &LOG, &SLOTS, &cfg(), &verifier()).is_ok();
        let mut m = f.m;
        match Log::load(&mut m, &LOG) {
            Ok(l) => assert_eq!(l.state, want, "cut at step {cut}"),
            Err(e) => {
                assert_eq!(e, LogError::NoValidFace, "cut at step {cut}");
                assert_eq!(rebuilt(&mut m).unwrap().state, want, "cut at step {cut}");
            }
        }
        if done {
            break;
        }
        cut += 1;
    }
    // An erase, two records and the header.
    assert_eq!(cut, 5);
}
