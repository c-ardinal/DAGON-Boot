//! Helpers for MCUs written by espflash (`flash` and `deploy`): the app ELF
//! turned into an ESP image (espflash save-image), the bundled unmodified
//! 2nd stage checked against its checksums, the partitions a first write
//! fills and the first otadata entry. Chip, flash size, table offset and
//! the vendor files come from the MCU definition's `espflash` writer.

use std::path::{Path, PathBuf};
use std::process::Command;

use dagon_topology::defs::{Binding, ImageKind, McuDef, Writer};

/// otadata `ota_state` Valid.
const OTA_VALID: u32 = 2;
/// The ESP-IDF partition that selects the OTA slot to boot.
pub(crate) const OTADATA: &str = "otadata";
/// Where espflash puts the 2nd stage (shown in the plan only; espflash
/// places it itself).
// ponytail: true for the ESP32-S3 (and C3/C6/S2-class chips), not the
// classic ESP32 (0x1000); move to the writer definition when one is added.
pub(crate) const BOOTLOADER_AT: u32 = 0x0;

/// An MCU definition's `espflash` writer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Esp<'a> {
    pub chip: &'a str,
    /// The flash size the bundled 2nd stage was built for (its image
    /// header): every image and every write uses it.
    pub flash_size: &'a str,
    /// Where the 2nd stage reads the partition table (its sdkconfig).
    pub table_at: u32,
    /// Package-relative paths of the 2nd stage and its checksums.
    pub bootloader: &'a str,
    pub checksums: &'a str,
}

impl<'a> Esp<'a> {
    /// `mcu`'s writer when it is `espflash`.
    pub(crate) fn of(mcu: &'a McuDef) -> Option<Esp<'a>> {
        match &mcu.writer {
            Writer::Espflash {
                chip,
                flash_size,
                table_at,
                bootloader,
                checksums,
                ..
            } => Some(Esp {
                chip,
                flash_size,
                table_at: *table_at,
                bootloader,
                checksums,
            }),
            _ => None,
        }
    }
}

/// The partitions a first write fills, by name and offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub otadata: u32,
    /// The recovery agent's partition.
    pub factory: (String, u32),
    /// The app's slot A partition.
    pub ota_0: (String, u32),
    /// The partition holding the app's detached header, and the header's
    /// absolute address.
    pub dagon: (String, u32),
}

/// The first-write partitions of `mcu`, from its slots (app slot A, the
/// agent) and its partition table (otadata).
pub(crate) fn layout(mcu: &McuDef) -> Result<Layout, String> {
    let slot = |image: ImageKind| {
        mcu.slot_defs
            .iter()
            .zip(&mcu.slots)
            .find(|(d, _)| d.image == image && matches!(d.binding, Binding::A | Binding::Any))
            .ok_or_else(|| format!("{} has no {image:?} slot A", mcu.id))
    };
    let partition = |d: &dagon_topology::defs::SlotDef, what: &str| {
        d.partition
            .clone()
            .ok_or_else(|| format!("{}: the {what} slot names no partition", mcu.id))
    };
    let (app_def, app) = slot(ImageKind::App)?;
    let (agent_def, agent) = slot(ImageKind::Agent)?;
    let header = app_def
        .header_partition
        .clone()
        .ok_or_else(|| format!("{}: the app header names no partition", mcu.id))?;
    let otadata = mcu
        .partition_table
        .iter()
        .find(|p| p.name == OTADATA)
        .ok_or_else(|| format!("the partition table has no offset for {OTADATA}"))?
        .offset;
    Ok(Layout {
        otadata,
        factory: (partition(agent_def, "agent")?, agent.start),
        ota_0: (partition(app_def, "app")?, app.start),
        dagon: (header, app.header_addr),
    })
}

/// The bundled 2nd stage at `bootloader`, only when its SHA-256 is the one
/// `sums` lists for its file name (the unmodified ESP-IDF build).
pub(crate) fn checked_bootloader(bootloader: &Path, sums: &Path) -> Result<Vec<u8>, String> {
    let list = std::fs::read_to_string(sums).map_err(|e| format!("{}: {e}", sums.display()))?;
    let name = bootloader
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let want = list
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, n)| n.trim().trim_start_matches('*') == name)
        .map(|(sum, _)| sum.trim().to_ascii_lowercase())
        .ok_or_else(|| format!("{} lists no {name}", sums.display()))?;
    let bytes = std::fs::read(bootloader).map_err(|e| format!("{}: {e}", bootloader.display()))?;
    let got = dagon_sign::to_hex(&dagon_format::sha256(&bytes));
    if got != want {
        return Err(format!(
            "{} is not the bundled ESP-IDF 2nd stage (sha256 {got}, {} {want}); \
             nothing is written",
            bootloader.display(),
            sums.display()
        ));
    }
    Ok(bytes)
}

/// CRC of an otadata entry, as the 2nd stage computes it over `ota_seq`
/// (`esp_rom_crc32_le(UINT32_MAX, &seq, 4)`).
fn seq_crc(seq: u32) -> u32 {
    let mut crc = 0u32;
    for b in seq.to_le_bytes() {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
        }
    }
    !crc
}

/// otadata entry 0 selecting ota_0 as Valid (seq 1): the first write starts
/// the app in slot A, confirmed.
pub(crate) fn otadata_slot_a() -> [u8; 32] {
    let mut e = [0xFF; 32];
    e[0..4].copy_from_slice(&1u32.to_le_bytes());
    e[24..28].copy_from_slice(&OTA_VALID.to_le_bytes());
    e[28..32].copy_from_slice(&seq_crc(1).to_le_bytes());
    e
}

/// Turns the app ELF into the ESP image the OTA partitions hold.
pub(crate) fn save_image(esp: &Esp<'_>, elf: &Path, out: &Path) -> Result<Vec<u8>, String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let args = save_image_args(esp, elf, out);
    let o = Command::new("espflash")
        .args(&args)
        .output()
        .map_err(|e| format!("cannot run espflash ({e}); see `dagon doctor`"))?;
    if !o.status.success() {
        return Err(format!(
            "espflash save-image failed for {}: {}",
            elf.display(),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    std::fs::read(out).map_err(|e| format!("{}: {e}", out.display()))
}

pub(crate) fn save_image_args(esp: &Esp<'_>, elf: &Path, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "save-image",
        "--chip",
        esp.chip,
        "--flash-size",
        esp.flash_size,
    ]
    .map(String::from)
    .to_vec();
    a.push(elf.display().to_string());
    a.push(out.display().to_string());
    a
}

/// The bundled 2nd stage, its checksums and the partition table of `mcu`,
/// whose package is `pkg`.
pub(crate) fn vendor_files(
    mcu: &McuDef,
    esp: &Esp<'_>,
    pkg: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let table = mcu
        .partitions
        .as_deref()
        .ok_or_else(|| format!("{} has no partition table", mcu.id))?;
    Ok((
        pkg.join(esp.bootloader),
        pkg.join(esp.checksums),
        pkg.join(table),
    ))
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use dagon_topology::defs::Registry;

    fn s3() -> &'static McuDef {
        Registry::builtin().mcu("esp32s3").unwrap()
    }

    #[test]
    fn the_layout_comes_from_the_definition_and_matches_signing() {
        let l = layout(s3()).unwrap();
        assert_eq!(
            l,
            Layout {
                otadata: 0xD000,
                factory: ("factory".into(), 0x1_0000),
                ota_0: ("ota_0".into(), 0x11_0000),
                dagon: ("dagon".into(), 0x71_0000),
            }
        );
        use dagon_format::header::{image_kind, slot_binding};
        let s = dagon_sign::image::slot("esp32s3", image_kind::APP, slot_binding::A).unwrap();
        assert_eq!((s.load_addr, s.header_addr), (l.ota_0.1, l.dagon.1));
        assert!(layout(Registry::builtin().mcu("rp2040").unwrap()).is_err());
    }

    #[test]
    fn the_writer_parameters_are_the_definitions() {
        let e = Esp::of(s3()).unwrap();
        assert_eq!(
            (e.chip, e.flash_size, e.table_at),
            ("esp32s3", "16mb", 0x8000)
        );
        assert!(Esp::of(Registry::builtin().mcu("stm32f401re").unwrap()).is_none());
    }

    #[test]
    fn the_first_otadata_entry_is_ota_0_valid() {
        let e = otadata_slot_a();
        assert_eq!(&e[0..4], &[1, 0, 0, 0]);
        assert!(e[4..24].iter().all(|&b| b == 0xFF));
        assert_eq!(&e[24..28], &[2, 0, 0, 0]);
        // The value the manual procedure writes (its printf), and that the
        // watch's 2nd stage accepted.
        assert_eq!(&e[28..32], &[0x9a, 0x98, 0x43, 0x47]);
    }

    #[test]
    fn only_the_unmodified_bootloader_passes() {
        let pkg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../targets/mcu/esp32s3");
        let esp = Esp::of(s3()).unwrap();
        let (bl, sums, table) = vendor_files(s3(), &esp, &pkg).unwrap();
        assert!(table.is_file());
        assert!(checked_bootloader(&bl, &sums).is_ok());

        let d = std::env::temp_dir().join(format!("dagon-esp-bl-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (bl2, sums2) = (d.join("bootloader.bin"), d.join("SHA256SUMS"));
        std::fs::copy(&sums, &sums2).unwrap();
        let mut bytes = std::fs::read(&bl).unwrap();
        bytes[100] ^= 1;
        std::fs::write(&bl2, &bytes).unwrap();
        let e = checked_bootloader(&bl2, &sums2).unwrap_err();
        assert!(e.contains("not the bundled ESP-IDF 2nd stage"), "{e}");
        std::fs::write(&sums2, "00  other.bin\n").unwrap();
        assert!(checked_bootloader(&bl2, &sums2).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
