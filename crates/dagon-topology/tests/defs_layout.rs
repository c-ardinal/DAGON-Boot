//! Each MCU definition agrees with the MCU package's own flash layout
//! (`targets/mcu/<id>/src/layout.rs`) and with the memory.x its build.rs
//! writes from it (FLASH = the image body each slot leaves after its header).

#![allow(dead_code)]

use dagon_format::header::{image_kind, slot_binding};
use dagon_topology::defs::{McuDef, Registry, Slot};

#[path = "../../../targets/mcu/esp32s3/src/layout.rs"]
mod esp32s3;
#[path = "../../../targets/mcu/rp2040/src/layout.rs"]
mod rp2040;
#[path = "../../../targets/mcu/stm32f401re/src/layout.rs"]
mod stm32f401re;

fn mcu(id: &str) -> &'static McuDef {
    Registry::builtin().mcu(id).unwrap()
}

fn slot(mcu: &str, kind: u8, binding: u8) -> &'static Slot {
    Registry::builtin().slot(mcu, kind, binding).unwrap()
}

/// memory.x FLASH (origin, length) of an image in `s`.
fn flash(s: &Slot) -> (u32, u32) {
    (s.load_addr, s.max_payload)
}

#[test]
fn rp2040_matches_its_layout_and_memory_x() {
    use rp2040::*;
    let m = mcu("rp2040");
    let map = m.default_flash_map();
    assert_eq!(map.base, XIP_BASE);
    assert!(map.bytes as usize >= MIN_FLASH_SIZE);
    let stage0 = m.layout.stage0.unwrap();
    assert_eq!((stage0.at, stage0.size), (STAGE0, STAGE0_SIZE));
    let bl = m.layout.bootcore_log.unwrap();
    assert_eq!((bl.faces, bl.face_size), (BL_LOG, LOG_FACE_SIZE));
    let app = m.layout.app_log.unwrap();
    assert_eq!((app.faces, app.face_size), (APP_LOG, LOG_FACE_SIZE));
    assert_eq!(app.faces, APP_LOG_LAYOUT.faces);
    for (i, b) in [slot_binding::A, slot_binding::B].into_iter().enumerate() {
        let bc = slot("rp2040", image_kind::BOOTCORE, b);
        assert_eq!((bc.start, bc.size), (BL_SLOT[i], BL_SLOT_SIZE));
        let a = slot("rp2040", image_kind::APP, b);
        assert_eq!((a.start, a.size), (APP_SLOT[i], APP_SLOT_SIZE));
        assert_eq!((a.start, a.size), (APP_SLOTS.slots[i], APP_SLOTS.slot_len));
        // build.rs: body after the header, HEADER_LEN shorter than the slot.
        assert_eq!(
            flash(bc),
            (BL_SLOT[i] + HEADER_LEN, body_capacity(BL_SLOT_SIZE))
        );
        assert_eq!(
            flash(a),
            (APP_SLOT[i] + HEADER_LEN, body_capacity(APP_SLOT_SIZE))
        );
    }
    // The topology store lies outside every region the definition claims.
    for s in &m.slots {
        assert!(TOPOLOGY >= s.start + s.size || TOPOLOGY + TOPOLOGY_SIZE <= s.start);
    }
    assert!(TOPOLOGY + TOPOLOGY_SIZE <= map.base + map.bytes);
}

#[test]
fn stm32f401re_matches_its_layout_and_memory_x() {
    use stm32f401re::*;
    let m = mcu("stm32f401re");
    let map = m.default_flash_map();
    assert_eq!(m.wire_id, MCU_ID);
    assert_eq!(map.base, BOOTCORE);
    assert!(map.bytes as usize >= MIN_FLASH_SIZE);
    let log = m.layout.app_log.unwrap();
    assert_eq!((log.faces, log.face_size), (LOG.faces, LOG.face_len));
    let bc = slot("stm32f401re", image_kind::BOOTCORE, slot_binding::A);
    assert_eq!(bc.header_addr, BOOTCORE_TRAILER);
    // build.rs: sectors 0-1 up to the trailer.
    assert_eq!(flash(bc), (BOOTCORE, BOOTCORE_TRAILER - BOOTCORE));
    for (i, b) in [slot_binding::A, slot_binding::B].into_iter().enumerate() {
        let a = slot("stm32f401re", image_kind::APP, b);
        assert_eq!((a.start, a.size), (SLOTS.slots[i], SLOTS.slot_len));
        assert_eq!(
            flash(a),
            (SLOTS.slots[i] + HEADER_LEN, SLOTS.slot_len - HEADER_LEN)
        );
    }
    // The port's writable regions are exactly the log faces and APP slots.
    assert_eq!(FLASH_REGIONS[0].start, log.faces[0]);
    assert_eq!(FLASH_REGIONS[0].len, 2 * log.face_size);
    assert_eq!(FLASH_REGIONS[1].start, SLOTS.slots[0]);
}

#[test]
fn esp32s3_matches_its_layout() {
    use esp32s3::*;
    let m = mcu("esp32s3");
    assert!(m.default_flash_map().bytes >= MIN_FLASH_SIZE);
    let table: Vec<(&str, u32, u32)> = m
        .partition_table
        .iter()
        .map(|p| (p.name.as_str(), p.offset, p.size))
        .collect();
    let want: Vec<(&str, u32, u32)> = PARTITIONS
        .iter()
        .map(|p| (p.name, p.offset, p.size))
        .collect();
    assert_eq!(table, want);
    let app = slot("esp32s3", image_kind::APP, slot_binding::A);
    assert_eq!((app.start, app.size), (OTA_0.offset, OTA_0.size));
    let agent = slot("esp32s3", image_kind::AGENT, slot_binding::A);
    assert_eq!((agent.start, agent.size), (FACTORY.offset, FACTORY.size));
    for s in [app, agent] {
        // Detached headers sit in the dagon partition, writable by DAGON.
        assert!(region_for(s.header_addr, 512).is_some_and(|r| r.start == DAGON.offset));
    }
}
