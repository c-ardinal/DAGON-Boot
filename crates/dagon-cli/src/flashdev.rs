//! The device a first write goes to. Every device the writer tool reports
//! is a candidate (all probe-rs probes, all serial ports, all UF2 drives);
//! what the MCU definition knows about its device (a USB ID, a UF2
//! Board-ID) only labels candidates, never filters them. Exactly one is
//! taken (still shown before the confirmation); several need the user's
//! choice or a selector; none stops the write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
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
    /// The definition's hint (USB ID, UF2 Board-ID) matches this device.
    pub matched: bool,
    /// The volume itself for a drive (`id` is its lossy display form, so
    /// a mount path that is not UTF-8 is still written to exactly).
    #[serde(skip)]
    pub path: Option<PathBuf>,
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

/// Hint-matched candidates first, each group in the order reported.
pub(crate) fn ordered(mut cands: Vec<Candidate>) -> Vec<Candidate> {
    cands.sort_by_key(|c| !c.matched);
    cands
}

/// The candidate to write to: the one `selector` names, else the only
/// one. Several without a selector: [`Pick::Choose`]. None: an error.
pub(crate) fn pick(
    cands: Vec<Candidate>,
    selector: Option<&str>,
    w: &Writer,
) -> Result<Pick, String> {
    let mut cands = ordered(cands);
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
    // `cands` come ordered by `pick`: a match, if any, is first.
    let default = cands.first().is_some_and(|c| c.matched);
    if default {
        print!("write to which one? [1-{}, Enter = 1] ", cands.len());
    } else {
        print!("write to which one? [1-{}] ", cands.len());
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    let _ = input.read_line(&mut line);
    let line = line.trim();
    let line = if default && line.is_empty() {
        "1"
    } else {
        line
    };
    line.parse::<usize>()
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
                matched: false,
                path: None,
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
                matched: false,
                path: None,
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
    let matched = hint.is_some_and(|h| (h.vid, h.pid) == (vid, pid));
    if matched {
        label.push_str(&format!(" ({mcu})"));
    }
    Candidate {
        id: name,
        label,
        matched,
        path: None,
    }
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
    // Late volumes are asked again on every poll (a read still running is
    // not started twice); each is noted once.
    let mut noted: Vec<PathBuf> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled; nothing was copied".into());
        }
        let (found, late) = uf2_drives(&mounted_volumes(), want, VOLUME_WAIT, read_info);
        for r in late {
            if noted.contains(&r) {
                continue;
            }
            log(&format!(
                "{}: no answer within {} s yet; asked again while waiting",
                r.display(),
                VOLUME_WAIT.as_secs()
            ));
            noted.push(r);
        }
        if !found.is_empty() || Instant::now() >= until {
            return Ok(found);
        }
        std::thread::sleep(DRIVE_POLL);
    }
}

/// `file` on a volume, as text (a byte that is not UTF-8 does not hide
/// the drive).
fn read_info(file: &Path) -> Option<String> {
    std::fs::read(file)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

type Reader = fn(&Path) -> Option<String>;

/// Held by the one UF2 scan running in this process (CLI or serve jobs).
static SCAN: Mutex<()> = Mutex::new(());
/// Reads that outlived the scan that started them: still running (a
/// silent volume holds this one thread), or finished with what they read;
/// by volume and info file (MCUs with another UF2 writer read another).
static READS: Mutex<BTreeMap<ReadKey, Read>> = Mutex::new(BTreeMap::new());

type ReadKey = (PathBuf, String);
const LOCK_POLL: Duration = Duration::from_millis(10);
/// How old a late read's result may be for the next scan to use it: it
/// returned at most one scan and one poll before that scan began.
const LATE_RESULT_AGE: Duration = Duration::from_millis(2500);

enum Read {
    Running,
    Finished(Option<String>, Instant),
}

fn reads() -> MutexGuard<'static, BTreeMap<ReadKey, Read>> {
    READS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The UF2 boot drives among `roots`: `info_file` there starts with the
/// `info_header` line. A `Board-ID: <board_id>` line (exactly) labels the
/// drive as the definition's MCU. Scans run one at a time; within one,
/// each volume is read on its own thread. Volumes with no answer within
/// `wait` (from the call, the wait for another scan included) are returned
/// second, to be asked again by the next scan: all of them when another
/// scan held the lock that long, and a volume whose earlier read is still
/// running. A read that returns after its scan gave up is kept, and the
/// next scan takes its result instead of reading again.
pub(crate) fn uf2_drives(
    roots: &[PathBuf],
    want: &Uf2Want<'_>,
    wait: Duration,
    read: Reader,
) -> (Vec<Candidate>, Vec<PathBuf>) {
    let until = Instant::now() + wait;
    let _scan = loop {
        match SCAN.try_lock() {
            Ok(g) => break g,
            Err(std::sync::TryLockError::Poisoned(e)) => break e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) if Instant::now() < until => {
                std::thread::sleep(LOCK_POLL)
            }
            Err(std::sync::TryLockError::WouldBlock) => return (Vec::new(), roots.to_vec()),
        }
    };
    if Instant::now() >= until {
        return (Vec::new(), roots.to_vec());
    }
    let mut infos: Vec<Option<Option<String>>> = vec![None; roots.len()];
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let mut kept = reads();
        // A finished read of a volume no longer mounted is never asked for.
        kept.retain(|(path, _), r| matches!(r, Read::Running) || roots.contains(path));
        let key = |r: &PathBuf| (r.clone(), want.info_file.to_string());
        for (i, r) in roots.iter().enumerate() {
            match kept.remove(&key(r)) {
                Some(Read::Running) => {
                    kept.insert(key(r), Read::Running);
                    continue;
                }
                Some(Read::Finished(info, at)) if at.elapsed() <= LATE_RESULT_AGE => {
                    infos[i] = Some(info);
                    continue;
                }
                _ => {}
            }
            kept.insert(key(r), Read::Running);
            let (tx, k, file) = (tx.clone(), key(r), r.join(want.info_file));
            std::thread::spawn(move || {
                let info = read(&file);
                // Kept for the next scan in case this one has given up.
                reads().insert(k, Read::Finished(info.clone(), Instant::now()));
                let _ = tx.send((i, info));
            });
        }
    }
    drop(tx);
    while let Ok((i, info)) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
        infos[i] = Some(info);
        // Taken by this scan: not handed to the next.
        reads().remove(&(roots[i].clone(), want.info_file.to_string()));
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
    let matched = board == Some(want.board_id);
    let label = match board {
        Some(b) if b == want.board_id => format!("UF2 drive, Board-ID {b} ({})", want.mcu),
        Some(b) => format!("UF2 drive, Board-ID {b}"),
        None => "UF2 drive".into(),
    };
    Some(Candidate {
        id: root.display().to_string(),
        label,
        matched,
        path: Some(root.to_path_buf()),
    })
}

/// Every mounted volume: on Windows every drive letter A: to Z: (a letter
/// with nothing mounted fails its read at once; reads are time-limited),
/// elsewhere every mount point in /proc/self/mountinfo and every folder in
/// /Volumes (macOS).
// all 26 letters rather than GetLogicalDrives, which needs an
// unsafe FFI call the crate forbids (`#![forbid(unsafe_code)]`).
fn mounted_volumes() -> Vec<PathBuf> {
    if cfg!(windows) {
        return drive_letters();
    }
    let mut out = std::fs::read("/proc/self/mountinfo")
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

/// The mount points (field 5) of /proc/self/mountinfo, read as bytes:
/// paths need not be UTF-8, and a line too short to have one is skipped
/// alone.
pub(crate) fn mount_points(mountinfo: &[u8]) -> Vec<PathBuf> {
    mount_point_bytes(mountinfo)
        .into_iter()
        .filter_map(path_from_bytes)
        .collect()
}

/// Field 5 of each mountinfo line with its octal escapes (`\040` for a
/// space) decoded.
pub(crate) fn mount_point_bytes(mountinfo: &[u8]) -> Vec<Vec<u8>> {
    mountinfo
        .split(|&b| b == b'\n')
        .filter_map(|l| l.split(|&b| b == b' ').nth(4))
        .filter(|p| !p.is_empty())
        .map(unescape)
        .collect()
}

#[cfg(unix)]
fn path_from_bytes(b: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(b)))
}

#[cfg(not(unix))]
fn path_from_bytes(b: Vec<u8>) -> Option<PathBuf> {
    String::from_utf8(b).ok().map(PathBuf::from)
}

fn unescape(b: &[u8]) -> Vec<u8> {
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
    out
}

#[cfg(test)]
// Tests set up and inspect their own scratch folders directly.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn c(id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            label: format!("label of {id}"),
            matched: false,
            path: None,
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

    fn m(id: &str) -> Candidate {
        Candidate {
            matched: true,
            ..c(id)
        }
    }

    #[test]
    fn matched_candidates_come_first_in_stable_order() {
        let w = probe_rs();
        let got = pick(vec![c("a"), m("b"), c("c"), m("d")], None, &w).unwrap();
        assert_eq!(got, Pick::Choose(vec![m("b"), m("d"), c("a"), c("c")]));
    }

    #[test]
    fn enter_takes_the_matched_one_and_another_number_still_works() {
        let w = probe_rs();
        let l = [m("b"), c("a")];
        assert_eq!(choose(&l, &w, &mut "\n".as_bytes()), Ok(m("b")));
        assert_eq!(choose(&l, &w, &mut "2\n".as_bytes()), Ok(c("a")));
        // No match: Enter chooses nothing.
        assert!(choose(&[c("a"), c("b")], &w, &mut "\n".as_bytes()).is_err());
    }

    #[test]
    fn non_interactive_refusal_lists_the_matched_first() {
        let w = probe_rs();
        let Ok(Pick::Choose(l)) = pick(vec![c("a"), m("b")], None, &w) else {
            panic!()
        };
        let s = several(&l, &w);
        assert!(
            s.find("--probe b").unwrap() < s.find("--probe a").unwrap(),
            "{s}"
        );
        assert!(s.contains("Nothing was written"), "{s}");
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

    /// Every test that scans runs alone: the scan lock and the kept reads
    /// are process-wide, so a scan from a parallel test makes theirs busy
    /// or forgets the read they kept (it was of a volume not in its roots).
    fn scans_alone() -> MutexGuard<'static, ()> {
        static ALONE: Mutex<()> = Mutex::new(());
        ALONE.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn drives(roots: &[PathBuf]) -> Vec<Candidate> {
        let _alone = scans_alone();
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
    fn an_info_file_with_bytes_that_are_not_utf8_still_shows_the_drive() {
        let base = std::env::temp_dir().join(format!("dagon-uf2b-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("INFO_UF2.TXT"),
            b"UF2 Bootloader v3.0\nModel: Caf\xe9 board\nBoard-ID: RPI-RP2\n",
        )
        .unwrap();
        let f = drives(std::slice::from_ref(&base));
        assert_eq!(f.len(), 1);
        assert!(f[0].label.contains("RP2040"), "{f:?}");
        assert_eq!(f[0].path.as_deref(), Some(base.as_path()));
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
            "broken line",
        ]
        .join("\n");
        assert_eq!(
            mount_points(info.as_bytes()),
            [
                "/proc",
                "/mnt",
                "/media/u/RPI-RP2 1",
                r"/run/media/u/back\slash"
            ]
            .map(PathBuf::from)
        );
    }

    /// A mount path that is not UTF-8 (raw or escaped) is kept byte for
    /// byte and hides neither itself nor the other lines.
    #[test]
    fn mount_points_are_bytes_not_text() {
        let mut info = b"36 25 8:17 / /media/u/caf\xe9 rw - vfat /dev/sdb1 rw\n".to_vec();
        info.extend_from_slice(b"37 25 8:33 / /media/u/RPI\\377 rw - vfat /dev/sdc1 rw\n");
        info.extend_from_slice(b"38 25 8:49 / /mnt rw - vfat /dev/sdd1 rw\n");
        assert_eq!(
            mount_point_bytes(&info),
            [
                b"/media/u/caf\xe9".to_vec(),
                b"/media/u/RPI\xff".to_vec(),
                b"/mnt".to_vec()
            ]
        );
        assert!(mount_points(&info).contains(&PathBuf::from("/mnt")));
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_mount_point_is_a_volume() {
        use std::os::unix::ffi::OsStrExt;
        let info = b"36 25 8:17 / /media/u/caf\xe9 rw - vfat /dev/sdb1 rw\n";
        let p = mount_points(info);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].as_os_str().as_bytes(), b"/media/u/caf\xe9");
    }

    #[test]
    fn a_volume_that_does_not_answer_is_skipped_not_waited_for() {
        let _alone = scans_alone();
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

    /// UF2 info a fake volume answers after `ms`.
    fn slow_rp(ms: u64) -> Option<String> {
        std::thread::sleep(Duration::from_millis(ms));
        Some("UF2 Bootloader v3.0\nBoard-ID: RPI-RP2\n".into())
    }

    /// A fake volume's read waits on a gate the test opens; the wait is
    /// bounded, so a test that never opens it fails instead of hanging.
    struct Gate(Mutex<bool>, std::sync::Condvar);

    impl Gate {
        const fn new() -> Self {
            Gate(Mutex::new(false), std::sync::Condvar::new())
        }

        fn open(&self) {
            *self.0.lock().unwrap() = true;
            self.1.notify_all();
        }

        /// UF2 info once opened; None if never opened within 30 s.
        fn read(&self) -> Option<String> {
            let g = self.0.lock().unwrap();
            let (g, t) = self
                .1
                .wait_timeout_while(g, Duration::from_secs(30), |open| !*open)
                .unwrap();
            drop(g);
            if t.timed_out() {
                return None;
            }
            slow_rp(0)
        }
    }

    /// Spins (bounded to 30 s) until `done`.
    fn wait_until(what: &str, done: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(30);
        while !done() {
            assert!(Instant::now() < until, "never happened: {what}");
            std::thread::yield_now();
        }
    }

    fn finished(root: &Path) -> bool {
        let key = (root.to_path_buf(), RP.info_file.to_string());
        matches!(reads().get(&key), Some(Read::Finished(..)))
    }

    #[test]
    fn a_silent_volume_holds_one_thread_and_is_asked_again_later() {
        let _alone = scans_alone();
        static GATE: Gate = Gate::new();
        static READS: AtomicUsize = AtomicUsize::new(0);
        fn read(_: &Path) -> Option<String> {
            READS.fetch_add(1, Ordering::SeqCst);
            GATE.read()
        }
        let roots = [PathBuf::from("/silent-rp")];
        for _ in 0..3 {
            let (found, late) = uf2_drives(&roots, &RP, Duration::from_millis(50), read);
            assert!(found.is_empty());
            assert_eq!(late, roots, "not answered yet, never dropped");
        }
        wait_until("the first read starts", || {
            READS.load(Ordering::SeqCst) >= 1
        });
        assert_eq!(READS.load(Ordering::SeqCst), 1, "one thread, not three");
        GATE.open();
        wait_until("that read ends", || finished(&roots[0]));
        // Once that read is over, the next scan takes its result.
        let (found, late) = uf2_drives(&roots, &RP, Duration::from_secs(30), read);
        assert!(late.is_empty());
        assert_eq!(found.len(), 1);
        assert_eq!(READS.load(Ordering::SeqCst), 1);
        // Taken: the scan after that reads the volume afresh.
        let _ = uf2_drives(&roots, &RP, Duration::from_secs(30), read);
        assert_eq!(READS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_read_that_finishes_after_its_scan_is_used_by_the_next() {
        let _alone = scans_alone();
        static READS_DONE: AtomicUsize = AtomicUsize::new(0);
        // The volume answers only once the test opens the gate: the first
        // scan has given up by then, whatever the machine's load.
        static GATE: Gate = Gate::new();
        fn read(_: &Path) -> Option<String> {
            READS_DONE.fetch_add(1, Ordering::SeqCst);
            GATE.read()
        }
        let roots = [PathBuf::from("/just-late-rp")];
        let (found, late) = uf2_drives(&roots, &RP, Duration::from_millis(50), read);
        assert!(found.is_empty());
        assert_eq!(late, roots);
        GATE.open();
        // The read finishes after its scan gave up and is kept.
        wait_until("the released read finishes", || finished(&roots[0]));
        // The next poll takes the kept result instead of reading again.
        let (found, late) = uf2_drives(&roots, &RP, Duration::from_secs(30), read);
        assert!(late.is_empty());
        assert_eq!(found.len(), 1);
        assert_eq!(READS_DONE.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_late_read_is_kept_per_info_file_and_dropped_once_unmounted() {
        let _alone = scans_alone();
        static GATE: Gate = Gate::new();
        fn read(_: &Path) -> Option<String> {
            GATE.read()
        }
        let roots = [PathBuf::from("/late-keyed-rp")];
        let other = Uf2Want {
            info_file: "OTHER.TXT",
            ..RP
        };
        let _ = uf2_drives(&roots, &RP, Duration::from_millis(50), read);
        GATE.open();
        wait_until("the released read finishes", || finished(&roots[0]));
        let kept = |file: &str| reads().contains_key(&(roots[0].clone(), file.to_string()));
        assert!(kept(RP.info_file));
        assert!(!kept("OTHER.TXT"), "another info file is not this read");
        // A scan of other volumes forgets the finished read of this one.
        let _ = uf2_drives(&[], &other, Duration::from_secs(30), read);
        assert!(!kept(RP.info_file));
    }

    #[test]
    fn three_concurrent_scans_all_get_a_slow_volume() {
        let _alone = scans_alone();
        static GATE: Gate = Gate::new();
        static ENTERED: AtomicUsize = AtomicUsize::new(0);
        fn read(_: &Path) -> Option<String> {
            ENTERED.fetch_add(1, Ordering::SeqCst);
            GATE.read()
        }
        let roots = vec![PathBuf::from("/slow-rp-3")];
        let scans: Vec<_> = (0..3)
            .map(|_| {
                let roots = roots.clone();
                std::thread::spawn(move || uf2_drives(&roots, &RP, Duration::from_secs(30), read))
            })
            .collect();
        // A scan is waiting on the volume; the others queue behind it.
        wait_until("a read starts", || ENTERED.load(Ordering::SeqCst) >= 1);
        GATE.open();
        for s in scans {
            let (found, late) = s.join().unwrap();
            assert!(late.is_empty());
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].label.contains("RP2040"));
        }
    }

    #[test]
    fn a_scan_kept_waiting_by_another_reports_busy_and_finds_it_next_time() {
        let _alone = scans_alone();
        static GATE: Gate = Gate::new();
        static ENTERED: AtomicUsize = AtomicUsize::new(0);
        fn read(_: &Path) -> Option<String> {
            ENTERED.fetch_add(1, Ordering::SeqCst);
            GATE.read()
        }
        let roots = vec![PathBuf::from("/slow-rp-4")];
        let owner = {
            let roots = roots.clone();
            std::thread::spawn(move || uf2_drives(&roots, &RP, Duration::from_secs(30), read))
        };
        // The owner holds the scan lock before it starts the read.
        wait_until("the owner's read starts", || {
            ENTERED.load(Ordering::SeqCst) >= 1
        });
        let (found, late) = uf2_drives(&roots, &RP, Duration::from_millis(100), read);
        assert!(found.is_empty());
        assert_eq!(late, roots, "busy: every volume late, none excluded");
        GATE.open();
        let (found, _) = uf2_drives(&roots, &RP, Duration::from_secs(30), read);
        assert_eq!(found.len(), 1);
        assert_eq!(owner.join().unwrap().0.len(), 1);
    }
}
