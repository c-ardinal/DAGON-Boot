#!/usr/bin/env python3
"""The ESP32-S3 stage0 hardware run: DAGON's own
stage0 on the M5StopWatch in place of the ESP-IDF 2nd stage, with the test
boot core and app of the emulator test, to measure what the definition's
stage0 layout waits for. See README.md, "ESP32-S3 stage0 run".

    python esp_stage0.py prepare  [--app-image <ESP app image>] [--out <dir>]
    python esp_stage0.py run      --port <COMn> [--out <dir>] [--manual-reset]
    python esp_stage0.py restore  --port <COMn> [--out <dir>]
    python esp_stage0.py selftest

`prepare` builds stage0's measuring build (feature `measure`) and the test
images from this DAGON source, and writes the flash images (good node,
broken app signature, and with `--app-image` your own app in APP slot A).
`run` reads the whole flash into `backup.bin` first (once; an existing
backup is kept), then writes each image raw (`espflash write-bin`), resets
the node, reads its ROM text output, and records the measurement; at the
end it writes the backup back. `restore` writes the backup back after a
stopped run.

The images carry the RFC 8032 test key and stage0's emulator layout: they
are for this run only, and the run ends with the backup written back.
"""

import argparse
import datetime as dt
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
ESP = ROOT / "targets" / "mcu" / "esp32s3"
ELF = Path("target") / "xtensa-esp32s3-none-elf" / "release"
CHIP = "esp32s3"
TAG = "qemu-node"

# The system timer counts 16 MHz from the 40 MHz crystal: the reference.
SYSTIMER_HZ = 16_000_000
# stage0/budget.toml [image] max_bytes, and the definition's trial
# watchdog period ([watchdog] trial_ms): the run's limits.
STAGE0_MAX_BYTES = 30_720
TRIAL_WDT_MS = 4_000
# The size of the app body the definition's slots allow (3 MiB), for the
# hash time a full slot takes.
FULL_BODY = 0x30_0000

APP_RUNS = "app: running from flash"
REFUSED = "core: recovery mode, refusal 2"

LINE = re.compile(r"^(measure(?:-1)?): (.*)$")
FIELDS = [
    (re.compile(r"rom (\d+) MHz, reset (\d+), path (\d+)"), ("rom_mhz", "reset", "path")),
    (re.compile(r"cycles (\d+), systimer (\d+), rtc (\d+)"), ("cycles", "systimer", "rtc")),
    (re.compile(r"verify (\d+) cycles in (\d+), hash (\d+) cycles for (\d+) bytes"),
     ("verify_cycles", "verify_count", "hash_cycles", "hash_bytes")),
    (re.compile(r"earlier (\d+), changed (\d+), ([0-9a-f]{8})\.\.([0-9a-f]{8})"),
     ("earlier", "changed", "changed_first", "changed_last")),
]
STORE = re.compile(r"store ([0-9a-f]{8}) ([0-9a-f]{8}) ([0-9a-f]{8}) ([0-9a-f]{8})")
HEX = {"changed_first", "changed_last"}


# ---- reading the output -----------------------------------------------


def records(lines):
    """The measurement records in the ROM text output, in order: one dict
    per record (tag `measure` for the start's own, `measure-1` for the
    start before it), with its STORE words in `store`."""
    out = []
    for line in lines:
        m = LINE.match(line.strip())
        if not m:
            continue
        tag, rest = m.groups()
        if rest.startswith("rom ") or not out or out[-1]["tag"] != tag or out[-1].get("done"):
            out.append({"tag": tag, "store": []})
        rec = out[-1]
        s = STORE.match(rest)
        if s:
            rec["store"] += [int(w, 16) for w in s.groups()]
            rec["done"] = len(rec["store"]) == 8
            continue
        for pat, names in FIELDS:
            f = pat.match(rest)
            if f:
                for n, v in zip(names, f.groups()):
                    rec[n] = int(v, 16) if n in HEX else int(v)
    for r in out:
        r.pop("done", None)
    return out


def analyse(rec):
    """The values the run asks for from one record: the real CPU clock and the
    RTC slow clock against the system timer, the times of the checks, and
    the time a full slot's body hash takes at the measured rate."""
    t = rec.get("systimer", 0)
    if not t:
        return {"why": "the system timer did not count: no reference for the clocks"}
    seconds = t / SYSTIMER_HZ
    cpu_hz = rec["cycles"] / seconds
    a = {
        "cpu_hz": round(cpu_hz),
        "rtc_slow_hz": round(rec["rtc"] / seconds),
        "decision_ms": round(seconds * 1000, 3),
        "verify_ms_each": round(rec["verify_cycles"] / max(rec["verify_count"], 1) / cpu_hz * 1000, 3),
    }
    if rec["hash_bytes"]:
        per_byte = rec["hash_cycles"] / rec["hash_bytes"]
        a["hash_ms"] = round(rec["hash_cycles"] / cpu_hz * 1000, 3)
        a["hash_full_slot_ms"] = round(per_byte * FULL_BODY / cpu_hz * 1000, 1)
    return a


def used_stores(rec):
    """The RTC STORE registers (0 to 7) that hold a value other than 0 as
    the reset left them."""
    return [i for i, w in enumerate(rec.get("store", [])) if w != 0]


def verdict(stage0_bytes, boots):
    """The run's pass: stage0 at most 30 KiB, the decision (the app's start,
    its body hashed) within the trial watchdog period, and the app
    started; `boots` maps each case to its output lines."""
    why = []
    if stage0_bytes > STAGE0_MAX_BYTES:
        why.append(f"stage0 is {stage0_bytes} bytes, over {STAGE0_MAX_BYTES}")
    good = boots.get("good", [])
    if not any(l.startswith(APP_RUNS) for l in good):
        why.append("the good node's app did not tell it runs")
    apps = [r for r in records(good) if r["tag"] == "measure" and r.get("path") == 2]
    if not apps:
        why.append("no record of the app's start in the good node's output")
    for r in apps:
        ms = analyse(r).get("decision_ms")
        if ms is None or ms >= TRIAL_WDT_MS:
            why.append(f"the app's start took {ms} ms, not within {TRIAL_WDT_MS} ms")
    bad = boots.get("bad-signature", [])
    if any(l.startswith(APP_RUNS) for l in bad) or not any(l.startswith(REFUSED) for l in bad):
        why.append("the app with a broken signature was not refused")
    return {"verdict": "FAIL" if why else "PASS", "why": why}


# ---- the commands -----------------------------------------------------


def espflash(verb, port, *args):
    return ["espflash", verb, "--chip", CHIP, "--port", port, "--non-interactive", *args]


def flash_size(text):
    """Bytes from `espflash board-info`'s "Flash size: 16MB" line."""
    m = re.search(r"Flash size:\s*(\d+)\s*([KM])B", text)
    if not m:
        raise SystemExit(f"no flash size in espflash board-info output:\n{text}")
    return int(m.group(1)) << (10 if m.group(2) == "K" else 20)


def cargo_steps(app_image, images):
    """The builds `prepare` runs: (folder, command, environment)."""
    env = {"DAGON_ALLOW_TEST_KEY": "1", "DAGON_TARGET_TAG": TAG}
    stage0 = ESP / "stage0"
    tests = ESP / "stage0-qemu"
    elf = {
        "DAGON_STAGE0_ELF": stage0 / "target" / "measure" / ELF.relative_to("target") / "dagon-esp32s3-stage0",
        "DAGON_QEMU_CORE_ELF": tests / ELF / "core",
        "DAGON_QEMU_APP_ELF": tests / ELF / "app",
    }
    write = {**env, **{k: str(v) for k, v in elf.items()}, "DAGON_STAGE0_HW_OUT": str(images)}
    if app_image:
        write["DAGON_STAGE0_HW_APP"] = str(Path(app_image).resolve())
    return [
        (stage0, ["cargo", "build", "--release", "--locked", "--features", "qemu-layout,measure",
                  "--target-dir", "target/measure"], env),
        (tests, ["cargo", "build", "--release", "--locked"], env),
        (ROOT, ["cargo", "test", "-p", "dagon-cli", "--locked", "--bin", "dagon", "--",
                "--ignored", "--exact", "esp_image::qemu::esp32s3_stage0_hardware_images"], write),
    ]


def run_cmd(cmd, cwd=None, env=None, capture=False):
    print("$ " + " ".join(str(c) for c in cmd))
    r = subprocess.run(cmd, cwd=cwd, env={**os.environ, **(env or {})},
                       capture_output=capture, text=True)
    if r.returncode != 0:
        raise SystemExit(f"{cmd[0]} exited {r.returncode}" + (f": {r.stderr.strip()}" if capture else ""))
    return r.stdout if capture else ""


def out_dir(a):
    # Absolute: the image build runs in the repository root.
    return Path(a.out or Path.home() / "dagon-measure" / "esp-stage0").resolve()


def prepare(a, run=run_cmd):
    images = out_dir(a) / "images"
    images.mkdir(parents=True, exist_ok=True)
    # The images of an earlier prepare go: a run writes only this one's.
    for old in images.glob("*.bin"):
        old.unlink()
    for cwd, cmd, env in cargo_steps(a.app_image, images):
        run(cmd, cwd=cwd, env=env)
    names = sorted(p.name for p in images.glob("*.bin"))
    print(f"images in {images}: {', '.join(names)}")
    return 0


def open_serial(port, seconds=5.0):
    """The serial port, opened again while the node re-enumerates."""
    import serial  # pyserial

    until = time.monotonic() + seconds
    while True:
        try:
            return serial.Serial(port, 115200, timeout=0.2)
        except serial.SerialException:
            if time.monotonic() > until:
                raise
            time.sleep(0.2)


def hard_reset(s):
    """A reset through the USB Serial/JTAG controller's lines (RTS high
    with DTR low resets the chip; esptool's hard reset)."""
    s.dtr = False
    s.rts = True
    time.sleep(0.1)
    s.rts = False


def listen(port, seconds, reset=True, ask=input):
    """The ROM text output for `seconds` after a reset (with
    `--manual-reset` the owner presses the board's reset instead)."""
    s = open_serial(port)
    if reset:
        hard_reset(s)
    else:
        ask("reset the board now (its reset button, or off and on), then press Enter: ")
    lines, buf = [], b""
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        try:
            buf += s.read(256)
        except Exception:
            # The USB device left with the reset: read on once it is back.
            s.close()
            s = open_serial(port)
            continue
        *done, buf = buf.split(b"\n")
        for l in done:
            text = l.decode("utf-8", "replace").rstrip("\r")
            print(f"  | {text}")
            lines.append(text)
    s.close()
    return lines


def backup(a, run=run_cmd):
    f = out_dir(a) / "backup.bin"
    if f.exists():
        print(f"keeping the backup {f}")
        return f
    size = flash_size(run(espflash("board-info", a.port), capture=True))
    part = f.with_suffix(".part")
    run(espflash("read-flash", a.port, "0x0", hex(size), str(part)))
    part.replace(f)
    return f


def write(a, image, run=run_cmd):
    run(espflash("write-bin", a.port, "0x0", str(image)))


def measure(a, run=run_cmd, hear=listen):
    out = out_dir(a)
    images = out / "images"
    if not (images / "good.bin").exists():
        raise SystemExit(f"no images in {images}: run `prepare` first")
    stamp = out / dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    stamp.mkdir(parents=True)
    result = {"stage0_bytes": (images / "stage0.bin").stat().st_size, "boots": {}}
    boots = result["boots"]
    saved = backup(a, run)
    wrote = False
    try:
        cases = [("good", 2), ("bad-signature", 1)]
        if (images / "own-app.bin").exists():
            cases.append(("own-app", 2))
        for name, starts in cases:
            # A write that fails part way has already changed the flash.
            wrote = True
            write(a, images / f"{name}.bin", run)
            # Each start after the first shows what the one before left
            # (the RTC STORE words, the RTC slow memory an app changed).
            for i in range(starts):
                if name == "own-app" and i == 1:
                    print("let your app run for a while (as it does on the board) ...")
                    time.sleep(a.app_seconds)
                boots[f"{name}" if i == 0 else f"{name}-{i + 1}"] = hear(
                    a.port, a.listen_seconds, reset=not a.manual_reset)
        result.update(verdict(result["stage0_bytes"], boots))
        result["records"] = {k: [dict(r, **analyse(r), stores_used=used_stores(r)) for r in records(v)]
                             for k, v in boots.items()}
        if not any(boots.values()):
            result["why"].append(f"no ROM text output on {a.port}: try UART0 (GPIO43 TX, GPIO44 RX)")
    except (Exception, KeyboardInterrupt, SystemExit) as e:
        result["verdict"] = "FAIL"
        result["error"] = str(e) or type(e).__name__
    finally:
        result["restored"] = not wrote
        if wrote:
            print("writing the backup back ...")
            try:
                write(a, saved, run)
                result["restored"] = True
            except (Exception, SystemExit) as e:
                result["restore_error"] = str(e) or type(e).__name__
        for k, v in boots.items():
            (stamp / f"{k}.txt").write_text("\n".join(v) + "\n", encoding="utf-8")
        (stamp / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(json.dumps({k: v for k, v in result.items() if k != "boots"}, indent=2))
        print(f"recorded in {stamp}")
    if not result["restored"]:
        print(f"the backup was not written back: run `restore --port {a.port}`")
        return 2
    return 0 if result["verdict"] == "PASS" else 1


def restore(a, run=run_cmd):
    f = out_dir(a) / "backup.bin"
    if not f.exists():
        raise SystemExit(f"no backup at {f}")
    write(a, f, run)
    return 0


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    pr = sub.add_parser("prepare")
    pr.add_argument("--app-image", help="your app as an ESP app image (espflash save-image), for APP slot A")
    pr.add_argument("--out", help="folder (default ~/dagon-measure/esp-stage0)")
    r = sub.add_parser("run")
    r.add_argument("--port", required=True)
    r.add_argument("--out")
    r.add_argument("--manual-reset", action="store_true", help="press the board's reset when asked")
    r.add_argument("--listen-seconds", type=float, default=8.0)
    r.add_argument("--app-seconds", type=float, default=20.0)
    s = sub.add_parser("restore")
    s.add_argument("--port", required=True)
    s.add_argument("--out")
    sub.add_parser("selftest")
    a = p.parse_args(argv)
    if a.cmd == "selftest":
        import unittest

        suite = unittest.defaultTestLoader.discover(str(HERE), pattern="test_esp_stage0.py")
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    return {"prepare": prepare, "run": measure, "restore": restore}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
