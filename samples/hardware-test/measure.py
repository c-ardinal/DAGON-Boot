#!/usr/bin/env python3
"""Hardware measurements the firmware has a measuring build for: writes a
node with that build through `dagon flash`, reads the value it keeps (a
debug probe reads it by symbol; blinks on the board's `[boot] led` light,
e.g. the display backlight, where it has no probe), checks it against its limit, records it, and writes the node's
normal build back. See README.md, "Measurements".

    python measure.py stage0-verify --project <project> --dagon <exe> --node <id> --key <file>
                                    [--probe <sel> | --drive <dir>] [--out <dir>]
    python measure.py listen-time   --project <project> --dagon <exe> --node <id> --key <file>
                                    [--probe <sel>] [--chip <chip>] [--drive <dir>] [--out <dir>]
    python measure.py decide-time   (the same arguments as listen-time)
    python measure.py masked-time   --project <project> --dagon <exe> --node <id> --key <file>
                                    [--probe <sel>] [--chip <chip>] [--drive <dir>] [--out <dir>]
    python measure.py restore       --project <project>
    python measure.py selftest

A measuring build is switched on by naming its feature in the `default`
features of the package `dagon flash` builds (the node's stage0 in the
DAGON source, or the node's boot crate) for the one write, then put back
byte for byte; a copy waits beside the file until then (`restore` puts
back any copy a stopped run left).
"""

import argparse
import datetime as dt
import json
import re
import struct
import subprocess
import sys
import time
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
BACKUP = ".measure-backup"


# What each measurement builds and reads. `package` is where the feature
# goes: "stage0" is targets/mcu/<mcu>/stage0 in the DAGON source, "boot" the
# node's boot crate (<project>/boot/<id>), "app" the node's app crate
# (dagon.toml `[build.<id>] package`). `part` is the folder dagon flash
# builds that part in (<project>/out/flash/target/<part>); the app builds in
# its own crate's target folder.
MEASUREMENTS = {
    "stage0-verify": {
        "package": "stage0",
        "feature": "verify-time",
        "part": "stage0",
        "symbols": ["DAGON_VERIFY_CYCLES", "DAGON_VERIFY_COUNT"],
        # Longest wait for the check: RP2040 on its ring oscillator at
        # 1.8 MHz takes up to 28 s (lifecycle-events, "Stage0's clock").
        "wait_s": 35,
    },
    # The boot core's measuring builds wait out stage0's check first.
    "listen-time": {
        "package": "boot",
        "feature": "{bootcore}/listen-time",
        "part": "bootcore",
        "symbols": ["DAGON_LISTEN_US"],
        "wait_s": 35,
    },
    "decide-time": {
        "package": "boot",
        "feature": "{bootcore}/decide-time",
        "part": "bootcore",
        "symbols": ["DAGON_DECIDE_US"],
        # Stage0's check and the decision's check of the app.
        "wait_s": 65,
    },
    # The app times every critical section of DAGON's guard while it runs;
    # the user drives the node in between (read only).
    "masked-time": {
        "package": "app",
        "feature": "dagon-mcu-{mcu}/masked-time",
        "part": None,
        "symbols": ["DAGON_MASKED_MAX_US", "DAGON_MASKED_COUNT"],
        # The boot to the app: stage0's check and the boot decision.
        "wait_s": 65,
    },
}
# The margin the listen window may add beyond itself.
LISTEN_MARGIN_MS = 10
# The longest `[execution] masked_max_us` a definition may state
# (dagon-topology MASKED_MAX_US_LIMIT).
MASKED_MAX_US_LIMIT = 1000


# ---- files -------------------------------------------------------------


def with_default_feature(text, feature):
    """Cargo.toml `text` with `feature` added to `[features] default`
    (made when missing); the rest of the file unchanged."""
    lines = text.splitlines(keepends=True)
    start = next((i for i, l in enumerate(lines) if l.strip() == "[features]"), None)
    if start is None:
        nl = "" if text.endswith("\n") or not text else "\n"
        return f'{text}{nl}\n[features]\ndefault = ["{feature}"]\n'
    end = next((i for i in range(start + 1, len(lines)) if lines[i].lstrip().startswith("[")), len(lines))
    for i in range(start + 1, end):
        m = re.match(r'^(\s*default\s*=\s*\[)(.*)\]\s*$', lines[i])
        if m:
            have = [s.strip() for s in m.group(2).split(",") if s.strip()]
            if f'"{feature}"' in have:
                return text
            lines[i] = f"{m.group(1)}{', '.join(have + [chr(34) + feature + chr(34)])}]\n"
            return "".join(lines)
    lines.insert(start + 1, f'default = ["{feature}"]\n')
    return "".join(lines)


class Switched:
    """`path` with `feature` in its default features while inside; the
    original bytes back afterwards, also on an error or Ctrl-C."""

    def __init__(self, path, feature):
        self.path = Path(path)
        self.feature = feature
        self.backup = self.path.with_name(self.path.name + BACKUP)

    def __enter__(self):
        if self.backup.exists():
            raise SystemExit(f"{self.backup} exists: a run stopped before it put the file back; run `restore` first")
        original = self.path.read_bytes()
        self.backup.write_bytes(original)
        self.path.write_bytes(with_default_feature(original.decode("utf-8"), self.feature).encode("utf-8"))
        return self

    def __exit__(self, *exc):
        self.path.write_bytes(self.backup.read_bytes())
        self.backup.unlink()
        return False


def restore(project):
    """Puts back every file a stopped run left switched; returns their paths."""
    done = []
    for root in {Path(project), source_root(project, required=False)} - {None}:
        for b in Path(root).rglob("Cargo.toml" + BACKUP):
            if "target" in b.parts:
                continue
            original = b.with_name("Cargo.toml")
            original.write_bytes(b.read_bytes())
            b.unlink()
            done.append(str(original))
    return done


def source_root(project, required=True):
    """The DAGON source `dagon flash` builds stage0 from: dagon.toml's
    `[dagon] path` (path mode; a release tag's checkout is cargo's, not
    to be edited)."""
    cfg = tomllib.loads((Path(project) / "dagon.toml").read_text(encoding="utf-8"))
    path = cfg.get("dagon", {}).get("path")
    if path is None:
        if required:
            raise SystemExit("dagon.toml has no [dagon] path: stage0-verify needs the project on a DAGON clone")
        return None
    return (Path(project) / path).resolve()


def node_definition(project, node):
    """The node's MCU definition as dagon gen wrote it (its overrides in)."""
    path = Path(project) / "boot" / node / "mcu.toml"
    if not path.is_file():
        raise SystemExit(f"{path}: missing; run `dagon gen` first")
    return tomllib.loads(path.read_text(encoding="utf-8"))


def app_dir(project, node):
    """The node's app crate: dagon.toml `[build.<node>] package`."""
    cfg = tomllib.loads((Path(project) / "dagon.toml").read_text(encoding="utf-8"))
    package = cfg.get("build", {}).get(node, {}).get("package")
    if package is None:
        raise SystemExit(f"dagon.toml has no [build.{node}] package: masked-time builds the node's app")
    return Path(project) / package


def package_file(project, node, m, mcu):
    if m["package"] == "boot":
        return Path(project) / "boot" / node / "Cargo.toml"
    if m["package"] == "app":
        return app_dir(project, node) / "Cargo.toml"
    return source_root(project) / "targets" / "mcu" / mcu / "stage0" / "Cargo.toml"


def elf_folder(project, node, m):
    """Where the measuring build's ELF is: the app crate's target folder,
    else the part's folder under out/flash/target."""
    if m["package"] == "app":
        return app_dir(project, node) / "target"
    return Path(project) / "out" / "flash" / "target" / m["part"]


def feature_of(m, mcu):
    return m["feature"].format(bootcore=f"dagon-{mcu}-bootcore", mcu=mcu)


# ---- the built ELF -----------------------------------------------------


def newest_elf(folder):
    """The newest ELF executable in `<folder>/<triple>/release/`."""
    found = [
        p for p in Path(folder).glob("*/release/*")
        if p.is_file() and not p.suffix and p.read_bytes()[:4] == b"\x7fELF"
    ]
    if not found:
        raise SystemExit(f"no built ELF in {folder}/*/release/")
    return max(found, key=lambda p: p.stat().st_mtime)


def elf_symbols(data, names):
    """{name: address} of the `names` in a 32-bit little-endian ELF's
    .symtab; a name it lacks is missing from the result."""
    if data[:4] != b"\x7fELF" or data[4] != 1 or data[5] != 1:
        raise ValueError("not a 32-bit little-endian ELF")
    shoff, = struct.unpack_from("<I", data, 0x20)
    shentsize, shnum = struct.unpack_from("<HH", data, 0x2E)
    sections = [struct.unpack_from("<IIIIIIIIII", data, shoff + i * shentsize) for i in range(shnum)]
    want = set(names)
    out = {}
    for s in sections:
        if s[1] != 2:  # SHT_SYMTAB
            continue
        strtab = sections[s[6]]
        for off in range(s[4], s[4] + s[5], s[9]):
            name_at, value = struct.unpack_from("<II", data, off)
            start = strtab[4] + name_at
            name = data[start:data.index(b"\0", start)].decode("utf-8", "replace")
            if name in want:
                out[name] = value
    return out


def stage0_clock(part_folder):
    """(source, Hz) of the clock the built stage0 runs on: the clock.rs its
    build script wrote from the node's definitions."""
    found = sorted(Path(part_folder).glob("*/release/build/*/out/clock.rs"), key=lambda p: p.stat().st_mtime)
    if not found:
        raise SystemExit(f"no clock.rs under {part_folder}: the stage0 build did not run")
    text = found[-1].read_text(encoding="utf-8")
    return parse_clock(text)


def parse_clock(text):
    source = re.search(r'pub const SOURCE: &str = "([^"]*)";', text)
    hz = re.search(r"pub const OUT_HZ: u32 = (\d+);", text)
    if not (source and hz):
        raise ValueError("clock.rs: no SOURCE or OUT_HZ")
    return source.group(1), int(hz.group(1))


# ---- the probe ---------------------------------------------------------


def parse_words(text):
    """The 32-bit words `probe-rs read b32` printed (an address ending in
    ':' is skipped)."""
    words = []
    for tok in text.split():
        if tok.endswith(":"):
            continue
        t = tok[2:] if tok.lower().startswith("0x") else tok
        if re.fullmatch(r"[0-9a-fA-F]{1,8}", t):
            words.append(int(t, 16))
    return words


def probe_read(chip, probe, address, count=1):
    cmd = ["probe-rs", "read", "--chip", chip]
    if probe:
        cmd += ["--probe", probe]
    cmd += ["b32", hex(address), str(count)]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"{' '.join(cmd)} failed: {r.stderr.strip()}")
    words = parse_words(r.stdout)
    if len(words) < count:
        raise SystemExit(f"{' '.join(cmd)}: expected {count} words, got: {r.stdout.strip()}")
    return words[:count], cmd


# ---- verdicts ----------------------------------------------------------


def stage0_verdict(cycles, count, hz):
    """The stage0-verify record: the longest check in cycles and seconds at
    the built clock; fails when no check ran."""
    ok = count >= 1 and cycles > 0
    return {
        "verify_cycles": cycles,
        "verify_count": count,
        "clock_hz": hz,
        "verify_s": round(cycles / hz, 4) if hz else None,
        "blink_code": -(-cycles // max(hz // 10, 1)) if hz else None,
        "verdict": "PASS" if ok else "FAIL",
        "why": None if ok else "no signature check was timed (no boot core to check?)",
    }


def blink_verdict(blinks, hz):
    """The record from a counted blink code (tenths of a second, rounded up)."""
    ok = 1 <= blinks <= 254
    return {
        "blink_code": blinks,
        "verify_s_at_most": round(blinks / 10, 1),
        "clock_hz": hz,
        "verdict": "PASS" if ok else "FAIL",
        "why": None if ok else "a code of 0 or 255 is no reading (255: 25.5 s or longer)",
    }


def groups_us(text):
    """Microseconds from the blink groups a boot core's measuring build
    blinks on its `[boot] led` light (the display backlight on the Waveshare
    RP2040-Touch-LCD-1.28): one count per decimal digit of the milliseconds,
    ten for a 0. The milliseconds are rounded up, so this is at most 1 ms
    over the value the node keeps."""
    groups = [int(g) for g in text.split()]
    if not groups or not all(1 <= g <= 10 for g in groups):
        raise ValueError(f"{text!r}: give each group's count, 1 to 10 (10 for a 0)")
    return int("".join(str(g % 10) for g in groups)) * 1000


def listen_verdict(us, window_ms):
    limit_us = (window_ms + LISTEN_MARGIN_MS) * 1000
    ok = 0 < us <= limit_us
    return {
        "listen_us": us,
        "window_ms": window_ms,
        "limit_us": limit_us,
        "verdict": "PASS" if ok else "FAIL",
        "why": None if ok else ("the window never ended (0)" if us == 0 else f"{us} us > {limit_us} us"),
    }


def masked_verdict(us, count):
    """The longest time DAGON's own code masked interrupts: PASS when a
    section was timed and the longest is within what a definition may
    state; the value to write as `[execution] masked_max_us` goes in the
    record."""
    ok = count >= 1 and 0 < us <= MASKED_MAX_US_LIMIT
    why = None
    if count == 0:
        why = "no critical section was timed (the app did not start?)"
    elif not ok:
        why = f"{us} us is not within 1..={MASKED_MAX_US_LIMIT} us"
    return {
        "masked_max_us": us,
        "masked_count": count,
        "limit_us": MASKED_MAX_US_LIMIT,
        "definition": f"masked_max_us = {us}" if ok else None,
        "verdict": "PASS" if ok else "FAIL",
        "why": why,
    }


def decide_verdict(us):
    """The boot decision's time; it has no limit of its own (the boot
    core feeds its watchdog through the check), so only a taken decision
    (more than 0) passes."""
    ok = us > 0
    return {
        "decide_us": us,
        "decide_s": round(us / 1e6, 4),
        "verdict": "PASS" if ok else "FAIL",
        "why": None if ok else "the decision was never taken (0)",
    }


# ---- runs --------------------------------------------------------------


def flash_command(a, writer):
    """dagon flash of the node through its own writer: `--probe` goes to a
    probe-rs writer only (a UF2 writer takes `--drive`; the probe then only
    reads)."""
    cmd = [a.dagon, "flash", a.node, "--key", a.key]
    if writer.get("kind") == "probe-rs" and a.probe:
        cmd += ["--probe", a.probe]
    if writer.get("kind") != "probe-rs" and getattr(a, "drive", None):
        cmd += ["--drive", a.drive]
    return cmd


def run_flash(cmd, project):
    """Runs dagon flash; its plan and question go to the terminal."""
    rc = subprocess.run(cmd, cwd=project).returncode
    if rc != 0:
        raise SystemExit(f"dagon flash exited {rc}")


def read_chip(a, writer):
    """The chip probe-rs reads: `--chip`, else the probe-rs writer's; None
    when the value is read as a blink code."""
    if getattr(a, "chip", None):
        return a.chip
    if writer.get("kind") == "probe-rs":
        return writer["chip"]
    if a.probe:
        raise SystemExit("--probe on a node whose writer is not probe-rs needs --chip (probe-rs chip name, e.g. RP2040)")
    return None


def measure(a, name, flash=run_flash, read=probe_read, wait=time.sleep, ask=input):
    m = MEASUREMENTS[name]
    d = node_definition(a.project, a.node)
    mcu = d["id"]
    writer = d.get("writer", {})
    chip = read_chip(a, writer)
    out = Path(a.out or Path.home() / "dagon-measure") / dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    out.mkdir(parents=True)
    steps = []

    def log(rec):
        rec = {"time": dt.datetime.now().isoformat(timespec="seconds"), **rec}
        steps.append(rec)
        print(json.dumps(rec, ensure_ascii=False))

    def write():
        cmd = flash_command(a, writer)
        log({"command": cmd})
        flash(cmd, a.project)

    part = elf_folder(a.project, a.node, m)
    result = {"measurement": name, "node": a.node, "mcu": mcu}
    wrote = False
    try:
        if name == "masked-time" and not chip:
            raise SystemExit("masked-time reads the node by probe only: give --probe (and --chip on a UF2 node)")
        with Switched(package_file(a.project, a.node, m, mcu), feature_of(m, mcu)):
            wrote = True
            write()
        elf = newest_elf(part)
        syms = elf_symbols(elf.read_bytes(), m["symbols"])
        missing = [s for s in m["symbols"] if s not in syms]
        if missing:
            raise SystemExit(f"{elf}: no symbol {', '.join(missing)}: not the measuring build")
        log({"elf": str(elf), "symbols": {k: hex(v) for k, v in syms.items()}})
        print(f"waiting {m['wait_s']} s for the node to finish ...")
        wait(m["wait_s"])
        if name == "stage0-verify":
            source, hz = stage0_clock(part)
            result["clock_source"] = source
            if chip:
                (cycles,), cmd = read(chip, a.probe, syms["DAGON_VERIFY_CYCLES"])
                (count,), _ = read(chip, a.probe, syms["DAGON_VERIFY_COUNT"])
                log({"command": cmd, "cycles": cycles, "count": count})
                result.update(stage0_verdict(cycles, count, hz))
            else:
                print("Count the fast blinks between two dark pauses on the board's diagnostic output.")
                blinks = int(ask("blinks: ").strip())
                result.update(blink_verdict(blinks, hz))
        elif name == "masked-time":
            print("Talk to the node for a while without writing it (e.g. `dagon status --port <COMn>` "
                  "a few times, and let its app run), then press Enter.")
            ask("done? ")
            (us,), cmd = read(chip, a.probe, syms["DAGON_MASKED_MAX_US"])
            (count,), _ = read(chip, a.probe, syms["DAGON_MASKED_COUNT"])
            log({"command": cmd, "masked_max_us": us, "count": count})
            result.update(masked_verdict(us, count))
        else:
            (symbol,) = m["symbols"]
            if chip:
                (us,), cmd = read(chip, a.probe, syms[symbol])
                log({"command": cmd, "us": us})
            else:
                print("Count the blinks of each group on the board's boot light ([boot] led; the display "
                      "backlight on the Waveshare RP2040-Touch-LCD-1.28), between two dark pauses "
                      "(ten blinks for a 0), and give the counts in order, e.g. 5 2.")
                us = groups_us(ask("groups: "))
                log({"groups_us": us})
            if name == "listen-time":
                window = d.get("boot_receive", {}).get("listen_window_ms", 50)
                result.update(listen_verdict(us, window))
            else:
                result.update(decide_verdict(us))
    except (Exception, KeyboardInterrupt, SystemExit) as e:
        # Recorded, and the exit code waits for the write back below.
        result["verdict"] = "FAIL"
        result["error"] = str(e) or type(e).__name__
    finally:
        # Only a node the measuring build was written to (or may have been,
        # when the write stopped) gets its normal build back.
        result["restored"] = not wrote
        if wrote:
            print("writing the node's normal build back ...")
            try:
                write()
                result["restored"] = True
            except (Exception, SystemExit) as e:
                result["restore_error"] = str(e) or type(e).__name__
        (out / "steps.json").write_text(json.dumps(steps, indent=2, ensure_ascii=False), encoding="utf-8")
        (out / "result.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
        print(json.dumps(result, indent=2, ensure_ascii=False))
        print(f"recorded in {out}")
    if not result.get("restored"):
        return 2
    return 0 if result.get("verdict") == "PASS" else 1


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in MEASUREMENTS:
        s = sub.add_parser(name)
        s.add_argument("--project", required=True)
        s.add_argument("--dagon", required=True)
        s.add_argument("--node", required=True)
        s.add_argument("--key", required=True, help="secret key file (path only, passed to dagon)")
        s.add_argument("--probe", help="probe-rs probe selector (reads the value; writes through it only on a probe-rs writer)")
        s.add_argument("--chip", help="probe-rs chip to read (default: the probe-rs writer's)")
        s.add_argument("--drive", help="UF2 drive the node is written through (a UF2 writer)")
        s.add_argument("--out", help="record folder (default ~/dagon-measure)")
    r = sub.add_parser("restore")
    r.add_argument("--project", required=True)
    sub.add_parser("selftest")
    a = p.parse_args(argv)
    if a.cmd == "restore":
        for f in restore(a.project):
            print(f"put back {f}")
        return 0
    if a.cmd == "selftest":
        import unittest

        suite = unittest.defaultTestLoader.discover(str(HERE), pattern="test_measure.py")
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    return measure(a, a.cmd)


if __name__ == "__main__":
    sys.exit(main())
