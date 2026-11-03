//! The device a first write goes to. Every device the writer tool reports
//! is a candidate (all probe-rs probes, all serial ports, all UF2 drives);
//! what the MCU definition knows about its device (a USB ID, a UF2
//! Board-ID) only labels candidates, never filters them. Exactly one is
//! taken (still shown before the confirmation); several need the user's
//! choice or a selector; none stops the write.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dagon_topology::defs::{McuDef, UsbHint, Writer};

/// How long a UF2 drive is waited for (the user holds BOOT and plugs in).
pub(crate) const DRIVE_WAIT: Duration = Duration::from_secs(60);
const DRIVE_POLL: Duration = Duration::from_millis(500);
/// How long one volume may take to show its UF2 info file (network
/// drives, empty card readers).
const VOLUME_WAIT: Duration = Duration::from_secs(2);

/// A device the writer can write to: `id` is what the writer command and
/// the selector take, `label` what the user reads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Candidate {
    #[serde(rename = "device")]
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pick {
    Use(Candidate),
    /// Several and no selector: the user chooses.
    Choose(Vec<Candidate>),
}

/// The CLI option that names a device of `w`.
pub(crate) fn flag(w: &Writer) -> &'static str {
    match w {
        Writer::Uf2Drive { .. } => "drive",
        Writer::ProbeRs { .. } => "probe",
        Writer::Espflash { .. } => "port",
    }
}

fn tool(w: &Writer) -> &'static str {
    match w {
        Writer::Uf2Drive { .. } => "the system (UF2 drives)",
        Writer::ProbeRs { .. } => "probe-rs",
        Writer::Espflash { .. } => "the system (serial ports)",
    }
}

/// `selector` names `id` (trailing path separators aside; Windows names
/// are case-insensitive).
fn names(selector: &str, id: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['\\', '/']).to_string();
    let (a, b) = (trim(selector), trim(id));
    if cfg!(windows) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// One line per candidate with the option that selects it.
pub(crate) fn listing(cands: &[Candidate], w: &Writer) -> String {
    cands
        .iter()
        .map(|c| format!("  --{} {}  {}\n", flag(w), c.id, c.label))
        .collect()
}

/// The candidate to write to: the one `selector` names, else the only
/// one. Several without a selector: [`Pick::Choose`]. None: an error.
pub(crate) fn pick(
    mut cands: Vec<Candidate>,
    selector: Option<&str>,
    w: &Writer,
) -> Result<Pick, String> {
    if let Some(s) = selector {
        return match cands.iter().position(|c| names(s, &c.id)) {
            Some(i) => Ok(Pick::Use(cands.swap_remove(i))),
            None => Err(format!(
                "{s} is not among the devices {} reports{}. Nothing was written.",
                tool(w),
                if cands.is_empty() {
                    " (none)".to_string()
                } else {
                    format!(":\n{}", listing(&cands, w).trim_end())
                }
            )),
        };
    }
    match cands.len() {
        0 => Err(format!(
            "{} reports no device to write to. Nothing was written.",
            tool(w)
        )),
        1 => Ok(Pick::Use(cands.remove(0))),
        _ => Ok(Pick::Choose(cands)),
    }
}

/// Why several candidates stop a write that cannot ask.
pub(crate) fn several(cands: &[Candidate], w: &Writer) -> String {
    format!(
        "{} devices can be written to; name one with --{}:\n{}Nothing was written.",
        cands.len(),
        flag(w),
        listing(cands, w)
    )
}

/// The user's numbered choice among `cands` (1-based), read from `input`.
pub(crate) fn choose(
    cands: &[Candidate],
    w: &Writer,
    input: &mut dyn std::io::BufRead,
) -> Result<Candidate, String> {
    for (i, c) in cands.iter().enumerate() {
        println!("  {}) --{} {}  {}", i + 1, flag(w), c.id, c.label);
    }
    print!("write to which one? [1-{}] ", cands.len());
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    let _ = input.read_line(&mut line);
    line.trim()
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| cands.get(i).cloned())
        .ok_or_else(|| "no device chosen. Nothing was written.".into())
}

/// Every device `mcu`'s writer reports now; a UF2 writer waits up to
/// [`DRIVE_WAIT`] for the first drive.
pub(crate) fn find(
    mcu: &McuDef,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<Candidate>, String> {
    match &mcu.writer {
        Writer::ProbeRs { .. } => probes(),
        Writer::Espflash { usb_hint, .. } => ports(&mcu.name, *usb_hint),
        Writer::Uf2Drive {
            info_file,
            info_header,
            board_id,
            ..
        } => {
            let want = Uf2Want {
                info_file,
                info_header,
                board_id,
                mcu: &mcu.name,
            };
            wait_drives(&want, log, cancel)
        }
    }
}

fn probes() -> Result<Vec<Candidate>, String> {
    let o = Command::new("probe-rs")
        .arg("list")
        .output()
        .map_err(|e| format!("cannot run probe-rs ({e}); see `dagon doctor`"))?;
    if !o.status.success() {
        return Err(format!(
            "probe-rs list failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(parse_probes(&String::from_utf8_lossy(&o.stdout)))
}

/// The probes in `probe-rs list` output: `[0]: <name> -- <vid>:<pid>[:<serial>]
/// (<kind>)`, the `vid:pid[:serial]` being what `--probe` takes.
pub(crate) fn parse_probes(out: &str) -> Vec<Candidate> {
    out.lines()
        .filter(|l| l.trim_start().starts_with('['))
        .filter_map(|l| {
            let (_, rest) = l.split_once("]:")?;
            let (name, sel) = rest.split_once(" -- ")?;
            let id = sel.split_whitespace().next()?.trim_end_matches(':');
            let kind = sel[sel.find(id)? + id.len()..]
                .trim()
                .trim_start_matches(':');
            Some(Candidate {
                id: id.to_string(),
                label: format!("{} {}", name.trim(), kind.trim())
                    .trim()
                    .to_string(),
            })
        })
        .collect()
}

fn ports(mcu: &str, hint: Option<UsbHint>) -> Result<Vec<Candidate>, String> {
    let ports = serialport::available_ports().map_err(|e| format!("cannot list ports: {e}"))?;
    Ok(ports
        .into_iter()
        .map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) => {
                usb_port(p.port_name, u.vid, u.pid, u.product, mcu, hint)
            }
            other => Candidate {
                id: p.port_name,
                label: format!("{other:?}"),
            },
        })
        .collect())
}

/// A USB serial port, labelled with `mcu` when its ID is the hint's.
pub(crate) fn usb_port(
    name: String,
    vid: u16,
    pid: u16,
    product: Option<String>,
    mcu: &str,
    hint: Option<UsbHint>,
) -> Candidate {
    let mut label = format!("USB {vid:04x}:{pid:04x}");
    if let Some(p) = product {
        label.push_str(&format!(" {p}"));
    }
    if hint.is_some_and(|h| (h.vid, h.pid) == (vid, pid)) {
        label.push_str(&format!(" ({mcu})"));
    }
    Candidate { id: name, label }
}

/// What a UF2 writer's definition says about its boot drive.
pub(crate) struct Uf2Want<'a> {
    pub info_file: &'a str,
    pub info_header: &'a str,
    pub board_id: &'a str,
    pub mcu: &'a str,
}

fn wait_drives(
    want: &Uf2Want<'_>,
    log: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<Vec<Candidate>, String> {
    log(&format!(
        "waiting for a UF2 drive ({} shows up as Board-ID {}) for {} s",
        want.mcu,
        want.board_id,
        DRIVE_WAIT.as_secs()
    ));
    let until = Instant::now() + DRIVE_WAIT;
    // Volumes that did not answer in time are not asked again (a hung
    // network drive would hold one more thread on every poll).
    let mut slow: Vec<PathBuf> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled; nothing was copied".into());
        }
        let roots: Vec<PathBuf> = mounted_volumes()
            .into_iter()
            .filter(|r| !slow.contains(r))
            .collect();
        let (found, late) = uf2_drives(&roots, want, VOLUME_WAIT, read_info);
        for r in late {
            log(&format!(
                "skipped {}: no answer within {} s",
                r.display(),
                VOLUME_WAIT.as_secs()
            ));
            slow.push(r);
        }
        if !found.is_empty() || Instant::now() >= until {
            return Ok(found);
        }
        std::thread::sleep(DRIVE_POLL);
    }
}

/// `file` on a volume.
fn read_info(file: &Path) -> Option<String> {
    std::fs::read_to_string(file).ok()
}

type Reader = fn(&Path) -> Option<String>;

/// The UF2 boot drives among `roots`: `info_file` there starts with the
/// `info_header` line. A `Board-ID: <board_id>` line (exactly) labels the
/// drive as the definition's MCU. Each volume is read on its own thread;
/// those that have not answered within `wait` are returned second.
pub(crate) fn uf2_drives(
    roots: &[PathBuf],
    want: &Uf2Want<'_>,
    wait: Duration,
    read: Reader,
) -> (Vec<Candidate>, Vec<PathBuf>) {
    let (tx, rx) = std::sync::mpsc::channel();
    for (i, r) in roots.iter().enumerate() {
        let (tx, file) = (tx.clone(), r.join(want.info_file));
        std::thread::spawn(move || {
            let _ = tx.send((i, read(&file)));
        });
    }
    drop(tx);
    let mut infos: Vec<Option<Option<String>>> = vec![None; roots.len()];
    let until = Instant::now() + wait;
    while let Ok((i, info)) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        infos[i] = Some(info);
    }
    let late = roots
        .iter()
        .zip(&infos)
        .filter(|(_, i)| i.is_none())
        .map(|(r, _)| r.clone())
        .collect();
    let found = roots
        .iter()
        .zip(infos)
        .filter_map(|(r, info)| uf2_candidate(r, info.flatten()?.as_str(), want))
        .collect();
    (found, late)
}

fn uf2_candidate(root: &Path, info: &str, want: &Uf2Want<'_>) -> Option<Candidate> {
    let mut lines = info.lines();
    if !lines.next()?.starts_with(want.info_header) {
        return None;
    }
    let board = lines.find_map(|l| l.strip_prefix("Board-ID: "));
    let label = match board {
        Some(b) if b == want.board_id => format!("UF2 drive, Board-ID {b} ({})", want.mcu),
        Some(b) => format!("UF2 drive, Board-ID {b}"),
        None => "UF2 drive".into(),
    };
    Some(Candidate {
        id: root.display().to_string(),
        label,
    })
}

/// Every mounted volume: on Windows every drive letter A: to Z: (a letter
/// with nothing mounted fails its read at once; reads are time-limited),
/// elsewhere every mount point in /proc/self/mountinfo and every folder in
/// /Volumes (macOS).
// ponytail: all 26 letters rather than GetLogicalDrives, which needs an
// unsafe FFI call the crate forbids (`#![forbid(unsafe_code)]`).
fn mounted_volumes() -> Vec<PathBuf> {
    if cfg!(windows) {
        return drive_letters();
    }
    let mut out = std::fs::read_to_string("/proc/self/mountinfo")
        .map(|t| mount_points(&t))
        .unwrap_or_default();
    if let Ok(dir) = std::fs::read_dir("/Volumes") {
        out.extend(dir.filter_map(|e| e.ok().map(|e| e.path())));
    }
    out.sort();
    out.dedup();
    out
}

/// `A:\` .. `Z:\`.
pub(crate) fn drive_letters() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .collect()
}

/// The mount points (field 5) of /proc/self/mountinfo, with its octal
/// escapes (`\040` for a space) decoded.
pub(crate) fn mount_points(mountinfo: &str) -> Vec<PathBuf> {
    mountinfo
        .lines()
        .filter_map(|l| l.split(' ').nth(4))
        .map(|p| PathBuf::from(unescape(p)))
        .collect()
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let oct = b
            .get(i + 1..i + 4)
            .filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)));
        match (b[i], oct) {
            (b'\\', Some(d)) => {
                out.push(d.iter().fold(0u8, |v, c| v.wrapping_mul(8) + (c - b'0')));
                i += 4;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            label: format!("label of {id}"),
        }
    }

    fn probe_rs() -> Writer {
        Writer::ProbeRs { chip: "X".into() }
    }

    #[test]
    fn one_candidate_is_taken_several_are_offered_none_stops() {
        let w = probe_rs();
        assert_eq!(pick(vec![c("a")], None, &w), Ok(Pick::Use(c("a"))));
        assert_eq!(
            pick(vec![c("a"), c("b")], None, &w),
            Ok(Pick::Choose(vec![c("a"), c("b")]))
        );
        let none = pick(vec![], None, &w).unwrap_err();
        assert!(none.contains("Nothing was written"), "{none}");
    }

    #[test]
    fn a_selector_names_one_of_the_candidates_or_nothing_is_written() {
        let w = Writer::Espflash {
            chip: "x".into(),
            flash_size: "x".into(),
            table_at: 0,
            bootloader: "x".into(),
            checksums: "x".into(),
            usb_hint: None,
        };
        let all = vec![c("COM3"), c("COM7")];
        assert_eq!(
            pick(all.clone(), Some("COM7"), &w),
            Ok(Pick::Use(c("COM7")))
        );
        let e = pick(all, Some("COM9"), &w).unwrap_err();
        assert!(
            e.contains("COM9") && e.contains("--port COM3") && e.contains("Nothing was written"),
            "{e}"
        );
        // A selector is never passed on unless it names a candidate.
        assert!(pick(vec![c("COM3")], Some("--erase-all"), &w).is_err());
    }

    #[test]
    fn several_without_a_selector_print_every_selector() {
        let w = probe_rs();
        let s = several(&[c("0483:374b:A1"), c("0483:374b:B2")], &w);
        assert!(s.contains("--probe 0483:374b:A1") && s.contains("--probe 0483:374b:B2"));
        assert!(s.contains("Nothing was written"), "{s}");
    }

    #[test]
    fn the_numbered_choice_takes_only_a_listed_number() {
        let w = probe_rs();
        let all = [c("a"), c("b")];
        assert_eq!(choose(&all, &w, &mut "2\n".as_bytes()), Ok(c("b")));
        for bad in ["0\n", "3\n", "b\n", ""] {
            assert!(choose(&all, &w, &mut bad.as_bytes()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_probe_probe_rs_lists_is_a_candidate() {
        let out = "The following debug probes were found:\n\
                   [0]: STLink V2-1 -- 0483:374b:066DFF485550755187161627 (ST-LINK)\n\
                   [1]: CMSIS-DAP -- 2e8a:000c:E66038B713849D31 (CMSIS-DAP)\n\
                   [2]: J-Link -- 1366:0101 (J-Link)\n";
        let p = parse_probes(out);
        let ids: Vec<&str> = p.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "0483:374b:066DFF485550755187161627",
                "2e8a:000c:E66038B713849D31",
                "1366:0101"
            ]
        );
        assert_eq!(p[0].label, "STLink V2-1 (ST-LINK)");
        assert!(parse_probes("No debug probes were found.\n").is_empty());
    }

    #[test]
    fn the_usb_hint_labels_a_port_and_filters_nothing() {
        let hint = Some(UsbHint {
            vid: 0x303A,
            pid: 0x1001,
        });
        let esp = usb_port("COM7".into(), 0x303A, 0x1001, None, "ESP32-S3", hint);
        assert_eq!(esp.label, "USB 303a:1001 (ESP32-S3)");
        let other = usb_port(
            "COM5".into(),
            0xC0DE,
            0xCAFE,
            Some("X".into()),
            "ESP32-S3",
            hint,
        );
        assert_eq!(other.label, "USB c0de:cafe X");
    }

    fn drives(roots: &[PathBuf]) -> Vec<Candidate> {
        let (found, late) = uf2_drives(roots, &RP, Duration::from_secs(5), read_info);
        assert!(late.is_empty());
        found
    }

    const RP: Uf2Want<'static> = Uf2Want {
        info_file: "INFO_UF2.TXT",
        info_header: "UF2 Bootloader",
        board_id: "RPI-RP2",
        mcu: "RP2040",
    };

    #[test]
    fn every_uf2_drive_is_a_candidate_and_the_board_id_labels_it() {
        let base = std::env::temp_dir().join(format!("dagon-uf2c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dirs = ["usb", "rp", "rp2350", "plain"].map(|d| base.join(d));
        for d in &dirs {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(dirs[0].join("INFO_UF2.TXT"), "Board-ID: RPI-RP2\n").unwrap();
        std::fs::write(
            dirs[1].join("INFO_UF2.TXT"),
            "UF2 Bootloader v3.0\r\nModel: Raspberry Pi RP2\r\nBoard-ID: RPI-RP2\r\n",
        )
        .unwrap();
        std::fs::write(
            dirs[2].join("INFO_UF2.TXT"),
            "UF2 Bootloader v1.0\nBoard-ID: RP2350\n",
        )
        .unwrap();
        let found = drives(&dirs);
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "UF2 drive, Board-ID RPI-RP2 (RP2040)",
                "UF2 drive, Board-ID RP2350"
            ]
        );
        assert_eq!(found[0].id, dirs[1].display().to_string());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_the_exact_board_id_line_labels_a_drive() {
        let base = std::env::temp_dir().join(format!("dagon-uf2l-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        for not in [
            "UF2 Bootloader v3.0\nBoard-ID: RPI-RP2-CLONE\n",
            "UF2 Bootloader v3.0\nBoard-ID: RPI-RP2 \n",
            "UF2 Bootloader v3.0\nBoard-ID:RPI-RP2\n",
            "UF2 Bootloader v3.0\n Board-ID: RPI-RP2\n",
            "UF2 Bootloader v3.0\nModel: RPI-RP2\n",
        ] {
            std::fs::write(base.join("INFO_UF2.TXT"), not).unwrap();
            let f = drives(std::slice::from_ref(&base));
            assert!(!f[0].label.contains("RP2040"), "{not:?}: {f:?}");
        }
        for not_uf2 in ["My stick\nUF2 Bootloader v3.0\n", "\nUF2 Bootloader\n", ""] {
            std::fs::write(base.join("INFO_UF2.TXT"), not_uf2).unwrap();
            assert!(drives(std::slice::from_ref(&base)).is_empty());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_drive_letter_and_mount_point_is_a_volume() {
        let letters = drive_letters();
        assert_eq!(letters.len(), 26);
        assert_eq!(
            (letters[0].clone(), letters[25].clone()),
            ("A:\\".into(), "Z:\\".into())
        );
        let info = [
            r"22 1 0:21 / /proc rw - proc proc rw",
            r"36 25 8:17 / /mnt rw,nosuid - vfat /dev/sdb1 rw",
            r"37 25 8:33 / /media/u/RPI-RP2\0401 rw - vfat /dev/sdc1 rw",
            r"38 25 8:49 / /run/media/u/back\134slash rw - vfat /dev/sdd1 rw",
        ]
        .join("\n");
        let info = info.as_str();
        assert_eq!(
            mount_points(info),
            [
                "/proc",
                "/mnt",
                "/media/u/RPI-RP2 1",
                "/run/media/u/back\\slash"
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn a_volume_that_does_not_answer_is_skipped_not_waited_for() {
        fn read(file: &Path) -> Option<String> {
            if file.starts_with("/hung") {
                std::thread::sleep(Duration::from_secs(5));
            }
            Some("UF2 Bootloader v3.0\nBoard-ID: RPI-RP2\n".into())
        }
        let roots = ["/hung", "/rp"].map(PathBuf::from);
        let t = Instant::now();
        let (found, late) = uf2_drives(&roots, &RP, Duration::from_millis(300), read);
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        assert_eq!(late, [PathBuf::from("/hung")]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, PathBuf::from("/rp").display().to_string());
    }
}
