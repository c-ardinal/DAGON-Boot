#!/usr/bin/env python3
"""Desk hardware test: drives the real desk (hub on USB, seg on I2C, watch
on UART) through the dagon CLI, scenario by scenario (scenarios.toml), and
records every step: time, command, exit code and the status of every node.
Stops at the first unexpected state. See README.md.

    python hwt.py sync --project <desk> --repo <DAGON-Boot clone> --dagon <exe>
    python hwt.py run  --project <desk> --dagon <exe> --key <secret key file>
                       [--out <dir>] [--only <id>...] [--from <id>]
    python hwt.py selftest
"""

import argparse
import datetime as dt
import json
import re
import shutil
import subprocess
import sys
import time
import zlib
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
NODES = ["seg", "watch", "hub"]  # every [build.<id>] of the desk, update order
FAIL_FEATURE = "no-confirm"
SETTLE_TIMEOUT_S = 360  # hub: 3 trial attempts of ~70 s each
POLL_S = 5
STATUS_TRIES = 6

LINE = re.compile(
    r"^(?P<id>[\w-]+): version (?P<version>\S+)(?P<trial> \(on trial\))?, mcu \S+, "
    r"active (?P<active>\d+), confirmed (?P<confirmed>\d+), attempts (?P<attempts>\d+), "
    r"last reason (?P<reason>\d+), \[(?P<slots>.*)\]$"
)
SLOT = re.compile(r"([ABF]):(\w+) ([^,\s]+)")


def parse_status(text):
    """`dagon status` output -> {node: {...}}; a node without an answer maps to None."""
    out = {}
    for line in text.splitlines():
        m = LINE.match(line.strip())
        if m:
            slots = {s: {"state": st, "version": v} for s, st, v in SLOT.findall(m["slots"])}
            out[m["id"]] = {
                "version": m["version"],
                "trial": bool(m["trial"]),
                "active": int(m["active"]),
                "confirmed": int(m["confirmed"]),
                "attempts": int(m["attempts"]),
                "last_reason": int(m["reason"]),
                "slots": slots,
            }
        elif re.match(r"^[\w-]+: (no answer|not reachable)", line.strip()):
            out[line.split(":")[0]] = None
    return out


def settled(n):
    """Running a confirmed version: not on trial, active slot is the confirmed one."""
    return n is not None and not n["trial"] and n["active"] == n["confirmed"]


def core(n):
    return n and {k: n[k] for k in ("version", "trial", "active", "confirmed")}


class Fail(Exception):
    pass


class Run:
    def __init__(self, a):
        self.project = Path(a.project).resolve()
        self.dagon = str(Path(a.dagon).resolve())
        self.key = a.key  # path only; never opened here
        stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
        self.out = Path(a.out or Path.home() / "dagon-hwt" / stamp).resolve() / stamp
        self.out.mkdir(parents=True, exist_ok=True)
        self.n = 0
        self.steps = self.out / "steps.jsonl"
        self.results = []

    # -- running the CLI ----------------------------------------------------

    def cli(self, args, name, stdin=None, config=None, interrupt=None):
        """Runs dagon; output in <out>/<nn>-<name>.log. `interrupt=(node, pct)`
        kills it when that node's transfer reaches pct. Returns (rc, text, cmd)."""
        self.n += 1
        log = self.out / f"{self.n:03d}-{name}.log"
        cmd = [self.dagon, *args]
        if config is not False:  # key gen takes no --config
            cmd += ["--config", str(config or self.project / "dagon.toml")]
        shown = " ".join(cmd)
        t0 = now()
        p = subprocess.Popen(cmd, cwd=self.project, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, encoding="utf-8", errors="replace", bufsize=1)
        p.stdin.write(stdin or "")
        p.stdin.close()
        lines, killed = [], False
        want = interrupt and f"{interrupt[0]}: transfer {interrupt[1]}%"
        for line in p.stdout:
            lines.append(line)
            if want and line.startswith(want):
                p.kill()
                killed = True
                lines.append(f"[hwt: killed the CLI at '{want}' {now()}]\n")
                break
        p.stdout.close()
        rc = p.wait()
        text = "".join(lines)
        log.write_text(f"$ {shown}\n# start {t0}\n{text}# rc={rc} end {now()}\n", encoding="utf-8")
        if want and not killed:
            raise Fail(f"transfer never reached {interrupt[1]}% (see {log.name})")
        return rc, text, shown, log

    def status(self, label):
        """Status of every node (retried while the hub is away, e.g. rebooting)."""
        for i in range(STATUS_TRIES):
            rc, text, cmd, log = self.cli(["status"], f"status-{label}")
            st = parse_status(text)
            if rc == 0 and all(k in st for k in NODES):
                return st
            time.sleep(POLL_S)
        raise Fail(f"dagon status failed {STATUS_TRIES} times (last {log.name})")

    def record(self, sc, step, cmd, rc, log, st, verdict, note=""):
        rec = {"time": now(), "scenario": sc, "step": step, "command": cmd, "rc": rc,
               "log": log.name if log else None, "status": st, "verdict": verdict, "note": note}
        with self.steps.open("a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        print(f"[{rec['time']}] {sc}/{step}: {verdict} rc={rc} {note}")
        for k in NODES:
            n = st.get(k) if st else None
            print(f"    {k}: " + (f"{n['version']} {'TRIAL ' if n['trial'] else ''}"
                                   f"active {n['active']} confirmed {n['confirmed']} "
                                   + " ".join(f"{s}:{v['state']} {v['version']}"
                                              for s, v in n['slots'].items()) if n else "-"))

    def wait_settled(self, want, label):
        """Polls until every node in `want` ({node: version}) runs it, confirmed."""
        end = time.time() + SETTLE_TIMEOUT_S
        while True:
            st = self.status(label)
            if all(settled(st[k]) and st[k]["version"] == v for k, v in want.items()):
                return st
            if time.time() > end:
                raise Fail(f"not settled on {want} after {SETTLE_TIMEOUT_S} s")
            time.sleep(POLL_S)

    def fail_config(self, nodes):
        """dagon.hwt-fail.toml: dagon.toml with `features = ["no-confirm"]` for `nodes`."""
        text = (self.project / "dagon.toml").read_text(encoding="utf-8")
        for n in nodes:
            head = f"[build.{n}]\n"
            if head not in text:
                raise Fail(f"no [build.{n}] in dagon.toml")
            text = text.replace(head, f'{head}features = ["{FAIL_FEATURE}"]\n', 1)
        p = self.project / "dagon.hwt-fail.toml"
        p.write_text(text, encoding="utf-8")
        return p

    # -- steps --------------------------------------------------------------

    def step(self, sc, i, s):
        kind = s["do"]
        name = f"{sc}-{i}-{kind}"
        before = self.status(f"{name}-before") if kind != "skip" else None
        if kind == "status":
            bad = [k for k in NODES if not settled(before[k])]
            if bad:
                raise Fail(f"not on a confirmed version: {bad}")
            return "-", 0, None, before, ""
        if kind == "skip":
            return "-", None, None, None, s["reason"]
        if kind == "deploy":
            return self.deploy(name, s, before)
        if kind in ("tamper", "foreign"):
            return self.refused(name, s, before)
        if kind == "rollback":
            node = s["node"]
            other = 1 - before[node]["active"]
            want = before[node]["slots"]["AB"[other]]["version"]
            rc, _, cmd, log = self.cli(["rollback", "--node", node], name)
            st = self.status(f"{name}-after")
            expect(rc == 0, f"rollback rc={rc}")
            check_on(st, node, want, flipped_from=before[node]["active"])
            unchanged(before, st, [k for k in NODES if k != node])
            return cmd, rc, log, st, f"{node} back on {want}"
        if kind == "recover":
            node, v = s["node"], s["of"]
            dgb = self.project / "out" / "deploy" / f"v{v}.dgb"
            rc, _, cmd, log = self.cli(["recover", node, "--release", str(dgb)], name)
            expect(rc == 0, f"recover rc={rc}")
            st = self.wait_settled({node: v}, f"{name}-after")
            return cmd, rc, log, st, ""
        raise Fail(f"unknown step kind {kind}")

    def deploy(self, name, s, before):
        nodes = s.get("nodes", NODES)
        v = s["version"]
        mode = s.get("expect", "unchanged" if "interrupt_at" in s else "ok")
        cfg = self.fail_config(nodes) if mode == "rolled-back" else None
        inter = (nodes[0], s["interrupt_at"]) if "interrupt_at" in s else None
        args = ["deploy", *s.get("nodes", []), "--version", v, "--key", self.key]
        rc, text, cmd, log = self.cli(args, name, stdin="y\n", config=cfg, interrupt=inter)
        if mode == "ok":
            expect(rc == 0, f"deploy rc={rc}")
            st = self.status(f"{name}-after")
            for k in nodes:
                check_on(st, k, v, flipped_from=before[k]["active"])
            unchanged(before, st, [k for k in NODES if k not in nodes])
            return cmd, rc, log, st, ""
        if mode == "rolled-back":
            expect(rc != 0, "deploy of a failing self-check image succeeded")
            want = {k: before[k]["version"] for k in nodes}
            st = self.wait_settled(want, f"{name}-after")
            for k in nodes:
                expect(st[k]["active"] == before[k]["active"], f"{k}: active slot changed")
            unchanged(before, st, [k for k in NODES if k not in nodes])
            return cmd, rc, log, st, f"rolled back to {want}"
        # interrupted
        time.sleep(POLL_S)
        st = self.wait_settled({k: before[k]["version"] for k in NODES}, f"{name}-after")
        for k in NODES:
            expect(core(st[k]) == core(before[k]), f"{k}: changed after the interrupt")
        return cmd, rc, log, st, f"killed at {inter[1]}%"

    def refused(self, name, s, before):
        if s["do"] == "tamper":
            src = self.project / "out" / "deploy" / f"v{s['of']}.dgb"
            dgb = self.out / f"{name}.dgb"
            dgb.write_bytes(tamper(src.read_bytes(), s.get("fix_crc", False)))
        else:
            dgb = self.foreign_release(name, s)
        rc, text, cmd, log = self.cli(["apply", str(dgb)], name)
        st = self.status(f"{name}-after")
        expect(rc != 0 and "apply refused" in text, f"apply not refused (rc={rc})")
        expect("transfer" not in text, "a transfer started")
        unchanged(before, st, NODES, slots=True)
        return cmd, rc, log, st, text.strip().splitlines()[-1][:160]

    def foreign_release(self, name, s):
        """The node's last ELFs signed with a throwaway key made for this run."""
        kdir = self.out / "throwaway-key"
        if not kdir.exists():
            rc, _, _, _ = self.cli(["key", "gen", "--out", str(kdir)], f"{name}-keygen",
                                  config=False)
            expect(rc == 0, "key gen failed")
        secret = next(p for p in kdir.iterdir() if p.suffix != ".pub")
        node, v = s["node"], s["version"]
        build = self.project / "out" / "build" / node
        imgs = []
        for slot in "ab":
            img = self.out / f"foreign-{node}-{slot}.img"
            rc, _, _, _ = self.cli(["image", "sign", str(build / f"app-{slot}.elf"), "--node", node,
                                    "--version", v, "--slot", slot, "--out", str(img),
                                    "--key", str(secret)], f"{name}-sign-{slot}")
            expect(rc == 0, "image sign with the throwaway key failed")
            imgs.append(str(img))
        dgb = self.out / "foreign.dgb"
        rc, _, _, _ = self.cli(["release", "build", *imgs, "--name", "foreign", "--out", str(dgb),
                                "--key", str(secret)], f"{name}-release")
        expect(rc == 0, "release build with the throwaway key failed")
        return dgb

    # -- the run ------------------------------------------------------------

    def run(self, scenarios):
        print(f"logs: {self.out}")
        ok = True
        for sc in scenarios:
            verdict = "PASS"
            for i, s in enumerate(sc["steps"]):
                try:
                    cmd, rc, log, st, note = self.step(sc["id"], i, s)
                    v = "SKIPPED" if s["do"] == "skip" else "ok"
                    self.record(sc["id"], i, cmd, rc, log, st, v, note)
                    if s["do"] == "skip":
                        verdict = "SKIPPED"
                except Fail as e:
                    self.record(sc["id"], i, s, None, None, None, "UNEXPECTED", str(e))
                    verdict, ok = f"FAIL: {e}", False
                    break
            self.results.append({"id": sc["id"], "title": sc["title"], "result": verdict})
            if not ok:
                break
        final = None if not ok else self.status("final")
        (self.out / "results.json").write_text(
            json.dumps({"results": self.results, "final_status": final}, indent=2,
                       ensure_ascii=False), encoding="utf-8")
        print("\n".join(f"{r['id']:<16} {r['result']}" for r in self.results))
        if not ok:
            print("STOPPED at the first unexpected state; nodes left as they are.")
        return 0 if ok else 1


def tamper(dgb, fix_crc):
    """Flips 4 payload bytes of the bundle's first image item; with `fix_crc`
    its item CRC-32 is recomputed, so only the digest/signature chain can
    tell (layout: dagon-format bundle.rs)."""
    data = bytearray(dgb)
    k = data[6]
    for i in range(k):
        e = 16 + 16 * i
        if data[e + 1] in (1, 2):  # HUB_DIRECT / SUB_STORE image item
            off, length = (int.from_bytes(data[e + o:e + o + 4], "little") for o in (4, 8))
            mid = off + 512 + (length - 512) // 2
            for j in range(mid, mid + 4):
                data[j] ^= 0xFF
            if fix_crc:
                crc = zlib.crc32(data[off:off + length])
                data[e + 12:e + 16] = crc.to_bytes(4, "little")
            return bytes(data)
    raise Fail("no image item in the release")


def now():
    return dt.datetime.now().isoformat(timespec="milliseconds")


def expect(cond, msg):
    if not cond:
        raise Fail(msg)


def check_on(st, node, v, flipped_from):
    n = st[node]
    expect(settled(n) and n["version"] == v, f"{node}: want {v} confirmed, got {core(n)}")
    expect(n["active"] != flipped_from, f"{node}: active slot did not change")


def unchanged(before, after, nodes, slots=False):
    for k in nodes:
        a, b = (before[k], after[k]) if slots else (core(before[k]), core(after[k]))
        if slots and a and b:
            a = {**core(a), "slots": a["slots"]}
            b = {**core(b), "slots": b["slots"]}
        expect(a == b, f"{k} changed: {a} -> {b}")


# -- sync: the desk's node crates from the repo samples ----------------------

HUB_CHECK_OLD = "pub fn self_check() -> bool {\n    true\n}"
HUB_CHECK_NEW = f'pub fn self_check() -> bool {{\n    !cfg!(feature = "{FAIL_FEATURE}")\n}}'
HUB_FEATURE = f'\n# Test build whose self-check fails (hardware-test/desk).\n{FAIL_FEATURE} = []\n'


def sync(a):
    """Copies samples/f401-app -> nodes/seg and samples/esp32s3-app ->
    nodes/watch-app (paths to the repo), gives the hub app the no-confirm
    switch and runs dagon gen hub."""
    project, repo = Path(a.project).resolve(), Path(a.repo).resolve()
    for sample, node in (("f401-app", "seg"), ("esp32s3-app", "watch-app")):
        src, dst = repo / "samples" / sample, project / "nodes" / node
        shutil.rmtree(dst / "src", ignore_errors=True)
        for p in src.rglob("*"):
            rel = p.relative_to(src)
            if p.is_dir() or rel.parts[0] == "target" or p.name == "README.md":
                continue
            out = dst / rel
            out.parent.mkdir(parents=True, exist_ok=True)
            if p.name == "Cargo.toml":
                text = p.read_text(encoding="utf-8")
                out.write_text(text.replace('path = "../../', f'path = "{repo.as_posix()}/'),
                               encoding="utf-8", newline="\n")
            else:
                shutil.copy2(p, out)
    hub = project / "nodes" / "hub"
    app = hub / "src" / "app.rs"
    text = app.read_text(encoding="utf-8").replace("\r\n", "\n")
    if HUB_CHECK_NEW not in text:
        if HUB_CHECK_OLD not in text:
            sys.exit(f"{app}: self_check() is not the generated one; add the {FAIL_FEATURE} switch by hand")
        app.write_text(text.replace(HUB_CHECK_OLD, HUB_CHECK_NEW), encoding="utf-8", newline="\n")
    cargo = hub / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    if f"\n{FAIL_FEATURE} = []" not in text:
        text = text.replace("\n[features]\n", "\n[features]" + HUB_FEATURE, 1)
        cargo.write_text(text, encoding="utf-8", newline="\n")
    rc = subprocess.call([str(Path(a.dagon).resolve()), "gen", "hub"], cwd=project)
    print("synced; dagon gen hub rc =", rc)
    return rc


def selftest():
    text = ("hub: version 1.4.0, mcu 0x0001, active 1, confirmed 1, attempts 0, last reason 3, "
            "[A:Written 1.3.0, B:Confirmed 1.4.0, F:Empty 0.0.0]\n"
            "seg: version 2.0.5 (on trial), mcu 0x0002, active 0, confirmed 1, attempts 1, "
            "last reason 0, [A:Pending 2.0.5, B:Confirmed 1.4.1, F:Empty 0.0.0]\n"
            "watch: no answer (Timeout)\n")
    st = parse_status(text)
    assert st["hub"]["version"] == "1.4.0" and settled(st["hub"])
    assert st["hub"]["slots"]["A"] == {"state": "Written", "version": "1.3.0"}
    assert st["seg"]["trial"] and not settled(st["seg"])
    assert st["watch"] is None and not settled(st["watch"])
    try:
        unchanged(st, {**st, "hub": {**st["hub"], "active": 0}}, ["hub"])
        raise AssertionError("change not seen")
    except Fail:
        pass
    body = bytes(range(256)) * 4  # 512-byte header + 512-byte payload
    raw = bytearray(64) + body
    raw[6], raw[16 + 1] = 1, 2
    raw[16 + 4:16 + 8] = (64).to_bytes(4, "little")
    raw[16 + 8:16 + 12] = len(body).to_bytes(4, "little")
    raw[16 + 12:16 + 16] = zlib.crc32(body).to_bytes(4, "little")
    for fix in (False, True):
        t = tamper(bytes(raw), fix)
        assert t[64:64 + 512] == body[:512] and t[64 + 512:] != body[512:]
        assert (int.from_bytes(t[28:32], "little") == zlib.crc32(t[64:])) == fix
    print("selftest ok")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("sync")
    s.add_argument("--project", required=True)
    s.add_argument("--repo", required=True)
    s.add_argument("--dagon", required=True)
    r = sub.add_parser("run")
    r.add_argument("--project", required=True)
    r.add_argument("--dagon", required=True)
    r.add_argument("--key", required=True, help="secret key file (path only)")
    r.add_argument("--out", help="log folder (default ~/dagon-hwt)")
    r.add_argument("--scenarios", default=str(HERE / "scenarios.toml"))
    r.add_argument("--only", nargs="*", help="scenario ids to run")
    r.add_argument("--from", dest="start", help="start at this scenario id")
    sub.add_parser("selftest")
    a = ap.parse_args()
    if a.cmd == "selftest":
        return selftest()
    if a.cmd == "sync":
        return sync(a)
    scs = tomllib.loads(Path(a.scenarios).read_text(encoding="utf-8"))["scenario"]
    if a.start:
        scs = scs[[s["id"] for s in scs].index(a.start):]
    if a.only:
        scs = [s for s in scs if s["id"] in a.only]
    return Run(a).run(scs)


if __name__ == "__main__":
    sys.exit(main())
