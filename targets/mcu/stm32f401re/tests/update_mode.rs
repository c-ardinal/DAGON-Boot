//! The boot core writing the app in update mode, over the F401 layout: the
//! update session as the boot core's writer (`Agent<_, _, _, BootCore>`)
//! over the board's slot store (`KindSStore`), its verifier and the boot
//! decision (`dagon_core::boot::decide`) on the same flash. Before a write
//! starts the store makes the log's room with the shared reservation, which
//! compacts a full log; a power cut lands at every flash operation of the
//! write and of that reservation. After any cut the log loads, a slot the
//! write did not touch still starts, and the boot core writes the image
//! again and the boot decision starts it: no state is left that cannot be
//! recovered.

mod cut;

use cut::*;

use dagon_agent::writer::BootCore;
use dagon_agent::{Agent, Config};
use dagon_core::boot::{decide, BootConfig, Decision};
use dagon_core::log::Log;
use dagon_ed25519::Ed25519Small;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_format::header::*;
use dagon_format::statelog::kind;
use dagon_mcu_stm32f401re::layout::*;
use dagon_mcu_stm32f401re::{KindSStore, ERASE_REGIONS, TRIAL_ATTEMPTS};
use dagon_proto::payload::{mode, role, target, SetPending, XferBegin, XferChunk};
use dagon_proto::{flags, max_payload, split_status, Cmd, Frame, Status};
use ed25519_compact::{KeyPair, Seed};

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

/// The boot core's chain: no extension.
fn chain() -> Chain {
    let events: &'static mut Published<(), 4> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    let periods = Watchdog::new(TRIAL_WDT_MS, CONFIRMED_WDT_MS);
    compose(Layer::App, events, Nop, Nop, Bounds::new(now, periods))
}

fn verifier() -> Ed25519Small {
    Ed25519Small::new(*KeyPair::from_seed(Seed::new(SEED)).pk)
}

/// The boot core's session in update mode, over `f`; `None` when the log
/// does not load.
fn session(f: &Cut) -> Option<Agent<KindSStore<Cut>, Chain, Ed25519Small, BootCore>> {
    let store = KindSStore::load(
        f.clone(),
        &LOG,
        SLOTS,
        &ERASE_REGIONS,
        dagon_format::statelog::slot::NONE,
    )
    .ok()?
    .with_cores(&CORE_SLOTS, true);
    let cfg = Config {
        mcu_id: MCU_ID,
        port_kind: port_kind::SELF_BOOT,
        target_tag: target_tag("node-f401"),
        role: role::NODE,
        mode: mode::UPDATE,
        max_payload: max_payload::I2C as u16,
        trial_attempts: TRIAL_ATTEMPTS,
    };
    Some(Agent::for_writer(cfg, verifier(), store, chain()))
}

fn call<S, X, V, W>(a: &mut Agent<S, X, V, W>, seq: u8, cmd: Cmd, payload: &[u8]) -> Status
where
    S: dagon_agent::SlotStore,
    X: dagon_agent::AppChain,
    V: dagon_format::SigVerifier,
    W: dagon_agent::Writer,
{
    let mut buf = [0u8; 300];
    let n = Frame {
        flags: 0,
        seq,
        cmd: cmd as u8,
        payload,
    }
    .encode(&mut buf)
    .unwrap();
    let reply = a.handle(&buf[..n]).expect("reply");
    let (f, _) = Frame::decode(reply.frame, 300).unwrap();
    assert_eq!(f.flags, flags::RESP);
    split_status(f.payload).unwrap().0
}

/// The host's write through the boot core: XFER of `img` into APP `slot`,
/// then SET_PENDING. `None` at the first request not answered OK.
fn write_in_update_mode(f: &Cut, slot: u8, img: &[u8]) -> Option<()> {
    let mut a = session(f)?;
    let mut seq = 0u8;
    let mut ok = |a: &mut Agent<_, _, _, BootCore>, cmd, p: &[u8]| {
        seq = seq.wrapping_add(1);
        (call(a, seq, cmd, p) == Status::Ok).then_some(())
    };
    let mut p = [0u8; 256];
    let n = XferBegin {
        target: target::APP,
        slot,
        total_len: img.len() as u32,
        hdr_len: HEADER_SIZE as u16,
    }
    .encode(&mut p)
    .unwrap();
    ok(&mut a, Cmd::XferBegin, &p[..n])?;
    let (hdr, body) = img.split_at(HEADER_SIZE);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(200).enumerate() {
            let n = XferChunk {
                offset: (i * 200) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            ok(&mut a, cmd, &p[..n])?;
        }
    }
    ok(&mut a, Cmd::XferEnd, &[])?;
    let n = SetPending {
        target: target::APP,
        slot,
    }
    .encode(&mut p)
    .unwrap();
    ok(&mut a, Cmd::SetPending, &p[..n])
}

/// The boot decision at the next start.
fn boots(f: &mut Cut) -> Option<Decision> {
    let mut log = Log::load(f, &LOG).ok()?;
    let cfg = BootConfig {
        mcu_id: MCU_ID,
        target_tag: target_tag("node-f401"),
        max_attempts: TRIAL_ATTEMPTS,
    };
    Some(decide(f, &mut log, &SLOTS, &cfg, &verifier()))
}

fn app(slot: u8) -> Vec<u8> {
    signed(
        image_kind::APP,
        slot,
        SLOTS.slots[usize::from(slot)].header,
        4,
        2000,
    )
}

/// What the start after a cut leaves: `Ok` when it is recoverable, the
/// reason when it is not. `slot` is the slot the cut write went into; app
/// A was the confirmed one.
fn recoverable(after: &Cut, slot: u8) -> Result<(), String> {
    if Log::load(&mut after.copy(), &LOG).is_err() {
        return Err("no log face loads".into());
    }
    match boots(&mut after.copy()) {
        Some(Decision::Boot { .. }) => {}
        Some(Decision::NoImage) if slot == 0 => {}
        d => return Err(format!("the untouched app A does not start: {d:?}")),
    }
    let mut again = after.copy();
    write_in_update_mode(&again, slot, &app(slot)).ok_or("the write again fails")?;
    match boots(&mut again) {
        Some(Decision::Boot {
            slot: s,
            trial: true,
            ..
        }) if s == slot => Ok(()),
        d => Err(format!("the image written again does not start: {d:?}")),
    }
}

/// The boot core writes an app into either APP slot (the confirmed one
/// too) and sets it pending; the boot decision then tries it.
#[test]
fn the_boot_core_writes_the_app_in_update_mode() {
    for slot in [1, 0] {
        let mut f = installed();
        write_in_update_mode(&f, slot, &app(slot)).expect("written");
        assert_eq!(
            boots(&mut f),
            Some(Decision::Boot {
                slot,
                entry: SLOTS.slots[usize::from(slot)].body,
                trial: true
            }),
            "slot {slot}"
        );
    }
}

/// A power cut at every flash operation of the boot core's write and of
/// its log reservation (a full log is compacted first) leaves no state
/// that cannot be recovered.
#[test]
fn a_power_cut_at_every_step_of_a_boot_core_write_leaves_every_state_recoverable() {
    let mut cuts = 0;
    let mut covered = Coverage::default();
    let mut unrecoverable = Vec::new();
    for (full, slot) in [(false, 1), (true, 1), (false, 0), (true, 0)] {
        let start = || if full { installed_full() } else { installed() };
        let ops = {
            let f = start();
            let generation = Log::load(&mut f.copy(), &LOG).unwrap().generation;
            write_in_update_mode(&f, slot, &app(slot)).expect("written");
            let after = Log::load(&mut f.copy(), &LOG).unwrap().generation;
            assert_eq!(after > generation, full, "the full log is compacted first");
            let n = *f.ops.borrow();
            n
        };
        assert!(ops > 10, "{ops}");
        for n in 0..ops {
            for half in [false, true] {
                let mut f = start();
                f.cut = Some((n, half));
                let _ = write_in_update_mode(&f, slot, &app(slot));
                cuts += 1;
                let site = f.site(n).expect("operation n was asked for");
                covered.add(site, Way { half, keep: false });
                if let Err(why) = recoverable(&f, slot) {
                    unrecoverable
                        .push(format!("full={full} slot={slot} op={n} half={half}: {why}"));
                }
            }
        }
    }
    println!(
        "power cuts: {cuts}, unrecoverable states: {}",
        unrecoverable.len()
    );
    assert!(unrecoverable.is_empty(), "{unrecoverable:#?}");
    let mut expected = compaction_sites(&[
        (target::BOOTCORE, kind::SNAPSHOT),
        (target::APP, kind::SNAPSHOT),
    ]);
    for slot in [0, 1] {
        expected.extend(slot_sites(target::APP, slot));
    }
    expected.push(record(target::APP, kind::SLOT_PENDING));
    covered.check(&expected, &expected, &POWER_LOSS);
    Coverage::report(
        "update-mode",
        &format!(
            "## ブートローダの更新モードでのアプリの書込\n\n\
             開始状態: 初回書込の直後、およびログの面の残量が試行の予約量未満（書込の開始時に圧縮）。\
             書込先: APP スロット A と B。電断 {cuts} 件、復旧失敗 0 件。\n\n{}",
            covered.table(&POWER_LOSS)
        ),
    );
}
