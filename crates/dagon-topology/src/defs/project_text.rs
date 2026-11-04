//! A project's own definitions are made safe once, on load, for every
//! consumer (terminal, page, AI, check messages): descriptive words (names,
//! products, labels, connection copy, doctor names and hints) that are not
//! [`page_safe`] are stored as [`PROJECT_TEXT`]; identifiers and writer
//! parameters other text names (ids, flash maps, pins, controllers, part
//! names, the UF2 drive's info file, header and Board-ID, probe-rs and
//! espflash chip names) must be `page_safe` or the definition does not
//! load. File paths and doctor commands stay as written: they are never
//! put into text that leaves the terminal.

use super::{page_safe, BoardDef, I2cPair, McuDef, Writer, PROJECT_TEXT};

fn scrub(s: &mut String) {
    if !page_safe(s) {
        *s = PROJECT_TEXT.into();
    }
}

fn plain(what: &str, s: &str) -> Result<(), String> {
    if page_safe(s) {
        Ok(())
    } else {
        Err(format!(
            "{what} must be at most 40 characters of A-Z a-z 0-9, space, '.', '_', '-'"
        ))
    }
}

fn plain_pairs(what: &str, pairs: &[I2cPair]) -> Result<(), String> {
    pairs.iter().try_for_each(|p| {
        plain(&format!("{what}.controller"), &p.controller)?;
        plain(&format!("{what}.sda"), &p.sda)?;
        plain(&format!("{what}.scl"), &p.scl)
    })
}

/// A project MCU definition, made safe (see the module).
pub(super) fn mcu(m: &mut McuDef) -> Result<(), String> {
    plain("id", &m.id)?;
    for f in &m.flash_maps {
        plain("flash_map.name", &f.name)?;
    }
    for p in &m.parts {
        plain("part.name", &p.name)?;
    }
    match &m.writer {
        Writer::Uf2Drive {
            info_file,
            info_header,
            board_id,
            ..
        } => {
            plain("writer.info_file", info_file)?;
            plain("writer.info_header", info_header)?;
            plain("writer.board_id", board_id)?;
        }
        Writer::ProbeRs { chip } => plain("writer.chip", chip)?,
        Writer::Espflash {
            chip, flash_size, ..
        } => {
            plain("writer.chip", chip)?;
            plain("writer.flash_size", flash_size)?;
        }
    }
    if let Some(r) = &mut m.rom_i2c {
        plain_pairs("rom_i2c.pairs", &r.pairs)?;
        scrub(&mut r.name);
    }
    scrub(&mut m.name);
    scrub(&mut m.connection.label);
    scrub(&mut m.connection.hint);
    for d in &mut m.doctor {
        scrub(&mut d.what);
        scrub(&mut d.hint);
    }
    Ok(())
}

/// A project board definition, made safe (see the module).
pub(super) fn board(b: &mut BoardDef) -> Result<(), String> {
    plain("id", &b.id)?;
    plain("mcu", &b.mcu)?;
    plain_pairs("i2c", &b.i2c_pairs)?;
    if let Some(c) = &b.coordinator {
        plain("coordinator.i2c.controller", &c.i2c.controller)?;
    }
    for h in &mut b.header {
        plain("header.pin", &h.pin)?;
        if let Some(l) = &mut h.label {
            scrub(l);
        }
    }
    scrub(&mut b.product);
    scrub(&mut b.drawing);
    Ok(())
}
