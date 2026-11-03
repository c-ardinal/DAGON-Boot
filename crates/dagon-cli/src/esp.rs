//! ESP32-S3 helpers for `flash` and `deploy`: the app ELF turned into an
//! ESP image (espflash save-image), the bundled unmodified ESP-IDF 2nd stage
//! checked against its SHA256SUMS, the partition offsets, the first otadata
//! entry, and the one USB Serial/JTAG port a write may use.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The bundled 2nd stage and the partition table, in the DAGON checkout.
pub(crate) const VENDOR_DIR: &str = "targets/mcu/esp32s3/vendor";
pub(crate) const PARTITIONS: &str = "targets/mcu/esp32s3/partitions.csv";
const BOOTLOADER: &str = "bootloader.bin";
/// The flash size the bundled 2nd stage was built for (its image header):
/// every image and every write uses it.
pub(crate) const FLASH_SIZE: &str = "16mb";
pub(crate) const CHIP: &str = "esp32s3";
/// Where the 2nd stage reads the partition table (its sdkconfig).
pub(crate) const TABLE_AT: u32 = 0x8000;
/// Espressif's USB VID and the ESP32-S3's USB Serial/JTAG PID.
pub(crate) const USB_JTAG: (u16, u16) = (0x303A, 0x1001);
/// otadata `ota_state` Valid.
const OTA_VALID: u32 = 2;

/// Offsets of the partitions a first write fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Layout {
    pub otadata: u32,
    pub factory: u32,
    pub ota_0: u32,
    pub dagon: u32,
}

fn number(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u32::from_str_radix(h, 16).ok();
    }
    s.parse().ok()
}

/// The offsets in a partition table CSV (`# Name, Type, SubType, Offset,
/// Size`); every partition the first write uses must be there.
pub(crate) fn layout(csv: &str) -> Result<Layout, String> {
    let offset = |name: &str| {
        csv.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.split(',').map(str::trim).collect::<Vec<_>>())
            .find(|f| f.first() == Some(&name))
            .and_then(|f| f.get(3).and_then(|o| number(o)))
            .ok_or_else(|| format!("the partition table has no offset for {name}"))
    };
    Ok(Layout {
        otadata: offset("otadata")?,
        factory: offset("factory")?,
        ota_0: offset("ota_0")?,
        dagon: offset("dagon")?,
    })
}

/// The bundled 2nd stage, only when its SHA-256 is the one SHA256SUMS
/// lists for it (the unmodified ESP-IDF build).
pub(crate) fn checked_bootloader(vendor: &Path) -> Result<Vec<u8>, String> {
    let sums = std::fs::read_to_string(vendor.join("SHA256SUMS"))
        .map_err(|e| format!("{}: {e}", vendor.join("SHA256SUMS").display()))?;
    let want = sums
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim().trim_start_matches('*') == BOOTLOADER)
        .map(|(sum, _)| sum.trim().to_ascii_lowercase())
        .ok_or("SHA256SUMS lists no bootloader.bin")?;
    let path = vendor.join(BOOTLOADER);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let got = dagon_sign::to_hex(&dagon_format::sha256(&bytes));
    if got != want {
        return Err(format!(
            "{} is not the bundled ESP-IDF 2nd stage (sha256 {got}, SHA256SUMS {want}); \
             nothing is written",
            path.display()
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
pub(crate) fn save_image(elf: &Path, out: &Path) -> Result<Vec<u8>, String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let args = save_image_args(elf, out);
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

pub(crate) fn save_image_args(elf: &Path, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["save-image", "--chip", CHIP, "--flash-size", FLASH_SIZE]
        .map(String::from)
        .to_vec();
    a.push(elf.display().to_string());
    a.push(out.display().to_string());
    a
}

/// The one ESP32-S3 USB Serial/JTAG port among `ports` (name, VID, PID).
/// None or several: an error, and nothing is written.
pub(crate) fn pick_port(ports: &[(String, u16, u16)]) -> Result<String, String> {
    let found: Vec<&String> = ports
        .iter()
        .filter(|(_, v, p)| (*v, *p) == USB_JTAG)
        .map(|(n, _, _)| n)
        .collect();
    match found[..] {
        [one] => Ok(one.clone()),
        [] => Err(format!(
            "no ESP32-S3 on USB (USB Serial/JTAG {:04x}:{:04x}); connect it with USB-C \
             (hold BOOT while plugging in if it does not show up). Nothing was written.",
            USB_JTAG.0, USB_JTAG.1
        )),
        _ => Err(format!(
            "{} ESP32-S3 boards are connected ({}); unplug every board but the one to \
             flash, then run flash again. Nothing was written.",
            found.len(),
            found
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Serial ports with their USB IDs (non-USB ports are left out).
pub(crate) fn usb_ports() -> Result<Vec<(String, u16, u16)>, String> {
    let ports = serialport::available_ports().map_err(|e| format!("cannot list ports: {e}"))?;
    Ok(ports
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) => Some((p.port_name, u.vid, u.pid)),
            _ => None,
        })
        .collect())
}

/// The bundled 2nd stage's directory and the partition table of `home`.
pub(crate) fn vendor_files(home: &Path) -> (PathBuf, PathBuf) {
    (home.join(VENDOR_DIR), home.join(PARTITIONS))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = include_str!("../../../targets/mcu/esp32s3/partitions.csv");

    #[test]
    fn the_layout_comes_from_the_partition_table_and_matches_signing() {
        let l = layout(CSV).unwrap();
        assert_eq!(
            l,
            Layout {
                otadata: 0xD000,
                factory: 0x1_0000,
                ota_0: 0x11_0000,
                dagon: 0x71_0000,
            }
        );
        use dagon_format::header::{image_kind, slot_binding};
        let s = dagon_sign::image::slot(CHIP, image_kind::APP, slot_binding::A).unwrap();
        assert_eq!((s.load_addr, s.header_addr), (l.ota_0, l.dagon));
        assert!(layout("factory, app, factory, 0x10000, 1M\n").is_err());
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
        let real = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(VENDOR_DIR);
        assert!(checked_bootloader(&real).is_ok());

        let d = std::env::temp_dir().join(format!("dagon-esp-bl-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(real.join("SHA256SUMS"), d.join("SHA256SUMS")).unwrap();
        let mut bl = std::fs::read(real.join(BOOTLOADER)).unwrap();
        bl[100] ^= 1;
        std::fs::write(d.join(BOOTLOADER), &bl).unwrap();
        let e = checked_bootloader(&d).unwrap_err();
        assert!(e.contains("not the bundled ESP-IDF 2nd stage"), "{e}");
        std::fs::write(d.join("SHA256SUMS"), "00  other.bin\n").unwrap();
        assert!(checked_bootloader(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn writes_only_to_exactly_one_esp32s3() {
        let esp = |n: &str| (n.to_string(), 0x303A, 0x1001);
        let other = ("COM5".to_string(), 0xC0DE, 0xCAFE);
        assert_eq!(pick_port(&[other.clone(), esp("COM7")]).unwrap(), "COM7");
        let none = pick_port(std::slice::from_ref(&other)).unwrap_err();
        assert!(none.contains("no ESP32-S3") && none.contains("Nothing was written"));
        let two = pick_port(&[esp("COM7"), other, esp("COM9")]).unwrap_err();
        assert!(
            two.contains("2 ESP32-S3 boards") && two.contains("COM9"),
            "{two}"
        );
    }
}
