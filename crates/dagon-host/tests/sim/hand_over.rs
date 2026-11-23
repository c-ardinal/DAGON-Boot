//! Nodes whose MCU definition gives the app one slot: the app never writes
//! the slot it runs from, so the host has it hand over to its boot core
//! (REBOOT mode 1), which writes the slot in update mode; a node with no
//! app to start answers in recovery mode and is written the same way.

use super::*;
use dagon_proto::payload::mode;
use dagon_sim::{f401_config, hub_config, Layout, Run, SimFlash, SimNode};

const V1: Version = Version::new(1, 0, 0);
const V2: Version = Version::new(1, 1, 0);

/// The seg's and the hub's layouts with app slot A only.
const SEG_ONE: Layout = Layout {
    slots: &[(0x0802_0000, 0x2_0000)],
    ..Layout::STM32F401XE
};
const HUB_ONE: Layout = Layout {
    slots: &[(0x1002_0000, 0x8_0000)],
    ..Layout::RP2040_HUB
};

/// Hub and seg, each with one app slot, running 1.0.0 from it.
fn one_slot_sim() -> Sim {
    let mut hub = SimNode::new(hub_config(), *key().pk, SimFlash::rp2040_2m(7), HUB_ONE);
    hub.provision(&hub_img(0, V1), 0).unwrap();
    let mut seg = SimNode::new(f401_config(), *key().pk, SimFlash::stm32f401xe(6), SEG_ONE);
    seg.i2c_addr = Some(NODE_I2C_ADDR);
    seg.provision(&seg_img(0, V1), 0).unwrap();
    let mut relay = SimRelay::new(hub);
    relay.add_i2c(NODE_I2C_PORT, seg, true);
    sim_of(relay)
}

/// The built-in definitions with app slot B of both MCUs left out, as a
/// project overrides them.
fn one_slot_defs() -> dagon_topology::defs::Registry {
    use dagon_topology::defs::{Registry, Source};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("dagon-host-one-slot-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let toml = dir.join("dagon.toml");
    let mut over = String::from("format = 1\n");
    for mcu in ["rp2040", "stm32f401re"] {
        over += &format!(
            "[[mcu_override.{mcu}.slot]]\nimage = \"app\"\nbinding = \"b\"\ndagon-remove = true\n\
             [[mcu_override.{mcu}.slot_layout]]\ntarget = \"app\"\nslot = \"b\"\ndagon-remove = true\n"
        );
    }
    std::fs::write(&toml, over).unwrap();
    let r = Registry::load(&[Source::Project(&toml)]);
    std::fs::remove_dir_all(dir).unwrap();
    r.unwrap()
}

/// A release with app slot A of the seg and the hub at `v`.
fn one_slot_release(v: Version) -> Vec<u8> {
    let t = dagon_topology::parse_str(&yaml()).unwrap();
    let images = [seg_img(0, v), hub_img(0, v)];
    dagon_sign::release::build(
        &one_slot_defs(),
        &t,
        &topology(&yaml()),
        &images,
        "r",
        &key(),
    )
    .unwrap()
    .bundle
}

fn apply_one(s: &Sim, node: u8, v: Version) -> Outcome<()> {
    block_on(host(s).apply(
        &one_slot_release(v),
        &topology(&yaml()),
        &key().pk,
        Some(&[node]),
        &mut |_| {},
    ))
    .unwrap()
    .remove(0)
    .outcome
}

fn with_seg<R>(s: &Sim, f: impl FnOnce(&mut SimNode, u64) -> R) -> R {
    let mut hub = s.relay.borrow_mut();
    let now = hub.now_ms();
    f(hub.i2c_node(NODE_I2C_PORT, NODE_I2C_ADDR).unwrap(), now)
}

/// The REBOOT requests the host sent, by mode (the inner one for RELAY).
fn reboots(s: &Sim) -> usize {
    let f = s.faults.borrow();
    f.seen.iter().filter(|c| c.0 == Cmd::Reboot as u8).count()
}

/// The app slot of `to`, confirmed at `v` and running.
fn assert_runs_confirmed(s: &Sim, to: Target, v: Version) {
    let st = block_on(host(s).status(to)).unwrap();
    assert_eq!(st.hello.mode, mode::APP, "{to:?}");
    assert_eq!((st.status.active, st.status.confirmed), (0, 0), "{to:?}");
    assert_eq!(
        (st.status.slots[0].state, st.status.slots[0].version),
        (SlotState::Confirmed, v),
        "{to:?}"
    );
}

#[test]
fn the_one_slot_seg_hands_over_and_its_boot_core_writes_the_app() {
    let s = one_slot_sim();
    let out = apply_one(&s, SEG, V2);
    assert!(matches!(out, Outcome::Done(x) if x == V2), "{out:?}");
    // REBOOT mode 1 to hand over, then mode 0 to try the new app.
    assert_eq!(reboots(&s), 2);
    assert_runs_confirmed(&s, seg_at(), V2);
    with_seg(&s, |seg, now| {
        seg.reset(now);
        assert_eq!(seg.run(), Run::App(0), "no stay left to take");
    });
}

#[test]
fn the_one_slot_hub_hands_over_on_usb_and_its_boot_core_writes_the_app() {
    let s = one_slot_sim();
    let out = apply_one(&s, 0, V2);
    assert!(matches!(out, Outcome::Done(x) if x == V2), "{out:?}");
    assert_eq!(reboots(&s), 2);
    assert_runs_confirmed(&s, Target::Local, V2);
}

/// The same version again is not written: nothing hands over.
#[test]
fn an_unchanged_one_slot_app_is_not_handed_over() {
    let s = one_slot_sim();
    let out = apply_one(&s, SEG, V1);
    assert!(matches!(out, Outcome::Done(x) if x == V1), "{out:?}");
    assert_eq!(reboots(&s), 0);
}

fn recover_assets<'a>(release: &'a [u8], topo: &'a [u8], pk: &'a [u8; 32]) -> RecoverAssets<'a> {
    RecoverAssets {
        release,
        topology: topo,
        pubkey: pk,
        bootcore: None,
        stage0: None,
        roms: &[],
        rom_confirmed: false,
    }
}

/// A new one-slot app that never starts leaves nothing to go back to: the
/// boot core answers in recovery mode, and the recovery writes a good app
/// through it.
#[test]
fn a_one_slot_app_that_never_starts_is_recovered_through_the_boot_core() {
    let s = one_slot_sim();
    with_seg(&s, |seg, _| seg.trial_app = TrialApp::CrashesAtBoot);
    let out = apply_one(&s, SEG, V2);
    assert!(matches!(out, Outcome::Failed(_)), "{out:?}");
    with_seg(&s, |seg, _| {
        assert_eq!(seg.run(), Run::BootCore(mode::RECOVERY));
        seg.trial_app = TrialApp::Confirms;
    });
    let v3 = Version::new(1, 2, 0);
    let (rel, topo, pk) = (one_slot_release(v3), topology(&yaml()), *key().pk);
    let (path, v) =
        block_on(host(&s).recover(SEG, &recover_assets(&rel, &topo, &pk), &mut |_| {})).unwrap();
    assert_eq!((path, v), (RecoverPath::Retransfer, v3));
    assert_runs_confirmed(&s, seg_at(), v3);
}

/// Both app slots of a two-slot seg broken: its boot core answers in
/// recovery mode and the console writes the app through it.
#[test]
fn a_seg_with_both_app_slots_broken_is_recovered_through_the_boot_core() {
    use dagon_port::Flash;
    let s = sim();
    with_seg(&s, |seg, now| {
        for &(at, len) in Layout::STM32F401XE.slots {
            seg.flash().erase(at, len).unwrap();
        }
        seg.reset(now);
        assert_eq!(seg.run(), Run::BootCore(mode::RECOVERY));
    });
    let (rel, topo, pk) = (release(), topology(&yaml()), *key().pk);
    let (path, v) =
        block_on(host(&s).recover(SEG, &recover_assets(&rel, &topo, &pk), &mut |_| {})).unwrap();
    assert_eq!((path, v), (RecoverPath::Retransfer, V2));
    let st = block_on(host(&s).status(seg_at())).unwrap();
    assert_eq!(st.hello.mode, mode::APP);
    assert_eq!(st.hello.active_version, V2);
    assert!(!st.hello.trial);
}

/// A power cut at every flash operation of the seg's one-slot update:
/// after power returns the seg answers (its app, or its boot core in
/// update or recovery mode) and the same release then completes.
#[test]
fn a_power_cut_at_every_step_of_a_one_slot_update_leaves_the_seg_writable() {
    let ops = {
        let s = one_slot_sim();
        with_seg(&s, |seg, _| seg.flash().clear_ops());
        let out = apply_one(&s, SEG, V2);
        assert!(matches!(out, Outcome::Done(_)), "{out:?}");
        with_seg(&s, |seg, _| seg.flash().ops().len())
    };
    assert!(ops > 8, "{ops}");
    for i in 0..ops {
        for cut in [
            dagon_sim::CutPoint::During(i),
            dagon_sim::CutPoint::After(i),
        ] {
            let s = one_slot_sim();
            with_seg(&s, |seg, _| {
                let mut f = seg.flash();
                f.clear_ops();
                f.arm_cut(cut);
            });
            let _ = apply_one(&s, SEG, V2);
            with_seg(&s, |seg, now| {
                if !seg.flash().is_powered() {
                    seg.power_on(now);
                }
                assert!(
                    matches!(seg.run(), Run::App(0) | Run::BootCore(_)),
                    "{cut:?}: {:?}",
                    seg.run()
                );
            });
            let out = apply_one(&s, SEG, V2);
            assert!(
                matches!(out, Outcome::Done(x) if x == V2),
                "{cut:?}: {out:?}"
            );
            assert_runs_confirmed(&s, seg_at(), V2);
        }
    }
}
