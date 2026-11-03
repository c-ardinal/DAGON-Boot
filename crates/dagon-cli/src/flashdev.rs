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
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled; nothing was copied".into());
        }
        let found = uf2_drives(&drive_roots(), want);
        if !found.is_empty() || Instant::now() >= until {
            return Ok(found);
        }
        std::thread::sleep(DRIVE_POLL);
    }
}

/// The UF2 boot drives among `roots`: `info_file` there starts with the
/// `info_header` line. A `Board-ID: <board_id>` line (exactly) labels the
/// drive as the definition's MCU.
pub(crate) fn uf2_drives(roots: &[PathBuf], want: &Uf2Want<'_>) -> Vec<Candidate> {
    roots
        .iter()
        .filter_map(|r| {
            let info = std::fs::read_to_string(r.join(want.info_file)).ok()?;
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
                id: r.display().to_string(),
                label,
            })
        })
        .collect()
}

/// Where a removable drive shows up on this system.
fn drive_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        return (b'D'..=b'Z')
            .map(|c| PathBuf::from(format!("{}:\\", c as char)))
            .collect();
    }
    let user = std::env::var("USER").unwrap_or_default();
    let parents = [
        PathBuf::from("/Volumes"),
        Path::new("/media").join(&user),
        Path::new("/run/media").join(&user),
        PathBuf::from("/media"),
        PathBuf::from("/mnt"),
    ];
    mounts_under(&parents)
}

/// Every directory directly under `parents`: a second board mounts under
/// another name ("RPI-RP2 1"), so names are not guessed.
pub(crate) fn mounts_under(parents: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = parents
        .iter()
        .filter_map(|p| std::fs::read_dir(p).ok())
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out.dedup();
    out
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
        let found = uf2_drives(&dirs, &RP);
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
            let f = uf2_drives(std::slice::from_ref(&base), &RP);
            assert!(!f[0].label.contains("RP2040"), "{not:?}: {f:?}");
        }
        for not_uf2 in ["My stick\nUF2 Bootloader v3.0\n", "\nUF2 Bootloader\n", ""] {
            std::fs::write(base.join("INFO_UF2.TXT"), not_uf2).unwrap();
            assert!(uf2_drives(std::slice::from_ref(&base), &RP).is_empty());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_second_board_under_another_mount_name_is_seen() {
        let base = std::env::temp_dir().join(format!("dagon-mnt-{}", std::process::id()));
        let (volumes, media) = (base.join("Volumes"), base.join("media/user"));
        let info = "UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n";
        for d in [volumes.join("RPI-RP2"), volumes.join("RPI-RP2 1")] {
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("INFO_UF2.TXT"), info).unwrap();
        }
        std::fs::create_dir_all(media.join("USB STICK")).unwrap();
        std::fs::write(volumes.join("not-a-dir"), "x").unwrap();
        let parents = [volumes.clone(), media.clone(), base.join("missing")];
        let roots = mounts_under(&parents);
        assert_eq!(roots.len(), 3, "{roots:?}");
        assert_eq!(uf2_drives(&roots, &RP).len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }
}
