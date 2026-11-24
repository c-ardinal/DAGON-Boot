//! One project's overrides of a built-in MCU and board and of a synthetic
//! pair (tests/parity) reach the registry and the topology check, the
//! host's node timings and ROM maps, signing, and the firmware constants
//! generated from the definitions `dagon gen` writes; the tree alone has
//! other values, so each assertion sees the override.

#[path = "parity/mod.rs"]
mod parity;

use dagon_format::header::{image_kind, slot_binding};
use dagon_format::Version;
use dagon_sign::image::{self, sign_image, ImageError, ImageSpec};
use dagon_topology::defs::boot_folder::{board_file_text, mcu_file_text};
use dagon_topology::defs::{codegen, BoardDef, Fact, Registry, Source};
use parity::*;

fn loaded(p: &Project) -> Registry {
    Registry::load(&[Source::Tree(&p.tree), Source::Project(&p.toml)]).unwrap()
}

fn tree_only(p: &Project) -> Registry {
    Registry::load(&[Source::Tree(&p.tree)]).unwrap()
}

fn topology() -> dagon_topology::Topology {
    dagon_topology::parse_str(TOPOLOGY).unwrap()
}

#[test]
fn the_definitions_alone_differ_from_every_overridden_value() {
    let p = project("tree");
    let reg = tree_only(&p);
    assert_eq!(reg.mcu(SYNTHETIC.0).unwrap().wire_id, SYNTHETIC_WIRE_ID);
    for (mcu, board, _) in OVERRIDDEN {
        let m = reg.mcu(mcu).unwrap();
        let b = reg.slot(mcu, image_kind::APP, slot_binding::B).unwrap();
        assert_eq!(b.start, APP_B.0, "{mcu}");
        assert_ne!(b.size, APP_B.1, "{mcu}");
        let w = m.watchdog.as_ref().unwrap();
        assert_ne!((w.trial_ms, w.confirmed_ms), (TRIAL_MS, CONFIRMED_MS));
        let r = m.erase_regions_on(None).unwrap();
        let r = r.iter().find(|r| r.start == ERASE_START).unwrap();
        assert_ne!(r.unit_max_ms, Fact::Known(ERASE_MAX_MS), "{mcu}");
        let rom = m.rom_i2c.as_ref().unwrap();
        assert_ne!(
            (rom.addr, &rom.erase_sectors[..]),
            (ROM_ADDR, &ROM_ERASE[..])
        );
        let b = reg.board(board).unwrap();
        assert_ne!(b.product, PRODUCT);
        assert!(!b.pins.iter().any(|x| x.pin == PIN.0), "{board}");
    }
    // The project is clean against its overridden definitions.
    let report = dagon_topology::check_with(&loaded(&p), &topology());
    assert_eq!(report.errors().count(), 0, "{}", report.to_json());
}

#[test]
fn the_registry_holds_the_overridden_values() {
    let p = project("registry");
    let reg = loaded(&p);
    for (mcu, board, _) in OVERRIDDEN {
        let b = reg.slot(mcu, image_kind::APP, slot_binding::B).unwrap();
        assert_eq!((b.start, b.size), APP_B, "{mcu}");
        let w = reg.mcu(mcu).unwrap().watchdog.as_ref().unwrap();
        assert_eq!((w.trial_ms, w.confirmed_ms), (TRIAL_MS, CONFIRMED_MS));
        let b = reg.board(board).unwrap();
        assert_eq!(b.product, PRODUCT);
        assert!(b
            .pins
            .iter()
            .any(|x| (x.pin.as_str(), x.name.as_deref()) == (PIN.0, Some(PIN.1))));
    }
}

#[test]
fn the_host_times_and_maps_each_node_by_its_overridden_definition() {
    let p = project("host");
    let reg = loaded(&p);
    let t = topology();
    let timings = dagon_host::node_timings(&reg, &t).unwrap();
    let wires = [
        reg.mcu(BUILTIN.0).unwrap().wire_id,
        reg.mcu(SYNTHETIC.0).unwrap().wire_id,
    ];
    let roms = dagon_host::rom::rom_maps(&reg);
    for ((_, _, node), wire) in OVERRIDDEN.into_iter().zip(wires) {
        let i = t.nodes.iter().position(|n| n.id == node).unwrap();
        let (_, nt) = timings
            .iter()
            .find(|(at, _)| usize::from(*at) == i)
            .unwrap();
        assert_eq!(nt.trial_ms, TRIAL_MS, "{node}");
        let erase = nt.erase.as_ref().unwrap();
        let r = erase
            .regions
            .iter()
            .find(|r| r.start == ERASE_START)
            .unwrap();
        assert_eq!(r.unit_max_ms, ERASE_MAX_MS, "{node}");
        let b = erase.slots.iter().find(|s| s.start == APP_B.0).unwrap();
        assert_eq!(b.capacity, APP_B_CAPACITY, "{node}");
        let map = roms.iter().find(|m| m.wire_id == wire).unwrap();
        assert_eq!(
            (map.addr, &map.erase[..]),
            (ROM_ADDR, &ROM_ERASE[..]),
            "{node}"
        );
    }
}

/// The hub's first ROM link reaches an MCU with `[rom_i2c]` but no APP
/// log, so no first install through its loader; the second, one with a
/// full map. `dagon gen` and the host pick the same ROM recovery peer, the
/// second, by the one rule (`McuDef::has_rom_loader`).
#[test]
fn gen_and_the_host_skip_a_rom_peer_without_a_full_map() {
    let p = project("rom-peer");
    let def = p.tree.join("targets/mcu/stm32f401re/dagon-mcu.toml");
    let text = std::fs::read_to_string(&def).unwrap();
    let line = text.lines().find(|l| l.starts_with("app_log = ")).unwrap();
    std::fs::write(&def, text.replace(&format!("{line}\n"), "")).unwrap();
    let reg = loaded(&p);
    assert!(reg.mcu(BUILTIN.0).unwrap().rom_i2c.is_some());
    let t = topology();
    let gen = dagon_topology::recovery_peer_mcu(&reg, &t, "hub");
    assert_eq!(gen.as_deref(), Some(SYNTHETIC.0));
    let roms = dagon_host::rom::rom_maps(&reg);
    let bin = dagon_topology::compile_with(&reg, &t).unwrap();
    let view = dagon_format::topology::TopologyView::parse(&bin).unwrap();
    let hub = (0..view.node_count())
        .find(|&i| view.node(i).mcu_id == reg.mcu("rp2040").unwrap().wire_id)
        .unwrap();
    let peer = view
        .rom_peer(hub as u8, |w| roms.iter().any(|m| m.wire_id == w))
        .unwrap();
    assert_eq!(view.node(usize::from(peer)).mcu_id, SYNTHETIC_WIRE_ID);
}

#[test]
fn signing_takes_the_overridden_slot() {
    let p = project("sign");
    let reg = loaded(&p);
    let made = dagon_sign::generate_to_dir(&p.dir.join("keys")).unwrap();
    let key = dagon_sign::load_key_pair(&made.secret_path).unwrap();
    for (mcu, _, _) in OVERRIDDEN {
        let slot = image::slot_with(&reg, mcu, image_kind::APP, slot_binding::B).unwrap();
        let wide = image::slot_with(&tree_only(&p), mcu, image_kind::APP, slot_binding::B);
        assert_eq!(slot.max_payload, APP_B_CAPACITY, "{mcu}");
        assert!(wide.unwrap().max_payload > APP_B_CAPACITY, "{mcu}");
        let (flags, caps) = image::default_bits(image_kind::APP);
        let spec = ImageSpec {
            image_kind: image_kind::APP,
            port_kind: image::port_kind_of("self-boot").unwrap(),
            mcu_id: reg.mcu(mcu).unwrap().wire_id,
            slot,
            version: Version::new(1, 0, 0),
            build_flags: flags,
            release_seq: 1,
            min_bootcore_version: Version::new(0, 0, 0),
            target_tag: dagon_format::header::target_tag("seg"),
            proto_version: 1,
            capabilities: caps,
        };
        let fits = vec![0xA5; APP_B_CAPACITY as usize];
        assert!(sign_image(&spec, &fits, &key).is_ok(), "{mcu}");
        let over = vec![0xA5; APP_B_CAPACITY as usize + 1];
        assert!(
            matches!(
                sign_image(&spec, &over, &key),
                Err(ImageError::TooLarge { .. })
            ),
            "{mcu}"
        );
    }
}

#[test]
fn the_firmware_constants_come_from_the_overridden_definitions() {
    let p = project("firmware");
    let reg = loaded(&p);
    for (mcu, board, _) in OVERRIDDEN {
        // What `dagon gen` writes into the node's boot folder, as the MCU
        // packages' builds read it.
        let text = mcu_file_text(&reg, mcu).unwrap();
        let rs = dagon_mcu_build::definition_rs(&text);
        for want in [
            format!("pub const TRIAL_WDT_MS: u32 = {TRIAL_MS};"),
            format!("pub const CONFIRMED_WDT_MS: u32 = {CONFIRMED_MS};"),
            format!("({:#x}, {:#x})]", APP_B.0, APP_B.1),
            format!("start: {ERASE_START:#x}, len: 0x60000, unit: 0x20000, unit_max_ms: {ERASE_MAX_MS} }}"),
            format!("capacity: {APP_B_CAPACITY:#x}"),
        ] {
            assert!(rs.contains(&want), "{mcu}: {want}\n{rs}");
        }
        // The board package's build: its pins, from the board's text there.
        let text = board_file_text(&reg, board).unwrap();
        let b: BoardDef = toml::from_str(&text).unwrap();
        let name = |s: &str| s.to_string();
        let hal = codegen::Hal {
            peripherals: "hal::peripherals",
            name: &name,
            lifetime: false,
        };
        let pins = codegen::pins_rs(&b, reg.mcu(mcu).unwrap(), &hal);
        assert!(pins.contains(&format!("$p.{}", PIN.0)), "{board}\n{pins}");
    }
}
