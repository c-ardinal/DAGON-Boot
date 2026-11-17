//! A board's `[flash]` reaches its firmware through generated code: the
//! erase regions a board package's build writes are the loader's
//! effective regions for that MCU and board (`McuDef::erase_regions_on`),
//! and the ESP32-S3 package's test builds its store from the same file
//! (`targets/mcu/esp32s3/tests/fixtures/flash_board_def.rs`).

use dagon_topology::defs::codegen::erase_regions_rs;
use dagon_topology::defs::{Registry, Source};

/// A synthetic ESP32-S3 board carrying a slower 4 KiB-sector part.
const BOARD: &str = "format = 1\n[board.slow-s3]\nformat = 3\nproduct = \"Slow S3\"\nmcu = \"esp32s3\"\n\
                     flash_bytes = 0x100_0000\ndrawing = \"pico\"\n[board.slow-s3.flash]\npart = \"X\"\n\
                     erase_regions = [{ start = 0x0, len = 0x100_0000, unit = 0x1000, unit_max_ms = 900 }]\n";

const FIXTURE: &str =
    include_str!("../../../targets/mcu/esp32s3/tests/fixtures/flash_board_def.rs");

fn registry() -> Registry {
    let dir = std::env::temp_dir().join(format!("dagon-board-flash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dagon.toml"), BOARD).unwrap();
    let r = Registry::load(&[Source::Project(&dir.join("dagon.toml"))]);
    std::fs::remove_dir_all(dir).unwrap();
    r.unwrap()
}

#[test]
fn the_generated_regions_are_the_loaders_effective_regions() {
    let reg = registry();
    let m = reg.mcu("esp32s3").unwrap();
    let b = reg.board("slow-s3").unwrap();
    let effective = m.erase_regions_on(Some(b)).unwrap();
    // The board's part replaces the MCU's default.
    assert_ne!(Some(effective), m.erase_regions_on(None));
    let rs = erase_regions_rs(effective);
    assert_eq!(rs, FIXTURE.replace("\r\n", "\n"));
    // What the generated rows say is the layout the host plans from.
    let layout = m.erase_layout_on(Some(b)).unwrap();
    let rows: Vec<(u32, u32, u32, u32)> = layout
        .regions
        .iter()
        .map(|r| (r.start, r.len, r.unit, r.unit_max_ms))
        .collect();
    assert_eq!(rows, [(0x0, 0x100_0000, 0x1000, 900)]);
    // A board without [flash] gets its MCU's regions generated the same way.
    let plain = reg.board("m5stopwatch").unwrap();
    assert_eq!(
        erase_regions_rs(m.erase_regions_on(Some(plain)).unwrap()),
        erase_regions_rs(m.erase_regions_on(None).unwrap())
    );
}
