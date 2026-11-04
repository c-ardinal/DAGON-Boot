#!/usr/bin/env python3
"""Desk sample hardware test: drives the desk (hub on USB, seg on I2C,
watch on UART) through the dagon CLI, scenario by scenario (hwt.toml), and
records every step: time, command, exit code and the status of every node.
Stops at the first unexpected state, then brings every node back to the
last known-good release. See README.md.

    python hwt.py sync --project <desk> --repo <DAGON-Boot clone> --dagon <exe>
    python hwt.py run  --project <desk> --dagon <exe> --key <secret key file> --port <COMn>
                       [--out <dir>] [--only <id>...] [--from <id>]
    python hwt.py selftest
"""

import argparse
import codecs
import contextlib
import datetime as dt
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROMPT = "continue? [y/N] "
# dagon commands that can write to a board; cli() marks the run as written
# before starting one, so no call site can forget.
MUTATING = {"deploy", "apply", "rollback", "recover", "flash", "provision", "rescue"}

LINE = re.compile(
    r"^(?P<id>[\w-]+): version (?P<version>\S+)(?P<trial> \(on trial\))?, mcu (?P<mcu>\S+), "
    r"(?:wire (?P<wire>\d+), )?active (?P<active>\d+), confirmed (?P<confirmed>\d+), attempts (?P<attempts>\d+), "
    r"last reason (?P<reason>\d+), \[(?P<slots>.*)\]$"
)
SLOT = re.compile(r"([ABF]):(\w+) ([^,\s]+)")
PLAN_HEAD = re.compile(r"^deploy (\S+) to (\d+) node\(s\) \((.+)\):$")
PLAN_NODE = re.compile(r"^  (\S+)\s+(\S+) -> (\S+)$")


def parse_status(text, coordinator="hub"):
    """`dagon status` output -> {node: {...}}; a node without an answer maps
    to None. The CLI names the coordinator `hub` whatever its id."""
    out = {}
    for line in text.splitlines():
        m = LINE.match(line.strip())
        if m:
            slots = {s: {"state": st, "version": v} for s, st, v in SLOT.findall(m["slots"])}
            out[m["id"]] = {
                "version": m["version"],
                "trial": bool(m["trial"]),
                "mcu": m["mcu"],
                "active": int(m["active"]),
                "confirmed": int(m["confirmed"]),
                "attempts": int(m["attempts"]),
                "last_reason": int(m["reason"]),
                "slots": slots,
            }
        elif re.match(r"^[\w-]+: (no answer|not reachable)", line.strip()):
            out[line.split(":")[0]] = None
    if "hub" in out and coordinator != "hub":
        out[coordinator] = out.pop("hub")
    return out


def check_plan(text, version, nodes, release):
    """The deploy listing before PROMPT must be exactly: `version`, the
    nodes `nodes`, each `-> version`, written to `release`. Returns the
    problem, or None."""
    lines = text.splitlines()
    heads = [i for i, l in enumerate(lines) if PLAN_HEAD.match(l)]
    if not heads or not text.endswith(PROMPT):
        return "no deploy plan before the prompt"
    v, count, path = PLAN_HEAD.match(lines[heads[-1]]).groups()
    rows = [PLAN_NODE.match(l) for l in lines[heads[-1] + 1:-1]]
    if v != version:
        return f"plan version {v}, want {version}"
    if None in rows or int(count) != len(rows):
        return "plan node lines do not match its count"
    ids = [r[1] for r in rows]
    if sorted(ids) != sorted(nodes):
        return f"plan nodes {ids}, want {nodes}"
    if any(r[3] != version for r in rows):
        return "a plan line goes to another version"
    if Path(path.replace("\\", "/")).resolve() != Path(release).resolve():
        return f"plan release {path}, want {release}"
    return None


def select(scs, only, start):
    """Scenarios to run: from `start`, then those in `only`; unknown or
    empty ids are an error, never "run everything"."""
    ids = [s["id"] for s in scs]
    if start is not None:
        if start not in ids:
            raise ValueError(f"--from {start}: no such scenario (have {ids})")
        scs = scs[ids.index(start):]
    if only is not None:
        bad = [i for i in only if i not in ids]
        if not only or bad:
            raise ValueError(f"--only needs scenario ids from {ids} (bad: {bad or 'none given'})")
        scs = [s for s in scs if s["id"] in only]
        if not scs:
            raise ValueError("--only and --from leave no scenario")
    return scs


@contextlib.contextmanager
def throwaway_dir():
    """A temporary folder outside any work tree, deleted however the block ends."""
    d = Path(tempfile.mkdtemp(prefix="hwt-key-"))
    try:
        yield d
    finally:
        shutil.rmtree(d, ignore_errors=True)


def port_identity(port):
    """Windows PnP device id of `port` (USB VID/PID and position), or None."""
    q = (f"(Get-CimInstance Win32_PnPEntity | Where-Object {{ $_.Name -like '*({port})' }})"
         ".PNPDeviceID")
    r = subprocess.run(["powershell", "-NoProfile", "-Command", q],
                       capture_output=True, text=True, timeout=60)
    return r.stdout.strip() or None


def settled(n):
    """Running a confirmed version: not on trial, active slot is the confirmed one."""
    return n is not None and not n["trial"] and n["active"] == n["confirmed"]


def core(n):
    return n and {k: n[k] for k in ("version", "trial", "active", "confirmed")}


class Fail(Exception):
    pass


class Run:
    def __init__(self, a, desk):
        self.desk = desk
        self.nodes = desk["nodes"]
        self.coord = desk["coordinator"]
        self.project = Path(a.project).resolve()
        # The CLI as an argv prefix (the selftest puts a fake one here).
        self.dagon = getattr(a, "dagon_argv", None) or [str(Path(a.dagon).resolve())]
        self.port_identity = getattr(a, "port_identity", port_identity)
        self.key = a.key  # path only; never opened here
        self.port = a.port
        stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
        self.out = Path(a.out or Path.home() / "dagon-hwt").resolve() / stamp
        self.out.mkdir(parents=True, exist_ok=True)
        self.n = 0
        self.steps = self.out / "steps.jsonl"
        self.results = []
        self.identity = None
        self.start = None
        self.good = None  # (release copy, version, sha256) last applied to every node
        self.start_state = None  # every node's status when the run began
        self.wrote = False  # a MUTATING command has been started (set only by cli())

    # -- running the CLI ----------------------------------------------------

    def cli(self, args, name, config=None, port=True, plan=None, interrupt=None):
        """Runs dagon; output in <out>/<nn>-<name>.log. `port`: pass --port.
        `plan=(version, nodes, release)`: answer the confirmation prompt y
        only when the listing is exactly that plan, else n (and fail).
        `interrupt=(node, pct)`: kill the CLI when that node's transfer
        reaches pct. Returns (rc, text, cmd, log)."""
        self.n += 1
        log = self.out / f"{self.n:03d}-{name}.log"
        cmd = [*self.dagon, *args]
        if port:
            cmd += ["--port", self.port]
        if config is not False:  # key gen, release build of a test key: no project config
            cmd += ["--config", str(config or self.project / "dagon.toml")]
        shown = " ".join(cmd)
        if args[0] in MUTATING or args[:2] == ["topo", "push"]:
            self.wrote = True
        t0 = now()
        p = subprocess.Popen(cmd, cwd=self.project, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
        dec = codecs.getincrementaldecoder("utf-8")("replace")
        text, problem, answered, killed = "", None, False, False
        want = interrupt and f"{interrupt[0]}: transfer {interrupt[1]}%"
        while chunk := p.stdout.read(4096):
            text += dec.decode(chunk)
            if not answered and text.endswith(PROMPT):
                problem = check_plan(text, *plan) if plan else "unexpected prompt"
                p.stdin.write(b"n\n" if problem else b"y\n")
                p.stdin.close()
                answered = True
                text += f"[hwt: answered {'n: ' + problem if problem else 'y'} {now()}]\n"
            if want and any(l.startswith(want) for l in text.splitlines()):
                p.kill()
                killed = True
                text += f"\n[hwt: killed the CLI at '{want}' {now()}]\n"
                break
        if not answered:
            p.stdin.close()
        p.stdout.close()
        rc = p.wait()
        log.write_text(f"$ {shown}\n# start {t0}\n{text}\n# rc={rc} end {now()}\n",
                       encoding="utf-8")
        if problem:
            raise Fail(f"refused the CLI's plan: {problem} (see {log.name})")
        if plan and not answered:
            raise Fail(f"no confirmation prompt (rc={rc}, see {log.name})")
        if want and not killed:
            raise Fail(f"transfer never reached {interrupt[1]}% (see {log.name})")
        return rc, text, shown, log

    def status(self, label):
        """Status of every node (retried while the hub is away, e.g. rebooting)."""
        for _ in range(self.desk["status_tries"]):
            rc, text, cmd, log = self.cli(["status"], f"status-{label}")
            st = parse_status(text, self.coord)
            if rc == 0 and all(k in st for k in self.nodes):
                return st
            time.sleep(self.desk["poll_s"])
        raise Fail(f"dagon status failed {self.desk['status_tries']} times (last {log.name})")

    def check_identity(self, label):
        """The pinned port is still the same hub: PnP id (USB VID/PID, position)
        and the coordinator's MCU id as at the start."""
        pnp = self.port_identity(self.port)
        st = self.status(f"{label}-identity")
        ident = {"pnp": pnp, "mcu": st[self.coord] and st[self.coord]["mcu"]}
        if self.identity is None:
            if not pnp or self.desk["hub_usb"].upper() not in pnp.upper():
                raise Fail(f"{self.port} is not the hub ({self.desk['hub_usb']}): {pnp}")
            self.identity = ident
        elif ident != self.identity:
            raise Fail(f"hub identity changed: {self.identity} -> {ident}")
        return st

    def record(self, sc, step, cmd, rc, log, st, verdict, note=""):
        rec = {"time": now(), "scenario": sc, "step": step, "command": cmd, "rc": rc,
               "log": log.name if log else None, "status": st, "verdict": verdict, "note": note}
        with self.steps.open("a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        print(f"[{rec['time']}] {sc}/{step}: {verdict} rc={rc} {note}")
        for k in self.nodes:
            print(f"    {k}: {show(st.get(k) if st else None)}")

    def wait_settled(self, want, label):
        """Polls until every node in `want` ({node: version, or None for any})
        runs it, confirmed."""
        end = time.time() + self.desk["settle_timeout_s"]
        while True:
            st = self.status(label)
            if all(settled(st[k]) and v in (None, st[k]["version"]) for k, v in want.items()):
                return st
            if time.time() > end:
                raise Fail(f"not settled on {want} after {self.desk['settle_timeout_s']} s")
            time.sleep(self.desk["poll_s"])

    def fail_config(self, nodes):
        """dagon.hwt-fail.toml: dagon.toml with the self-check-failing feature for `nodes`."""
        text = (self.project / "dagon.toml").read_text(encoding="utf-8")
        for n in nodes:
            head = f"[build.{n}]\n"
            if head not in text:
                raise Fail(f"no [build.{n}] in dagon.toml")
            text = text.replace(head, f'{head}features = ["{self.desk["fail_feature"]}"]\n', 1)
        p = self.project / "dagon.hwt-fail.toml"
        p.write_text(text, encoding="utf-8")
        return p

    # -- steps --------------------------------------------------------------

    def step(self, sc, i, s):
        kind = s["do"]
        name = f"{sc}-{i}-{kind}"
        if kind == "skip":
            return "-", None, None, None, s["reason"]
        before = self.status(f"{name}-before")
        if kind == "status":
            bad = [k for k in self.nodes if not settled(before[k])]
            if bad:
                raise Fail(f"not on a confirmed version: {bad}")
            return "-", 0, None, before, ""
        if kind == "deploy":
            return self.deploy(name, s, before)
        if kind in ("tamper", "foreign"):
            return self.refused(name, s, before)
        if kind == "rollback":
            node = s["node"]
            want = rollback_target(before[node])
            if want is None:
                raise Fail(f"{node}: other slot holds no confirmed version; nothing to roll back to")
            rc, cmd, log, st = self.rollback(node, before, name)
            return cmd, rc, log, st, f"{node} back on {want}"
        if kind == "recover":
            node, v = s["node"], s["of"]
            dgb = self.project / "out" / "deploy" / f"v{v}.dgb"
            expect(dgb.is_file(), f"no release {dgb}")
            rc, _, cmd, log = self.cli(["recover", node, "--release", str(dgb)], name)
            expect(rc == 0, f"recover rc={rc}")
            st = self.wait_settled({node: v}, f"{name}-after")
            return cmd, rc, log, st, ""
        raise Fail(f"unknown step kind {kind}")

    def rollback(self, node, before, name):
        """dagon rollback asks nothing: the caller has checked the other slot
        (rollback_target). Checks the node is on that version, confirmed, slot
        flipped, others unchanged."""
        want = rollback_target(before[node])
        rc, _, cmd, log = self.cli(["rollback", "--node", node], name)
        st = self.status(f"{name}-after")
        expect(rc == 0, f"rollback rc={rc}")
        check_on(st, node, want, flipped_from=before[node]["active"])
        unchanged(before, st, [k for k in self.nodes if k != node])
        return rc, cmd, log, st

    def deploy(self, name, s, before):
        nodes = s.get("nodes", self.nodes)
        v = s["version"]
        mode = s.get("expect", "unchanged" if "interrupt_at" in s else "ok")
        cfg = self.fail_config(nodes) if mode == "rolled-back" else None
        inter = (nodes[0], s["interrupt_at"]) if "interrupt_at" in s else None
        release = self.project / "out" / "deploy" / f"v{v}.dgb"
        args = ["deploy", *s.get("nodes", []), "--version", v, "--key", self.key]
        rc, text, cmd, log = self.cli(args, name, config=cfg, plan=(v, nodes, release),
                                      interrupt=inter)
        if mode == "ok":
            expect(rc == 0, f"deploy rc={rc}")
            st = self.status(f"{name}-after")
            for k in nodes:
                check_on(st, k, v, flipped_from=before[k]["active"])
            unchanged(before, st, [k for k in self.nodes if k not in nodes])
            if sorted(nodes) == sorted(self.nodes):
                good = self.out / f"known-good-v{v}.dgb"
                shutil.copyfile(release, good)
                self.good = (good, v, sha256(good))
            return cmd, rc, log, st, ""
        if mode == "rolled-back":
            expect(rc != 0, "deploy of a failing self-check image succeeded")
            want = {k: before[k]["version"] for k in nodes}
            st = self.wait_settled(want, f"{name}-after")
            for k in nodes:
                expect(st[k]["active"] == before[k]["active"], f"{k}: active slot changed")
            unchanged(before, st, [k for k in self.nodes if k not in nodes])
            return cmd, rc, log, st, f"rolled back to {want}"
        # interrupted
        time.sleep(self.desk["poll_s"])
        st = self.wait_settled({k: before[k]["version"] for k in self.nodes}, f"{name}-after")
        for k in self.nodes:
            expect(core(st[k]) == core(before[k]), f"{k}: changed after the interrupt")
        return cmd, rc, log, st, f"killed at {inter[1]}%"

    def refused(self, name, s, before):
        if s["do"] == "tamper":
            src = self.project / "out" / "deploy" / f"v{s['of']}.dgb"
            dgb = self.out / f"{name}.dgb"
            dgb.write_bytes(tamper(src.read_bytes(), s.get("fix_crc", False)))
            note = ""
        else:
            dgb, key_id = self.foreign_release(name, s)
            note = f"throwaway key_id {key_id}; "
        rc, text, cmd, log = self.cli(["apply", str(dgb)], name)
        st = self.status(f"{name}-after")
        expect(rc != 0 and "apply refused" in text, f"apply not refused (rc={rc})")
        expect("transfer" not in text, "a transfer started")
        unchanged(before, st, self.nodes, slots=True)
        return cmd, rc, log, st, note + text.strip().splitlines()[-1][:160]

    def foreign_release(self, name, s):
        """The node's last ELFs signed with a throwaway key; the key lives in a
        temporary folder removed when this returns. Returns (release, key_id)."""
        node, v = s["node"], s["version"]
        build = self.project / "out" / "build" / node
        with throwaway_dir() as kdir:
            rc, text, _, _ = self.cli(["key", "gen", "--out", str(kdir / "k")],
                                      f"{name}-keygen", config=False, port=False)
            expect(rc == 0, "key gen failed")
            key_id = re.search(r"key_id:\s+(\S+)", text)[1]
            secret = str(kdir / "k" / "dagon-signing.key")
            imgs = []
            for slot in "ab":
                img = self.out / f"foreign-{node}-{slot}.img"
                rc, _, _, _ = self.cli(
                    ["image", "sign", str(build / f"app-{slot}.elf"), "--node", node,
                     "--version", v, "--slot", slot, "--out", str(img), "--key", secret],
                    f"{name}-sign-{slot}", port=False)
                expect(rc == 0, "image sign with the throwaway key failed")
                imgs.append(str(img))
            dgb = self.out / "foreign.dgb"
            rc, _, _, _ = self.cli(["release", "build", *imgs, "--name", "foreign",
                                    "--out", str(dgb), "--key", secret],
                                   f"{name}-release", port=False)
            expect(rc == 0, "release build with the throwaway key failed")
        return dgb, key_id

    # -- restore after a failure ---------------------------------------------

    def restore(self):
        """After a failure: wait out trials (reads only), check the port is
        still the same hub (else write nothing), then bring every node back
        to a known-good confirmed version: the last release this run applied
        to every node (only the recorded copy, unaltered), else each node's
        version at the start of the run (dagon rollback where the other slot
        holds it). Verifies every version. Returns the restore record."""
        rec = {"time": now(), "release": self.good and str(self.good[0]), "wrote": self.wrote}
        if self.start_state is None:  # the start check failed: nothing to compare with
            result = ("FAILED: wrote without a start state" if self.wrote else
                      "unverified: start state not acquired; nothing written by this run")
            return rec | {"result": result, "status": self.last_status()}
        try:
            st = self.wait_settled({k: None for k in self.nodes}, "restore-wait")
            self.check_identity("restore")
            if self.good:
                st = self.restore_release(rec)
            else:
                st = self.restore_start(rec, st)
            rec |= {"result": "ok", "status": st}
        except Exception as e:  # noqa: BLE001 - every failure is reported
            rec |= {"result": f"FAILED: {e}", "status": self.last_status()}
        return rec

    def restore_release(self, rec):
        dgb, v, digest = self.good
        expect(sha256(dgb) == digest, f"{dgb} changed since it was applied; not applied")
        rc, _, cmd, log = self.cli(["apply", str(dgb)], "restore-apply")
        rec |= {"command": cmd, "rc": rc, "log": log.name}
        expect(rc == 0, f"restore apply rc={rc}")
        return self.wait_settled({k: v for k in self.nodes}, "restore-after")

    def restore_start(self, rec, st):
        start = {k: self.start_state[k]["version"] for k in self.nodes}
        plan = []
        for k in self.nodes:
            if st[k]["version"] == start[k]:
                continue
            if rollback_target(st[k]) != start[k]:
                raise Fail(f"{k}: on {st[k]['version']}, start version {start[k]} is not in "
                           "its other confirmed slot; cannot restore")
            plan.append(k)
        rec["rollbacks"] = plan
        for k in plan:  # every node checked before the first write
            before = self.status(f"restore-{k}-before")
            expect(rollback_target(before[k]) == start[k],
                   f"{k}: other slot no longer holds the start version {start[k]}")
            self.rollback(k, before, f"restore-rollback-{k}")
        st = self.status("restore-after")
        bad = {k: core(st[k]) for k in self.nodes
               if not settled(st[k]) or st[k]["version"] != start[k]}
        expect(not bad, f"not back on the start versions {start}: {bad}")
        return st

    def last_status(self):
        try:
            rc, text, _, _ = self.cli(["status"], "status-restore-final")
            return parse_status(text, self.coord) or text
        except Exception as e:  # noqa: BLE001
            return f"no status: {e}"

    # -- the run ------------------------------------------------------------

    def run(self, scenarios):
        print(f"logs: {self.out}")
        ok = True
        try:
            self.start_state = self.check_identity("run-start")
            for sc in scenarios:
                verdict, i = "PASS", None
                try:
                    self.check_identity(sc["id"])
                    for i, s in enumerate(sc["steps"]):
                        cmd, rc, log, st, note = self.step(sc["id"], i, s)
                        skip = s["do"] == "skip"
                        self.record(sc["id"], i, cmd, rc, log, st, "SKIPPED" if skip else "ok", note)
                        verdict = "SKIPPED" if skip else verdict
                except Exception as e:  # noqa: BLE001 - Fail or anything else stops the run
                    self.record(sc["id"], i, None, None, None, None, "UNEXPECTED",
                                f"{type(e).__name__}: {e}")
                    verdict, ok = f"FAIL: {e}", False
                self.results.append({"id": sc["id"], "title": sc["title"], "result": verdict})
                if not ok:
                    break
        except BaseException as e:  # noqa: BLE001 - Ctrl-C too ends in a restore
            ok = False
            self.results.append({"id": "-", "title": "run", "result": f"STOPPED: {e!r}"})
        return self.finish(ok)

    def finish(self, ok):
        """Restores after a failure, writes results.json; the exit code."""
        restore = None
        if not ok:
            print("STOPPED at the first unexpected state; restoring the last known-good release")
            restore = self.restore()
        final = restore["status"] if restore else self.status("final")
        (self.out / "results.json").write_text(json.dumps(
            {"port": self.port, "hub_identity": self.identity, "start_state": self.start_state,
             "results": self.results,
             "restore": restore, "final_status": final}, indent=2, ensure_ascii=False),
            encoding="utf-8")
        print("\n".join(f"{r['id']:<16} {r['result']}" for r in self.results))
        if restore:
            print(f"restore: {restore['result']}")
            if restore["result"] != "ok":  # failed or unverified: not known good
                print(f"!!! RESTORE NOT OK ({restore['result']}): nodes are not verified on a "
                      f"known-good confirmed release. Exact state:\n{json.dumps(final, indent=2)}")
                return 2
        return 0 if ok else 1


def rollback_target(n):
    """Version `dagon rollback` returns `n` to: its other slot when that holds
    a confirmed version (Confirmed or Written), else None."""
    other = n["slots"]["AB"[1 - n["active"]]]
    return other["version"] if other["state"] in ("Confirmed", "Written") else None


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def show(n):
    if not n:
        return "-"
    slots = " ".join(f"{s}:{v['state']} {v['version']}" for s, v in n["slots"].items())
    trial = "TRIAL " if n["trial"] else ""
    return f"{n['version']} {trial}active {n['active']} confirmed {n['confirmed']} {slots}"


def tamper(dgb, fix_crc):
    """Flips 4 payload bytes of the bundle's first image item; with `fix_crc`
    its item CRC-32 is recomputed, so only the digest/signature chain can
    tell (layout: dagon-format bundle.rs)."""
    data = bytearray(dgb)
    for i in range(data[6]):
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


def sync(a, desk):
    """Copies each sample of [desk.sync] into its node's package (paths to
    the repo), gives the coordinator's generated app the self-check switch
    and runs dagon gen for it."""
    project, repo = Path(a.project).resolve(), Path(a.repo).resolve()
    cfg = tomllib.loads((project / "dagon.toml").read_text(encoding="utf-8"))
    feat = desk["fail_feature"]
    for node, sample in desk["sync"].items():
        src, dst = repo / "samples" / sample, project / cfg["build"][node]["package"]
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
        set_bin(project / "dagon.toml", node, src / "Cargo.toml")
    hub = project / cfg["build"][desk["coordinator"]]["package"]
    new = f'pub fn self_check() -> bool {{\n    !cfg!(feature = "{feat}")\n}}'
    app = hub / "src" / "app.rs"
    text = app.read_text(encoding="utf-8").replace("\r\n", "\n")
    if new not in text:
        if HUB_CHECK_OLD not in text:
            sys.exit(f"{app}: self_check() is not the generated one; add the {feat} switch by hand")
        app.write_text(text.replace(HUB_CHECK_OLD, new), encoding="utf-8", newline="\n")
    cargo = hub / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    if f"\n{feat} = []" not in text:
        line = f"\n# Test build whose self-check fails (samples/desk-test).\n{feat} = []\n"
        cargo.write_text(text.replace("\n[features]\n", "\n[features]" + line, 1),
                         encoding="utf-8", newline="\n")
    rc = subprocess.call([str(Path(a.dagon).resolve()), "gen", desk["coordinator"]], cwd=project)
    print("synced; dagon gen rc =", rc)
    return rc


def set_bin(config, node, manifest):
    """`dagon build` builds the binary named like the package unless
    [build.<node>] names `bin`; a sample whose one binary has another name
    (esp32s3-app: `app`) gets that `bin` in its node's table, and any other
    `bin` there is replaced; one named like its package needs none, so a
    `bin` left there is removed. With several binaries the table is left
    as it is (the choice is the user's)."""
    m = tomllib.loads(manifest.read_text(encoding="utf-8"))
    bins = [b["name"] for b in m.get("bin", [])]
    if len(bins) > 1:
        return
    want = bins[0] if bins and bins[0] != m["package"]["name"] else None
    text = config.read_text(encoding="utf-8")
    head = f"[build.{node}]\n"
    if head not in text:
        sys.exit(f"{config}: no [build.{node}]")
    before, after = text.split(head, 1)
    table, sep, rest = after.partition("\n[")
    lines = [l for l in table.split("\n") if not re.match(r"bin\s*=", l)]
    if want:
        lines.insert(0, f'bin = "{want}"')
    new = before + head + "\n".join(lines) + sep + rest
    if new != text:
        config.write_text(new, encoding="utf-8", newline="\n")


def selftest():
    text = ("hub: version 1.4.0, mcu 0x0001, wire 1, active 1, confirmed 1, attempts 0, last reason 3, "
            "[A:Written 1.3.0, B:Confirmed 1.4.0, F:Empty 0.0.0]\n"
            "seg: version 2.0.5 (on trial), mcu 0x0002, active 0, confirmed 1, attempts 1, "
            "last reason 0, [A:Pending 2.0.5, B:Confirmed 1.4.1, F:Empty 0.0.0]\n"
            "watch: no answer (Timeout)\n")
    st = parse_status(text)
    assert st["hub"]["version"] == "1.4.0" and settled(st["hub"]) and st["hub"]["mcu"] == "0x0001"
    assert st["hub"]["slots"]["A"] == {"state": "Written", "version": "1.3.0"}
    assert st["seg"]["trial"] and not settled(st["seg"])
    assert st["watch"] is None and not settled(st["watch"])
    assert "boss" in parse_status(text, "boss") and "hub" not in parse_status(text, "boss")
    try:
        unchanged(st, {**st, "hub": {**st["hub"], "active": 0}}, ["hub"])
        raise AssertionError("change not seen")
    except Fail:
        pass
    selftest_tamper()
    selftest_plan()
    selftest_select()
    selftest_restore()
    selftest_set_bin()
    with throwaway_dir() as d:
        (d / "k").mkdir()
        (d / "k" / "dagon-signing.key").write_text("x")
    assert not d.exists()
    try:
        with throwaway_dir() as d:
            raise Fail("boom")
    except Fail:
        assert not d.exists()
    print("selftest ok")
    return 0


def selftest_set_bin():
    """[build.<node>] bin converges to the sample's manifest: added when
    absent, corrected when wrong, kept when right, removed when the binary
    is named like its package again; other tables are left alone."""
    other = '[build.seg]\npackage = "nodes/seg"\nbin = "keep"\n'
    renamed = '[package]\nname = "dagon-esp32s3-app"\n[[bin]]\nname = "app"\n'
    plain = '[package]\nname = "dagon-esp32s3-app"\n[[bin]]\nname = "dagon-esp32s3-app"\n'
    with throwaway_dir() as d:
        config, manifest = d / "dagon.toml", d / "Cargo.toml"
        for table, sample, want in [
            ('package = "nodes/watch"\n', renamed, "app"),
            ('package = "nodes/watch"\nbin = "old"\n', renamed, "app"),
            ('bin = "app"\npackage = "nodes/watch"\n', renamed, "app"),
            ('package = "nodes/watch"\nbin = "app"\n', plain, None),
            ('package = "nodes/watch"\n', plain, None),
        ]:
            config.write_text(f"{other}\n[build.watch]\n{table}", encoding="utf-8")
            manifest.write_text(sample, encoding="utf-8")
            set_bin(config, "watch", manifest)
            cfg = tomllib.loads(config.read_text(encoding="utf-8"))
            assert cfg["build"]["watch"].get("bin") == want, (table, cfg)
            assert cfg["build"]["watch"]["package"] == "nodes/watch"
            assert cfg["build"]["seg"] == {"package": "nodes/seg", "bin": "keep"}


def selftest_tamper():
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


def selftest_plan():
    rel = Path(tempfile.gettempdir()) / "p" / "out" / "deploy" / "v2.0.7.dgb"
    shown = str(rel).replace("/out/deploy/", "/out/deploy\\")
    good = (f"seg: building slot a\ndeploy 2.0.7 to 2 node(s) ({shown}):\n"
            f"  seg    2.0.4 -> 2.0.7\n  watch  (none) -> 2.0.7\n{PROMPT}")
    assert check_plan(good, "2.0.7", ["watch", "seg"], rel) is None
    assert "version" in check_plan(good, "2.0.8", ["seg", "watch"], rel)
    assert "nodes" in check_plan(good, "2.0.7", ["seg"], rel)
    assert "release" in check_plan(good, "2.0.7", ["seg", "watch"], rel.with_name("x.dgb"))
    assert check_plan(good.replace(f"\n{PROMPT}", "\n"), "2.0.7", ["seg", "watch"], rel)
    other = good.replace("watch  (none) -> 2.0.7", "watch  (none) -> 9.9.9")
    assert check_plan(other, "2.0.7", ["seg", "watch"], rel)
    extra = good.replace("2 node(s)", "3 node(s)")
    assert check_plan(extra, "2.0.7", ["seg", "watch"], rel)


def selftest_select():
    scs = [{"id": "a"}, {"id": "b"}, {"id": "c"}]
    assert select(scs, None, None) == scs
    assert [s["id"] for s in select(scs, ["c", "a"], None)] == ["a", "c"]
    assert [s["id"] for s in select(scs, None, "b")] == ["b", "c"]
    for only, start in (([], None), (["x"], None), (["a", "x"], None), (None, "x"), (["a"], "b")):
        try:
            select(scs, only, start)
            raise AssertionError(f"accepted {only} {start}")
        except ValueError:
            pass


# -- a fake dagon for the restore selftests ------------------------------------


def fake_dagon(argv):
    """`hwt.py _fake-dagon <state.json> <dagon args>`: status, rollback and
    apply over a JSON desk; every call is logged in the state's `calls`."""
    path, args = Path(argv[0]), [a for a in argv[1:]]
    st = json.loads(path.read_text(encoding="utf-8"))
    st["calls"].append(args[0])
    nodes = st["nodes"]
    if args[0] == "status":
        for k, n in nodes.items():
            slots = ", ".join(f"{s}:{n['slots'][s][0]} {n['slots'][s][1]}" for s in "AB")
            print(f"{'hub' if k == st['coordinator'] else k}: version {n['version']}, "
                  f"mcu {st['mcu']}, wire 1, active {n['active']}, confirmed {n['active']}, "
                  f"attempts 0, last reason 0, [{slots}, F:Empty 0.0.0]")
    elif args[0] == "rollback":
        n = nodes[args[args.index("--node") + 1]]
        other = 1 - n["active"]
        n["slots"]["AB"[n["active"]]][0] = "Written"
        n["slots"]["AB"[other]][0] = "Confirmed"
        n["active"], n["version"] = other, n["slots"]["AB"[other]][1]
    elif args[0] == "apply":
        rel = json.loads(Path(args[1]).read_text(encoding="utf-8"))
        v = rel["version"]
        for n in (nodes[k] for k in rel.get("nodes", nodes)):
            free = 1 - n["active"]
            n["slots"]["AB"[n["active"]]][0] = "Written"
            n["slots"]["AB"[free]] = ["Confirmed", v]
            n["active"], n["version"] = free, v
    path.write_text(json.dumps(st), encoding="utf-8")
    return 0


def selftest_restore():
    """Both restore holes against the fake CLI: back to the start versions
    by rollback when no release reached every node; nothing written when the
    hub's identity changed, the known-good copy changed, or the start
    version is out of reach."""
    desk = {"nodes": ["seg", "hub"], "coordinator": "hub", "hub_usb": "VID_C0DE",
            "status_tries": 1, "poll_s": 0, "settle_timeout_s": 0}

    def node(v, other=("Written", "1.0.0")):
        return {"version": v, "active": 0, "slots": {"A": ["Confirmed", v], "B": list(other)}}

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        state = tmp / "state.json"
        ident = {"pnp": "USB\\VID_C0DE&PID_CAFE\\1"}

        def fresh():
            state.write_text(json.dumps({"coordinator": "hub", "mcu": "0x0001", "calls": [],
                                         "nodes": {"seg": node("2.0.1"), "hub": node("2.0.1")}}))
            a = argparse.Namespace(project=tmp, dagon=None, key="k", port="COMX", out=tmp / "o",
                                   dagon_argv=[sys.executable, __file__, "_fake-dagon", str(state)],
                                   port_identity=lambda port: ident["pnp"])
            r = Run(a, desk)
            r.start_state = r.check_identity("start")
            return r

        def edit(f):
            st = json.loads(state.read_text())
            f(st)
            st["calls"] = []
            state.write_text(json.dumps(st))

        def writes():
            return [c for c in json.loads(state.read_text())["calls"] if c != "status"]

        def partial(st):  # seg took 2.0.5 before the run failed
            st["nodes"]["seg"] = {"version": "2.0.5", "active": 1,
                                  "slots": {"A": ["Written", "2.0.1"], "B": ["Confirmed", "2.0.5"]}}

        r = fresh()
        edit(partial)
        rec = r.restore()
        assert rec["result"] == "ok" and rec["rollbacks"] == ["seg"], rec
        assert writes() == ["rollback"] and rec["status"]["seg"]["version"] == "2.0.1"

        r = fresh()
        edit(lambda st: (partial(st), st["nodes"]["seg"]["slots"]["A"].__setitem__(0, "Reverted")))
        rec = r.restore()
        assert rec["result"].startswith("FAILED") and writes() == [], rec
        r.results = []
        assert r.finish(False) == 2

        r = fresh()
        good = tmp / "good.dgb"
        good.write_text(json.dumps({"version": "2.0.9"}))
        r.good = (good, "2.0.9", sha256(good))
        edit(partial)
        ident["pnp"] = "USB\\VID_C0DE&PID_CAFE\\2"
        rec = r.restore()
        assert "identity" in rec["result"] and writes() == [], rec
        ident["pnp"] = "USB\\VID_C0DE&PID_CAFE\\1"

        r = fresh()
        r.good = (good, "2.0.9", sha256(good))
        good.write_text(json.dumps({"version": "6.6.6"}))
        rec = r.restore()
        assert "changed" in rec["result"] and writes() == [], rec

        r = fresh()
        r.good = (good, "6.6.6", sha256(good))
        rec = r.restore()
        assert rec["result"] == "ok" and writes() == ["apply"], rec
        assert all(n["version"] == "6.6.6" for n in rec["status"].values())

        r = fresh()  # the start state was never acquired: unverified, exit 2
        r.start_state = None
        rec = r.restore()
        assert rec["result"].startswith("unverified") and writes() == [], rec
        r.results = []
        assert r.finish(False) == 2

        r = fresh()  # nothing written, nothing changed: a read-only check
        rec = r.restore()
        assert rec["result"] == "ok" and not rec["wrote"] and writes() == [], rec

        r = fresh()  # changed without a write by the harness: still restored
        edit(partial)
        rec = r.restore()
        assert rec["result"] == "ok" and rec["rollbacks"] == ["seg"] and not rec["wrote"], rec

        r = fresh()  # a refused-path apply that wrote seg after all
        bad = tmp / "bad.dgb"
        bad.write_text(json.dumps({"version": "2.0.66", "nodes": ["seg"]}))
        edit(lambda st: None)
        r.cli(["apply", str(bad)], "refused-but-wrote")
        assert r.wrote and r.status("x")["seg"]["version"] == "2.0.66"
        edit(lambda st: None)
        rec = r.restore()
        assert rec["result"] == "ok" and rec["rollbacks"] == ["seg"], rec
        assert rec["status"]["seg"]["version"] == "2.0.1"


def main():
    if sys.argv[1:2] == ["_fake-dagon"]:
        return fake_dagon(sys.argv[2:])
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
    r.add_argument("--port", required=True, help="the hub's serial port, pinned for every call")
    r.add_argument("--out", help="log folder (default ~/dagon-hwt)")
    r.add_argument("--only", nargs="*", help="scenario ids to run")
    r.add_argument("--from", dest="start", help="start at this scenario id")
    for p in (s, r):
        p.add_argument("--desk", default=str(HERE / "hwt.toml"), help="desk data and scenarios")
    sub.add_parser("selftest")
    a = ap.parse_args()
    if a.cmd == "selftest":
        return selftest()
    data = tomllib.loads(Path(a.desk).read_text(encoding="utf-8"))
    if a.cmd == "sync":
        return sync(a, data["desk"])
    try:
        scs = select(data["scenario"], a.only, a.start)
    except ValueError as e:
        ap.error(str(e))
    return Run(a, data["desk"]).run(scs)


if __name__ == "__main__":
    sys.exit(main())
