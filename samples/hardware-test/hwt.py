#!/usr/bin/env python3
"""Sample hardware test: drives the setup (hub on USB, seg on I2C,
watch on UART) through the dagon CLI, scenario by scenario (hwt.toml), and
records every step: time, command, exit code and the status of every node.
Stops at the first unexpected state, then brings every node back to the
last known-good release. See README.md.

    python hwt.py sync --project <project> --repo <DAGON-Boot clone> --dagon <exe>
    python hwt.py run  --project <project> --dagon <exe> --key <secret key file> --port <COMn>
                       [--out <dir>] [--only <id>...] [--from <id>] [--erase-probe <sel>]
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
MUTATING = {"deploy", "apply", "rollback", "flash", "provision", "rescue"}

LINE = re.compile(
    r"^(?P<id>[\w-]+): version (?P<version>\S+)(?P<trial> \(on trial\))?, mcu (?P<mcu>\S+), "
    r"(?:wire (?P<wire>\d+), )?active (?P<active>\d+), confirmed (?P<confirmed>\d+), attempts (?P<attempts>\d+), "
    r"last reason (?P<reason>\d+), \[(?P<slots>.*)\]$"
)
SLOT = re.compile(r"([ABF]):(\w+) ([^,\s]+)")
# `dagon status`' versions line of a node; the boot core part is
# `A:Confirmed 1.0.0 (running), B:Empty`, or `none (...)` / `unknown`.
VERSIONS = re.compile(r"^(?P<id>[\w-]+): versions: stage0 [^;]+; boot core (?P<core>[^;]+); ")
# `dagon deploy`'s plan before its prompt (deploy-plan.txt is one, as the CLI prints it):
#   deploy to <n> node(s) (<package file>):
#     <node>  <version now, or (none)> -> <new version>
PLAN_HEAD = re.compile(r"^deploy to (\d+) node\(s\) \((.+)\):$")
PLAN_NODE = re.compile(r"^  (\S+)\s+(\S+) -> (\S+)$")
# The version line of a crate's [package] table.
PACKAGE_VERSION = re.compile(r'(?m)^(version\s*=\s*)"[^"]*"')
# `dagon deploy`'s question before writing silent nodes through their ROM loader.
ROM_ASK = re.compile(r"^No answer from (.+)\. Writing through the ROM loader rewrites all their contents\.")
# A node written through its ROM loader, and the loader session's figures
# when the CLI prints them (dagon-romlink's Stats).
ROM_DONE = "{}: rewritten through the ROM loader, {}"
ROM_STATS = re.compile(r"^(?P<id>[\w-]+): ROM loader: attempts (?P<attempts>\d+), frames (?P<frames>\d+), "
                       r"max round trip (?P<max_rtt_ms>\d+) ms, elapsed (?P<elapsed_ms>\d+) ms")
# How long a written image's erase, data and verify took, as the CLI prints
# them after the image's verify line (the erase is the round trip of the
# request the node erases before answering).
TIMES = re.compile(r"^(?P<id>[\w-]+): erase (?P<erase_ms>\d+) ms, data (?P<data_ms>\d+) ms, "
                   r"verify (?P<verify_ms>\d+) ms$")
# dagon-romlink's limits the figures are checked against.
ROMLINK = HERE.parent.parent / "crates" / "dagon-romlink" / "src" / "lib.rs"
ROM_LIMITS = ("FRAME_TIMEOUT_MS", "FRAME_GAP_MS", "ATTEMPTS")


def parse_status(text, coordinator="hub"):
    """`dagon status` output -> {node: {...}}; a node without an answer maps
    to None. The CLI names the coordinator `hub` whatever its id."""
    out = {}
    boots = {}
    for line in text.splitlines():
        v = VERSIONS.match(line.strip())
        if v:
            boots[v["id"]] = parse_core(v["core"])
            continue
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
    for k, n in out.items():
        if n:
            n["boot"] = boots.get(k)
    if "hub" in out and coordinator != "hub":
        out[coordinator] = out.pop("hub")
    return out


def parse_core(text):
    """The boot core part of a versions line -> {"running": slot, slot: {...}};
    None for a node without boot core slots, or when they are not known."""
    found = re.findall(r"([AB]):(\w+)(?: ([^\s,]+))?( \(running\))?", text)
    if not found:
        return None
    out = {s: {"state": st, "version": v or None} for s, st, v, _ in found}
    out["running"] = next((s for s, _, _, r in found if r), None)
    return out


def check_plan(text, version, nodes, release):
    """The deploy listing before PROMPT must be exactly: the nodes `nodes`,
    each `-> version`, written to `release`. Returns the problem, or None."""
    lines = text.splitlines()
    heads = [i for i, l in enumerate(lines) if PLAN_HEAD.match(l)]
    if not heads or not text.endswith(PROMPT):
        return "no deploy plan before the prompt"
    count, path = PLAN_HEAD.match(lines[heads[-1]]).groups()
    rows = [PLAN_NODE.match(l) for l in lines[heads[-1] + 1:-1]]
    if None in rows or int(count) != len(rows):
        return "plan node lines do not match its count"
    ids = [r[1] for r in rows]
    if sorted(ids) != sorted(nodes):
        return f"plan nodes {ids}, want {nodes}"
    if any(r[3] != version for r in rows):
        return f"a plan line goes to another version than {version}"
    if Path(path.replace("\\", "/")).resolve() != Path(release).resolve():
        return f"plan release {path}, want {release}"
    return None


def check_rom_question(text, nodes):
    """The ROM loader's question before PROMPT must name exactly `nodes`.
    Returns the problem, or None."""
    asks = [m for m in map(ROM_ASK.match, text.splitlines()) if m]
    if not asks or not text.endswith(PROMPT):
        return "no ROM loader question before the prompt"
    named = [n.strip() for n in asks[-1][1].split(",")]
    if sorted(named) != sorted(nodes):
        return f"ROM question names {named}, want {nodes}"
    return None


def provision_dir(project):
    """Where the first write left each node's files (`provision_dir` of
    dagon.toml, else out/provision), as the CLI reads it for a ROM write."""
    cfg = tomllib.loads((project / "dagon.toml").read_text(encoding="utf-8"))
    return (project / cfg.get("provision_dir", "out/provision")).resolve()


def erase_command(chip, probe):
    """probe-rs erasing the whole flash of `chip` through the probe `probe`
    (VID:PID[:serial]) only."""
    return ["probe-rs", "erase", "--chip", chip, "--probe", probe]


def rom_limits(src=ROMLINK):
    """dagon-romlink's limits by name (ROM_LIMITS), read from its source;
    {} without the repo next to this sample."""
    if not Path(src).is_file():
        return {}
    text = Path(src).read_text(encoding="utf-8")
    out = {}
    for name in ROM_LIMITS:
        m = re.search(rf"(?m)^pub const {name}: \w+ = ([\d_]+);", text)
        if m:
            out[name] = int(m[1].replace("_", ""))
    return out


def rom_figures(lines, node, version, limits=None):
    """One ROM write of `node` from the CLI's timed lines [(ms, line)]: how
    long from its first progress line to ROM_DONE, and the loader's figures
    (ROM_STATS) when printed, against `limits`: every frame's round trip
    under FRAME_TIMEOUT_MS, at most ATTEMPTS sessions."""
    limits = rom_limits() if limits is None else limits
    done = ROM_DONE.format(node, version)
    end = next((ms for ms, l in lines if l == done), None)
    first = next((ms for ms, l in lines if l.startswith(f"{node}: ") and l != done), None)
    stats = next(({k: int(v) for k, v in m.groupdict().items() if k != "id"}
                  for m in (ROM_STATS.match(l) for _, l in lines) if m and m["id"] == node), None)
    out = {"write_ms": None if end is None or first is None else end - first, "stats": stats}
    why = []
    if end is None:
        why.append(f"no '{done}'")
    if stats and "FRAME_TIMEOUT_MS" in limits and stats["max_rtt_ms"] >= limits["FRAME_TIMEOUT_MS"]:
        why.append(f"round trip {stats['max_rtt_ms']} ms >= FRAME_TIMEOUT_MS {limits['FRAME_TIMEOUT_MS']}")
    if stats and "ATTEMPTS" in limits and not 1 <= stats["attempts"] <= limits["ATTEMPTS"]:
        why.append(f"attempts {stats['attempts']} outside 1..{limits['ATTEMPTS']}")
    out["verdict"] = "FAIL" if why else "PASS"
    out["why"] = "; ".join(why) or None
    return out


def write_times(lines, log):
    """Every image write the CLI timed in its lines [(ms, line)]:
    [{log, node, erase_ms, data_ms, verify_ms}]."""
    return [{"log": log, "node": m["id"], **{k: int(m[k]) for k in ("erase_ms", "data_ms", "verify_ms")}}
            for m in (TIMES.match(l) for _, l in lines) if m]


def times_summary(times):
    """Per node, the number of image writes timed and the longest erase,
    data and verify among them, with the log each longest is in."""
    out = {}
    for t in times:
        n = out.setdefault(t["node"], {"writes": 0})
        n["writes"] += 1
        for k in ("erase_ms", "data_ms", "verify_ms"):
            if k not in n or t[k] > n[k]["max"]:
                n[k] = {"max": t[k], "log": t["log"]}
    return out


def rom_note(f):
    s = f["stats"]
    figures = (f"attempts {s['attempts']}, {s['frames']} frames, max round trip {s['max_rtt_ms']} ms"
               if s else "the CLI printed no loader figures")
    return f"ROM write {f['write_ms']} ms, {figures}"


def set_crate_version(manifest, version):
    """Sets the [package] version of the crate `manifest` (its Cargo.toml):
    a node's version comes from it, so each deploy step raises it there.
    An inherited version (version.workspace = true) is refused."""
    text = Path(manifest).read_text(encoding="utf-8")
    head, sep, rest = text.partition("[package]\n")
    if not sep:
        raise Fail(f"{manifest}: no [package] table")
    table, nxt, tail = rest.partition("\n[")
    if re.search(r"(?m)^version\.workspace\s*=|^version\s*=\s*\{", table):
        raise Fail(f"{manifest}: the version is inherited from the workspace; set it there")
    if PACKAGE_VERSION.search(table):
        table = PACKAGE_VERSION.sub(lambda m: f'{m[1]}"{version}"', table, count=1)
    else:
        table = f'version = "{version}"\n' + table
    new = head + sep + table + nxt + tail
    if new != text:
        Path(manifest).write_text(new, encoding="utf-8", newline="\n")


@contextlib.contextmanager
def crate_version_back(manifest, version, keep):
    """Sets the crate's version back to `version` however the block ends,
    unless `keep()` says what was written took over."""
    try:
        yield
    finally:
        if not keep():
            set_crate_version(manifest, version)


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
    def __init__(self, a, setup):
        self.setup = setup
        self.nodes = setup["nodes"]
        self.coord = setup["coordinator"]
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
        self.erase_probe = getattr(a, "erase_probe", None)  # probe-rs selector; None: no erase
        self.erased = set()  # nodes this run erased (silent until written through their ROM)
        self.lines = []  # the last CLI call's output lines, (ms from its start, line)
        self.times = []  # every image write the CLI timed (write_times)

    # -- running the CLI ----------------------------------------------------

    def cli(self, args, name, config=None, port=True, plan=None, interrupt=None, rom=None):
        """Runs dagon; output in <out>/<nn>-<name>.log. `port`: pass --port.
        `plan=(version, nodes, release)`: answer the confirmation prompt y
        only when the listing is exactly that plan, else n (and fail).
        `rom=[nodes]`: then answer the ROM loader's question y only when it
        names exactly these nodes. `interrupt=(node, pct)`: kill the CLI
        when that node's transfer reaches pct. Each output line's time from
        the start goes to self.lines as (ms, line). Returns (rc, text, cmd, log)."""
        self.n += 1
        log = self.out / f"{self.n:03d}-{name}.log"
        cmd = [*self.dagon, *args]
        if port:
            cmd += ["--port", self.port]
        if config is not False:  # key gen, release of a test key: no project config
            cmd += ["--config", str(config or self.project / "dagon.toml")]
        shown = " ".join(cmd)
        if args[0] in MUTATING:
            self.wrote = True
        t0 = now()
        p = subprocess.Popen(cmd, cwd=self.project, stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
        dec = codecs.getincrementaldecoder("utf-8")("replace")
        text, problem, killed = "", None, False
        # The prompts to answer, in order: the plan, then the ROM question.
        checks = [lambda t: check_plan(t, *plan)] if plan else []
        if rom is not None:
            checks.append(lambda t: check_rom_question(t, rom))
        asked, start, done = 0, time.monotonic(), 0
        self.lines = []
        want = interrupt and f"{interrupt[0]}: transfer {interrupt[1]}%"
        while chunk := p.stdout.read(4096):
            text += dec.decode(chunk)
            ms = round((time.monotonic() - start) * 1000)
            *whole, _ = text[done:].split("\n")
            for line in whole:
                self.lines.append((ms, line.rstrip("\r")))
                done += len(line) + 1
            if not problem and text.endswith(PROMPT):
                problem = checks[asked](text) if asked < len(checks) else "unexpected prompt"
                asked += 1
                last = problem or asked == len(checks)
                p.stdin.write(b"n\n" if problem else b"y\n")
                p.stdin.flush()
                if last:
                    p.stdin.close()
                text += f"[hwt: answered {'n: ' + problem if problem else 'y'} {now()}]\n"
                done = len(text)
            if want and any(l.startswith(want) for l in text.splitlines()):
                p.kill()
                killed = True
                text += f"\n[hwt: killed the CLI at '{want}' {now()}]\n"
                break
        if not p.stdin.closed:
            p.stdin.close()
        p.stdout.close()
        rc = p.wait()
        log.write_text(f"$ {shown}\n# start {t0}\n{text}\n# rc={rc} end {now()}\n",
                       encoding="utf-8")
        self.times += write_times(self.lines, log.name)
        if problem:
            raise Fail(f"refused the CLI's plan: {problem} (see {log.name})")
        if asked < len(checks):
            raise Fail(f"{len(checks)} prompt(s) expected, {asked} seen (rc={rc}, see {log.name})")
        if want and not killed:
            raise Fail(f"transfer never reached {interrupt[1]}% (see {log.name})")
        return rc, text, shown, log

    def status(self, label):
        """Status of every node (retried while the hub is away, e.g. rebooting)."""
        for _ in range(self.setup["status_tries"]):
            rc, text, cmd, log = self.cli(["status"], f"status-{label}")
            st = parse_status(text, self.coord)
            if rc == 0 and all(k in st for k in self.nodes):
                return st
            time.sleep(self.setup["poll_s"])
        raise Fail(f"dagon status failed {self.setup['status_tries']} times (last {log.name})")

    def check_identity(self, label):
        """The pinned port is still the same hub: PnP id (USB VID/PID, position)
        and the coordinator's MCU id as at the start."""
        pnp = self.port_identity(self.port)
        st = self.status(f"{label}-identity")
        ident = {"pnp": pnp, "mcu": st[self.coord] and st[self.coord]["mcu"]}
        if self.identity is None:
            if not pnp or self.setup["hub_usb"].upper() not in pnp.upper():
                raise Fail(f"{self.port} is not the hub ({self.setup['hub_usb']}): {pnp}")
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
        end = time.time() + self.setup["settle_timeout_s"]
        while True:
            st = self.status(label)
            if all(settled(st[k]) and v in (None, st[k]["version"]) for k, v in want.items()):
                return st
            if time.time() > end:
                raise Fail(f"not settled on {want} after {self.setup['settle_timeout_s']} s")
            time.sleep(self.setup["poll_s"])

    def crate(self, node):
        """Node `node`'s Cargo.toml ([build.<node>] package of dagon.toml)."""
        cfg = tomllib.loads((self.project / "dagon.toml").read_text(encoding="utf-8"))
        return self.project / cfg["build"][node]["package"] / "Cargo.toml"

    def fail_config(self, nodes, key="features"):
        """dagon.hwt-fail.toml: dagon.toml with the self-check-failing feature for
        `nodes`, of their app (`features`) or their boot core (`boot_features`)."""
        text = (self.project / "dagon.toml").read_text(encoding="utf-8")
        for n in nodes:
            head = f"[build.{n}]\n"
            if head not in text:
                raise Fail(f"no [build.{n}] in dagon.toml")
            text = text.replace(head, f'{head}{key} = ["{self.setup["fail_feature"]}"]\n', 1)
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
        if kind == "erase":
            return self.erase(name, s, before)
        if kind == "status":
            bad = [k for k in self.nodes if not settled(before[k])]
            if bad:
                raise Fail(f"not on a confirmed version: {bad}")
            return "-", 0, None, before, ""
        if kind == "deploy":
            return self.deploy(name, s, before)
        if kind == "core":
            return self.core_update(name, s, before)
        if kind in ("tamper", "foreign"):
            return self.refused(name, s, before)
        if kind == "rollback":
            node = s["node"]
            want = rollback_target(before[node])
            if want is None:
                raise Fail(f"{node}: other slot holds no confirmed version; nothing to roll back to")
            rc, cmd, log, st = self.rollback(node, before, name)
            return cmd, rc, log, st, f"{node} back on {want}"
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

    def erase(self, name, s, before):
        """Erases the node's whole flash through its debug probe (`[setup.erase]`
        names its probe-rs chip; `--erase-probe` the probe), so it answers
        nothing and the next deploy goes through its ROM loader. Refused unless
        the node is on a confirmed version and the first write left its boot
        core and stage0, which that ROM write needs."""
        node = s["node"]
        chip = self.setup.get("erase", {}).get(node, {}).get("chip")
        expect(chip, f"no [setup.erase.{node}] chip")
        expect(self.erase_probe, "erase without --erase-probe")
        expect(settled(before[node]), f"{node}: not on a confirmed version; not erased")
        # The ROM write needs the boot core and stage0 the first write left
        # (Host::recover: NoImage without either; every erase node has a stage0).
        for f in ("bootcore.img", "stage0.bin"):
            path = provision_dir(self.project) / node / f
            expect(path.is_file() and path.stat().st_size > 0,
                   f"{path} missing or empty: the ROM write needs it from the first write")
        cmd = erase_command(chip, self.erase_probe)
        self.wrote = True
        self.erased.add(node)
        self.n += 1
        log = self.out / f"{self.n:03d}-{name}.log"
        r = subprocess.run(cmd, cwd=self.project, capture_output=True, text=True, timeout=300)
        log.write_text(f"$ {' '.join(cmd)}\n{r.stdout}{r.stderr}\n# rc={r.returncode} end {now()}\n",
                       encoding="utf-8")
        expect(r.returncode == 0, f"erase rc={r.returncode} (see {log.name})")
        st = self.status(f"{name}-after")
        expect(st[node] is None, f"{node} still answers after the erase: {core(st[node])}")
        unchanged(before, st, [k for k in self.nodes if k != node])
        return " ".join(cmd), r.returncode, log, st, f"{node} erased, silent"

    def deploy(self, name, s, before):
        nodes = s.get("nodes", self.nodes)
        v = s["version"]
        if "rom" in s:
            return self.rom_deploy(name, s, before)
        mode = s.get("expect", "unchanged" if "interrupt_at" in s else "ok")
        cfg = self.fail_config(nodes) if mode == "rolled-back" else None
        inter = (nodes[0], s["interrupt_at"]) if "interrupt_at" in s else None
        release = self.project / "out" / "deploy" / f"v{v}.dgb"
        # The version comes from each node's crate: raise it there.
        for k in nodes:
            set_crate_version(self.crate(k), v)
        args = ["deploy", *s.get("nodes", []), "--key", self.key]
        rc, text, cmd, log = self.cli(args, name, config=cfg, plan=(v, nodes, release),
                                      interrupt=inter)
        if mode == "ok":
            expect(rc == 0, f"deploy rc={rc}")
            st = self.status(f"{name}-after")
            for k in nodes:
                check_on(st, k, v, flipped_from=before[k]["active"],
                         in_place=self.setup.get("one_slot", False))
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
        time.sleep(self.setup["poll_s"])
        st = self.wait_settled({k: before[k]["version"] for k in self.nodes}, f"{name}-after")
        for k in self.nodes:
            expect(core(st[k]) == core(before[k]), f"{k}: changed after the interrupt")
        return cmd, rc, log, st, f"killed at {inter[1]}%"

    def rom_deploy(self, name, s, before):
        """dagon deploy to nodes that answer nothing (`rom`, erased by this
        run): the plan, then the ROM question naming exactly them, both
        answered y. Each is then on `version` from slot A, confirmed; the
        others unchanged. Records how long each ROM write took and the loader
        session's figures, checked against dagon-romlink's limits."""
        nodes, v, rom = s.get("nodes", self.nodes), s["version"], s["rom"]
        for k in rom:
            expect(k in nodes and before[k] is None, f"{k}: a ROM step needs it silent and deployed")
        release = self.project / "out" / "deploy" / f"v{v}.dgb"
        for k in nodes:
            set_crate_version(self.crate(k), v)
        args = ["deploy", *s.get("nodes", []), "--key", self.key]
        rc, text, cmd, log = self.cli(args, name, plan=(v, nodes, release), rom=rom)
        expect(rc == 0, f"deploy rc={rc}")
        figures = {k: rom_figures(self.lines, k, v) for k in rom}
        (self.out / f"{log.stem}-rom.json").write_text(json.dumps(
            {"limits": rom_limits(), "nodes": figures}, indent=2), encoding="utf-8")
        st = self.status(f"{name}-after")
        for k in nodes:
            if k in rom:
                expect(settled(st[k]) and st[k]["version"] == v and st[k]["active"] == 0,
                       f"{k}: want {v} confirmed in slot A, got {core(st[k])}")
                self.erased.discard(k)
            else:
                check_on(st, k, v, flipped_from=before[k]["active"])
        unchanged(before, st, [k for k in self.nodes if k not in nodes])
        bad = {k: f["why"] for k, f in figures.items() if f["verdict"] == "FAIL"}
        expect(not bad, f"ROM loader figures out of limits: {bad}")
        return cmd, rc, log, st, "; ".join(f"{k}: {rom_note(f)}" for k, f in figures.items())

    def core_update(self, name, s, before):
        """Raises the node's boot crate to the step's version and writes the node
        with its app on the version it runs (an app whose digest is the running
        one's stays where it is). Asserted through `dagon status`' versions lines. A failing
        core's version is taken back afterwards, so later steps do not write it."""
        node, v = s["node"], s["version"]
        n = before[node]
        expect(n and n["boot"] and n["boot"]["running"],
               f"{node}: no running boot core in its versions line")
        failing = s.get("expect") == "rolled-back"
        manifest = self.project / "boot" / node / "Cargo.toml"
        if failing and f'\n{self.setup["fail_feature"]} = ' not in manifest.read_text(encoding="utf-8"):
            raise Fail(f"{manifest}: no {self.setup['fail_feature']} feature; run dagon gen")
        app = n["version"]
        release = self.project / "out" / "deploy" / f"v{app}.dgb"
        set_crate_version(self.crate(node), app)
        was = n["boot"][n["boot"]["running"]]["version"]
        took_over = []
        with crate_version_back(manifest, was, keep=lambda: bool(took_over)):
            set_crate_version(manifest, v)
            cfg = self.fail_config([node], "boot_features") if failing else None
            rc, text, cmd, log = self.cli(["deploy", node, "--key", self.key], name, config=cfg,
                                          plan=(app, [node], release))
            if failing:
                expect(rc != 0 and "BootCoreRolledBack" in text,
                       f"deploy of a failing boot core: rc={rc}, no BootCoreRolledBack")
                st = self.wait_settled({node: app}, f"{name}-after")
                check_core_back(st[node], n["boot"])
                # The write stopped at the core: the app did not move, nor did any other node.
                unchanged(before, st, self.nodes, slots=True)
                return cmd, rc, log, st, f"{node} back on boot core slot {n['boot']['running']}"
            expect(rc == 0, f"deploy rc={rc}")
            st = self.wait_settled({node: app}, f"{name}-after")
            check_core_on(st[node], v, n["boot"])
            unchanged(before, st, [k for k in self.nodes if k != node])
            took_over.append(True)
            return cmd, rc, log, st, f"{node} boot core {v} on slot {st[node]['boot']['running']}"

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
                    ["image", str(build / f"app-{slot}.elf"), "--node", node,
                     "--version", v, "--slot", slot, "--out", str(img), "--key", secret],
                    f"{name}-sign-{slot}", port=False)
                expect(rc == 0, "image with the throwaway key failed")
                imgs.append(str(img))
            dgb = self.out / "foreign.dgb"
            rc, _, _, _ = self.cli(["release", *imgs, "--name", "foreign",
                                    "--out", str(dgb), "--key", secret],
                                   f"{name}-release", port=False)
            expect(rc == 0, "release with the throwaway key failed")
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
            silent = sorted(self.erased)
            if silent:  # only a ROM write brings them back; restore writes nothing
                raise Fail(f"{silent} erased by this run and silent: write each with "
                           "`dagon deploy <node>` (ROM loader) or `dagon flash <node>` (probe)")
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
                if sc.get("needs") == "erase-probe" and not self.erase_probe:
                    self.record(sc["id"], 0, None, None, None, None, "SKIPPED", "no --erase-probe")
                    self.results.append({"id": sc["id"], "title": sc["title"],
                                         "result": "SKIPPED (no --erase-probe)"})
                    continue
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
             "results": self.results, "times": times_summary(self.times),
             "restore": restore, "final_status": final}, indent=2, ensure_ascii=False),
            encoding="utf-8")
        print("\n".join(f"{r['id']:<16} {r['result']}" for r in self.results))
        for node, t in times_summary(self.times).items():
            print(f"{node}: {t['writes']} write(s) timed, longest erase {t['erase_ms']['max']} ms, "
                  f"data {t['data_ms']['max']} ms, verify {t['verify_ms']['max']} ms")
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


def check_core_on(n, v, was):
    """`n`'s boot core: `v` confirmed and running, in the slot that was idle."""
    b = n and n["boot"]
    expect(b, "no boot core in the versions line")
    slot = b["running"]
    expect(slot and slot != was["running"], f"boot core slot did not change: {b}")
    expect(b[slot] == {"state": "Confirmed", "version": v}, f"want boot core {v} confirmed: {b}")


def check_core_back(n, was):
    """`n`'s boot core: still the one it was, confirmed; the other one is not."""
    b = n and n["boot"]
    expect(b, "no boot core in the versions line")
    slot = was["running"]
    expect(b["running"] == slot, f"boot core left slot {slot}: {b}")
    expect(b[slot] == {"state": "Confirmed", "version": was[slot]["version"]},
           f"want boot core {was[slot]['version']} confirmed: {b}")
    other = "AB"[1 - "AB".index(slot)]
    expect(b[other]["state"] != "Confirmed", f"the failing boot core is confirmed: {b}")


def check_on(st, node, v, flipped_from, in_place=False):
    """`node` on `v`, confirmed, from the other slot; `in_place`: from the
    same one (a node with one app slot, written by its boot core)."""
    n = st[node]
    expect(settled(n) and n["version"] == v, f"{node}: want {v} confirmed, got {core(n)}")
    if in_place:
        expect(n["active"] == flipped_from, f"{node}: active slot changed on a one-slot node")
    else:
        expect(n["active"] != flipped_from, f"{node}: active slot did not change")


def unchanged(before, after, nodes, slots=False):
    for k in nodes:
        a, b = (before[k], after[k]) if slots else (core(before[k]), core(after[k]))
        if slots and a and b:
            a = {**core(a), "slots": a["slots"]}
            b = {**core(b), "slots": b["slots"]}
        expect(a == b, f"{k} changed: {a} -> {b}")


# -- sync: the setup's node crates from the repo samples ----------------------

HUB_CHECK_OLD = "pub fn self_check() -> bool {\n    true\n}"


def sync(a, setup):
    """Copies each sample of [setup.sync] into its node's package (paths to
    the repo), gives the coordinator's generated app the self-check switch
    and runs dagon gen for it."""
    project, repo = Path(a.project).resolve(), Path(a.repo).resolve()
    cfg = tomllib.loads((project / "dagon.toml").read_text(encoding="utf-8"))
    feat = setup["fail_feature"]
    for node, sample in setup["sync"].items():
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
    hub = project / cfg["build"][setup["coordinator"]]["package"]
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
        line = f"\n# Test build whose self-check fails (samples/hardware-test).\n{feat} = []\n"
        cargo.write_text(text.replace("\n[features]\n", "\n[features]" + line, 1),
                         encoding="utf-8", newline="\n")
    rc = subprocess.call([str(Path(a.dagon).resolve()), "gen", setup["coordinator"]], cwd=project)
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
    selftest_times()
    selftest_rom()
    selftest_rom_run()
    selftest_one_slot()
    selftest_tamper()
    selftest_plan()
    selftest_crate_version()
    selftest_select()
    selftest_restore()
    selftest_set_bin()
    selftest_core()
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


def selftest_times():
    """The CLI's times lines, and the longest of each per node."""
    lines = [(10, "seg: erase 0% (2.0.6)"), (4000, "seg: verify ok"),
             (4001, "seg: erase 3120 ms, data 880 ms, verify 41 ms"),
             (4002, "seg: erase 3120 ms, data 880 ms"),  # not the CLI's line
             (9000, "hub: erase 96 ms, data 1500 ms, verify 30 ms")]
    a = write_times(lines, "003-apply.log")
    assert a == [{"log": "003-apply.log", "node": "seg", "erase_ms": 3120, "data_ms": 880, "verify_ms": 41},
                 {"log": "003-apply.log", "node": "hub", "erase_ms": 96, "data_ms": 1500, "verify_ms": 30}], a
    b = write_times([(5, "seg: erase 2990 ms, data 910 ms, verify 40 ms")], "007-apply.log")
    s = times_summary(a + b)
    assert s["seg"] == {"writes": 2, "erase_ms": {"max": 3120, "log": "003-apply.log"},
                        "data_ms": {"max": 910, "log": "007-apply.log"},
                        "verify_ms": {"max": 41, "log": "003-apply.log"}}, s
    assert s["hub"]["writes"] == 1 and times_summary([]) == {}


def selftest_core():
    """The boot core of `dagon status`' versions lines, and what the boot core
    steps conclude from it."""
    def status(node, core):
        return parse_status(
            f"{node}: version 2.0.4, mcu 0x0002, active 0, confirmed 0, attempts 0, last reason 0, "
            "[A:Confirmed 2.0.4, B:Written 2.0.3, F:Empty 0.0.0]\n"
            f"{node}: boot core 0.1.0 (slot A, digest abababababababab), [A:Confirmed 0.1.0, B:Empty 0.0.0]\n"
            f"{node}: versions: stage0 0.1.0; boot core {core}; "
            "app A:Confirmed 2.0.4 (running), B:Written 2.0.3, F:Empty; wire 1; mcu 0x0002\n")[node]

    start = status("seg", "A:Confirmed 0.1.0 (running), B:Empty")
    assert start["boot"] == {"running": "A", "A": {"state": "Confirmed", "version": "0.1.0"},
                             "B": {"state": "Empty", "version": None}}
    assert start["version"] == "2.0.4" and settled(start), start
    assert status("seg", "A:Written 0.1.0, B:Confirmed 2.1.1 (running)")["boot"]["A"]["version"] == "0.1.0"
    assert status("watch", "none (vendor 2nd stage)")["boot"] is None
    assert status("seg", "unknown")["boot"] is None
    # A hub's lines are named `hub` whatever its id; so its boot core too.
    assert parse_status("hub: versions: stage0 0.1.0; boot core A:Confirmed 0.1.0 (running), B:Empty; "
                        "app A:Confirmed 1.0.0 (running), B:Empty, F:Empty; wire 1; mcu 0x0001\n"
                        "hub: version 1.0.0, mcu 0x0001, wire 1, active 0, confirmed 0, attempts 0, "
                        "last reason 0, [A:Confirmed 1.0.0, B:Empty 0.0.0, F:Empty 0.0.0]\n",
                        "boss")["boss"]["boot"]["running"] == "A"

    a_to_b = status("seg", "A:Written 0.1.0, B:Confirmed 2.1.1 (running)")
    check_core_on(a_to_b, "2.1.1", start["boot"])
    b_to_a = status("seg", "A:Confirmed 2.1.2 (running), B:Written 2.1.1")
    check_core_on(b_to_a, "2.1.2", a_to_b["boot"])
    bad = [
        (a_to_b, "2.1.9", start["boot"]),  # another version than the step's
        (start, "0.1.0", start["boot"]),  # the slot did not change
        (status("seg", "A:Written 0.1.0, B:Pending 2.1.1 (running)"), "2.1.1", start["boot"]),  # not confirmed
        (status("seg", "unknown"), "2.1.1", start["boot"]),  # no versions line
    ]
    for args in bad:
        try:
            check_core_on(*args)
            raise AssertionError(f"accepted {args}")
        except Fail:
            pass
    back = status("seg", "A:Confirmed 2.1.2 (running), B:Reverted 2.1.3")
    check_core_back(back, b_to_a["boot"])
    for n in (status("seg", "A:Written 2.1.2, B:Confirmed 2.1.3 (running)"),  # it took over
              status("seg", "A:Pending 2.1.2 (running), B:Reverted 2.1.3"),  # not confirmed
              status("seg", "A:Confirmed 2.1.2 (running), B:Confirmed 2.1.3")):  # both
        try:
            check_core_back(n, b_to_a["boot"])
            raise AssertionError(f"accepted {n}")
        except Fail:
            pass

    with throwaway_dir() as d:
        m = d / "Cargo.toml"
        m.write_text('[package]\nname = "b"\nversion = "2.1.3"\n', encoding="utf-8")
        version = lambda: tomllib.loads(m.read_text(encoding="utf-8"))["package"]["version"]
        try:
            with crate_version_back(m, "2.1.2", keep=lambda: False):
                raise Fail("the step failed")
        except Fail:
            pass
        assert version() == "2.1.2", "restored when the step fails"
        m.write_text('[package]\nname = "b"\nversion = "2.1.3"\n', encoding="utf-8")
        with crate_version_back(m, "2.1.2", keep=lambda: True):
            pass
        assert version() == "2.1.3", "a confirmed core keeps its version"
        with crate_version_back(m, "2.1.2", keep=lambda: False):
            pass
        assert version() == "2.1.2", "a core that went back does not"

    with throwaway_dir() as d:
        (d / "dagon.toml").write_text('[build.seg]\npackage = "nodes/seg"\n', encoding="utf-8")
        r = argparse.Namespace(project=d, setup={"fail_feature": "no-confirm"})
        p = Run.fail_config(r, ["seg"], "boot_features")
        assert tomllib.loads(p.read_text(encoding="utf-8"))["build"]["seg"]["boot_features"] == ["no-confirm"]

    # The scenarios: each node's boot core over both slots, then the failing one.
    scs = {s["id"]: s for s in tomllib.loads((HERE / "hwt.toml").read_text(encoding="utf-8"))["scenario"]}
    for node in ("seg", "hub"):
        steps = scs[f"core-{node}"]["steps"]
        assert [s["do"] for s in steps] == ["core"] * 3 and all(s["node"] == node for s in steps), steps
        assert [s.get("expect", "ok") for s in steps] == ["ok", "ok", "rolled-back"], steps
        assert len({s["version"] for s in steps}) == 3


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


def selftest_rom():
    """The ROM question, the erase command, the loader figures against the
    limits read from dagon-romlink, and the setups' ROM and erase steps."""
    q = ("No answer from seg. Writing through the ROM loader rewrites all their contents. "
         "A loose wire also stops an answer: check it first.\n" + PROMPT)
    assert check_rom_question(q, ["seg"]) is None
    assert "want" in check_rom_question(q.replace("seg.", "seg, seg2."), ["seg"])
    assert check_rom_question(PROMPT, ["seg"]) is not None
    assert erase_command("STM32F401RETx", "0483:374b:X") == [
        "probe-rs", "erase", "--chip", "STM32F401RETx", "--probe", "0483:374b:X"]
    limits = rom_limits()
    assert limits == {"FRAME_TIMEOUT_MS": 500, "FRAME_GAP_MS": 500, "ATTEMPTS": 3}, limits
    lines = [(100, "seg: erase 0% (2.0.18)"), (9000, "seg: write 50% (2.0.18)"),
             (15100, "seg: ROM loader: attempts 1, frames 812, max round trip 41 ms, elapsed 14900 ms"),
             (15200, "seg: rewritten through the ROM loader, 2.0.18")]
    f = rom_figures(lines, "seg", "2.0.18", limits)
    assert f["verdict"] == "PASS" and f["write_ms"] == 15100 and f["stats"]["frames"] == 812, f
    # A session that failed before the one that wrote: the CLI names its error at the end.
    again = [(t, l.replace("attempts 1", "attempts 2") + ", last error Busy" if "ROM loader:" in l else l)
             for t, l in lines]
    f = rom_figures(again, "seg", "2.0.18", limits)
    assert f["verdict"] == "PASS" and f["stats"]["attempts"] == 2 and f["stats"]["elapsed_ms"] == 14900, f
    slow = [(t, l.replace("41 ms", "500 ms").replace("attempts 1", "attempts 4")) for t, l in lines]
    f = rom_figures(slow, "seg", "2.0.18", limits)
    assert f["verdict"] == "FAIL" and "FRAME_TIMEOUT_MS" in f["why"] and "attempts 4" in f["why"], f
    f = rom_figures([l for l in lines if "ROM loader:" not in l[1]], "seg", "2.0.18", limits)
    assert f["verdict"] == "PASS" and f["stats"] is None and "no loader figures" in rom_note(f)
    assert rom_figures(lines[:2], "seg", "2.0.18", limits)["verdict"] == "FAIL"
    data = tomllib.loads((HERE / "hwt.toml").read_text(encoding="utf-8"))
    sc = next(s for s in data["scenario"] if s["id"] == "rom-recovery")
    assert sc["needs"] == "erase-probe" and [st["do"] for st in sc["steps"]] == ["erase", "deploy"]
    erase, deploy = sc["steps"]
    assert erase["node"] in data["setup"]["erase"] and deploy["rom"] == [erase["node"]] == deploy["nodes"]


def selftest_rom_run():
    """The erase and ROM deploy steps against the fake CLI: the erased seg
    answers nothing, the deploy answers both prompts, seg is on the new
    version in slot A with its figures recorded; a restore with an erased
    seg still silent writes nothing; a ROM question naming another node is
    answered n."""
    setup = {"nodes": ["seg", "hub"], "coordinator": "hub", "hub_usb": "VID_C0DE",
             "status_tries": 1, "poll_s": 0, "settle_timeout_s": 0,
             "erase": {"seg": {"chip": "STM32F401RETx"}}}
    node = {"version": "2.0.1", "active": 0, "slots": {"A": ["Confirmed", "2.0.1"], "B": ["Written", "1.0.0"]}}
    global erase_command
    real = erase_command
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        state = tmp / "state.json"
        fake = [sys.executable, __file__, "_fake-dagon", str(state)]
        (tmp / "seg").mkdir()
        (tmp / "seg" / "Cargo.toml").write_text('[package]\nname = "seg"\nversion = "2.0.1"\n')
        (tmp / "dagon.toml").write_text('format = 1\n[build.seg]\npackage = "seg"\n')
        boot = tmp / "out" / "provision" / "seg" / "bootcore.img"
        release = tmp / "out" / "deploy" / "v2.0.18.dgb"

        def fresh(probe="0483:374b:X"):
            state.write_text(json.dumps({"coordinator": "hub", "mcu": "0x0001", "calls": [],
                                         "deploy_version": "2.0.18", "release": str(release),
                                         "nodes": {"seg": dict(node), "hub": dict(node)}}))
            a = argparse.Namespace(project=tmp, dagon=None, key="k", port="COMX", out=tmp / "o",
                                   dagon_argv=fake, erase_probe=probe,
                                   port_identity=lambda port: "USB\\VID_C0DE&PID_CAFE\\1")
            r = Run(a, setup)
            r.start_state = r.check_identity("start")
            return r

        erase_command = lambda chip, probe: [*fake, "erase", "seg"]  # noqa: E731
        try:
            r = fresh()
            try:
                r.step("rom", 0, {"do": "erase", "node": "seg"})
                raise AssertionError("erased without the boot core of the first write")
            except Fail as e:
                assert "bootcore.img" in str(e) and "erase" not in json.loads(state.read_text())["calls"]
            boot.parent.mkdir(parents=True)
            boot.write_bytes(b"core")
            try:
                r.step("rom", 0, {"do": "erase", "node": "seg"})
                raise AssertionError("erased without the stage0 of the first write")
            except Fail as e:
                assert "stage0.bin" in str(e) and "erase" not in json.loads(state.read_text())["calls"]
            (boot.parent / "stage0.bin").write_bytes(b"s0")
            cmd, rc, log, st, note = r.step("rom", 0, {"do": "erase", "node": "seg"})
            assert st["seg"] is None and r.erased == {"seg"} and r.wrote, (st, note)
            rec = r.restore()
            assert "erased by this run" in rec["result"], rec
            assert [c for c in json.loads(state.read_text())["calls"] if c != "status"] == ["erase"]
            step = {"do": "deploy", "version": "2.0.18", "nodes": ["seg"], "rom": ["seg"]}
            cmd, rc, log, st, note = r.step("rom", 1, step)
            assert rc == 0 and st["seg"]["version"] == "2.0.18" and st["seg"]["active"] == 0, st
            assert not r.erased and "attempts 1, 9 frames, max round trip 40 ms" in note, note
            figures = json.loads((r.out / f"{log.stem}-rom.json").read_text())
            assert figures["nodes"]["seg"]["verdict"] == "PASS" and figures["limits"]["ATTEMPTS"] == 3
            assert figures["nodes"]["seg"]["write_ms"] >= 0, figures

            r = fresh()
            r.step("rom", 0, {"do": "erase", "node": "seg"})
            try:
                r.step("rom", 1, {**step, "rom": ["hub"], "nodes": ["seg", "hub"]})
                raise AssertionError("a ROM step for a node that answers went ahead")
            except Fail as e:
                assert "hub: a ROM step" in str(e), e
            try:
                r.cli(["deploy", "seg", "--key", "k"], "rom-other", plan=("2.0.18", ["seg"], release),
                      rom=["hub"])
                raise AssertionError("a ROM question naming another node was answered y")
            except Fail as e:
                assert "ROM question names ['seg'], want ['hub']" in str(e), e
            assert json.loads(state.read_text())["silent"] == ["seg"]

            r = fresh(probe=None)
            r.run([{"id": "rom-recovery", "title": "t", "needs": "erase-probe",
                    "steps": [{"do": "erase", "node": "seg"}]}])
            assert r.results[0]["result"].startswith("SKIPPED"), r.results
            assert "erase" not in json.loads(state.read_text())["calls"]
        finally:
            erase_command = real


def selftest_one_slot():
    """A one-slot node is on the new version in the same slot; the setup
    file runs every node in place, with no step that needs a second slot."""
    on = parse_status("seg: version 2.0.40, mcu 0x0002, active 0, confirmed 0, attempts 0, "
                      "last reason 0, [A:Confirmed 2.0.40, B:Empty 0.0.0, F:Empty 0.0.0]\n")
    check_on(on, "seg", "2.0.40", flipped_from=0, in_place=True)
    for in_place, was in ((True, 1), (False, 0)):
        try:
            check_on(on, "seg", "2.0.40", flipped_from=was, in_place=in_place)
            raise AssertionError(f"slot check passed: in_place={in_place}, was={was}")
        except Fail:
            pass
    data = tomllib.loads((HERE / "hwt-one-slot.toml").read_text(encoding="utf-8"))
    assert data["setup"]["one_slot"] is True and "watch" not in data["setup"]["nodes"]
    steps = [st for sc in data["scenario"] for st in sc["steps"]]
    assert {st["do"] for st in steps} <= {"status", "deploy"}, steps
    assert all(st.get("expect", "ok") == "ok" and "interrupt_at" not in st for st in steps), steps


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
    """deploy-plan.txt is `dagon deploy`'s plan as the CLI printed it before
    its prompt (the simulated-hub deploy of two nodes on 2.0.7; the
    package path replaced by <release>): it must pass as is, and every
    change hwt must catch must fail."""
    rel = Path(tempfile.gettempdir()) / "p" / "out" / "deploy" / "v2.0.7.dgb"
    shown = str(rel).replace("/out/deploy/", "/out/deploy\\")
    plan = (HERE / "deploy-plan.txt").read_text(encoding="utf-8").replace("<release>", shown)
    good = f"seg: building slot a\n{plan}{PROMPT}"
    assert check_plan(good, "2.0.7", ["hub", "seg"], rel) is None, check_plan(good, "2.0.7", ["hub", "seg"], rel)
    assert "version" in check_plan(good, "2.0.8", ["seg", "hub"], rel)
    assert "nodes" in check_plan(good, "2.0.7", ["seg"], rel)
    assert "release" in check_plan(good, "2.0.7", ["seg", "hub"], rel.with_name("x.dgb"))
    assert check_plan(good.replace(f"\n{PROMPT}", "\n"), "2.0.7", ["seg", "hub"], rel)
    other = good.replace("-> 2.0.7\n", "-> 9.9.9\n", 1)
    assert "version" in check_plan(other, "2.0.7", ["seg", "hub"], rel)
    extra = good.replace("2 node(s)", "3 node(s)")
    assert check_plan(extra, "2.0.7", ["seg", "hub"], rel)
    old = good.replace("deploy to 2 node(s)", "deploy 2.0.7 to 2 node(s)")
    assert check_plan(old, "2.0.7", ["seg", "hub"], rel), "an older CLI's plan is not taken"


def selftest_crate_version():
    """Only the [package] version changes; a dependency's stays; an inherited
    one is refused."""
    with throwaway_dir() as d:
        m = d / "Cargo.toml"
        m.write_text('[package]\nname = "seg"\nversion = "0.0.0"\nedition = "2021"\n\n'
                     '[dependencies]\nx = { version = "1.2" }\ny = "3"\n', encoding="utf-8")
        set_crate_version(m, "2.0.4")
        t = tomllib.loads(m.read_text(encoding="utf-8"))
        assert t["package"]["version"] == "2.0.4" and t["dependencies"] == {"x": {"version": "1.2"}, "y": "3"}
        m.write_text('[package]\nname = "seg"\n', encoding="utf-8")
        set_crate_version(m, "2.0.5")
        assert tomllib.loads(m.read_text(encoding="utf-8"))["package"]["version"] == "2.0.5"
        m.write_text('[package]\nname = "seg"\nversion.workspace = true\n', encoding="utf-8")
        try:
            set_crate_version(m, "2.0.6")
            raise AssertionError("an inherited version was overwritten")
        except Fail:
            pass


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
    apply over a JSON setup; every call is logged in the state's `calls`."""
    path, args = Path(argv[0]), [a for a in argv[1:]]
    st = json.loads(path.read_text(encoding="utf-8"))
    st["calls"].append(args[0])
    nodes = st["nodes"]
    if args[0] == "status":
        for k, n in nodes.items():
            if k in st.get("silent", []):
                print(f"{k}: no answer (Timeout)")
                continue
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
    elif args[0] == "erase":  # stands in for probe-rs erase of node args[1]
        st.setdefault("silent", []).append(args[1])
    elif args[0] == "deploy":  # the ROM path only: plan, ROM question, ROM write of the silent nodes
        names = args[1:args.index("--key")]
        v = st["deploy_version"]
        print(f"deploy to {len(names)} node(s) ({st['release']}):")
        for k in names:
            print(f"  {k}  {nodes[k]['version']} -> {v}")
        print(PROMPT, end="", flush=True)
        if sys.stdin.readline().strip() != "y":
            return 1
        silent = [k for k in names if k in st.get("silent", [])]
        print(f"No answer from {', '.join(silent)}. Writing through the ROM loader rewrites all "
              "their contents. A loose wire also stops an answer: check it first.")
        print(PROMPT, end="", flush=True)
        if sys.stdin.readline().strip() != "y":
            return 1
        for k in silent:
            print(f"{k}: erase 0% ({v})", flush=True)
            time.sleep(0.05)
            print(f"{k}: ROM loader: attempts 1, frames 9, max round trip 40 ms, elapsed 60 ms")
            print(f"{k}: rewritten through the ROM loader, {v}")
            nodes[k] = {"version": v, "active": 0, "slots": {"A": ["Confirmed", v], "B": ["Empty", "0.0.0"]}}
            st["silent"].remove(k)
    elif args[0] == "apply":
        rel = json.loads(Path(args[1]).read_text(encoding="utf-8"))
        v = rel["version"]
        for n in (nodes[k] for k in rel.get("nodes", nodes)):
            free = 1 - n["active"]
            n["slots"]["AB"[n["active"]]][0] = "Written"
            n["slots"]["AB"[free]] = ["Confirmed", v]
            n["active"], n["version"] = free, v
        for k in rel.get("nodes", nodes):
            print(f"{k}: erase 120 ms, data 300 ms, verify 20 ms")
    path.write_text(json.dumps(st), encoding="utf-8")
    return 0


def selftest_restore():
    """Both restore holes against the fake CLI: back to the start versions
    by rollback when no release reached every node; nothing written when the
    hub's identity changed, the known-good copy changed, or the start
    version is out of reach."""
    setup = {"nodes": ["seg", "hub"], "coordinator": "hub", "hub_usb": "VID_C0DE",
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
            r = Run(a, setup)
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
        assert times_summary(r.times) == {
            k: {"writes": 1, **{f: {"max": ms, "log": r.times[0]["log"]} for f, ms in
                                (("erase_ms", 120), ("data_ms", 300), ("verify_ms", 20))}}
            for k in ("seg", "hub")}, r.times

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
    r.add_argument("--erase-probe", help="probe-rs probe (VID:PID[:serial]) that may erase the "
                   "[setup.erase] nodes; without it, scenarios that need it are SKIPPED")
    for p in (s, r):
        p.add_argument("--setup", default=str(HERE / "hwt.toml"), help="setup data and scenarios")
    sub.add_parser("selftest")
    a = ap.parse_args()
    if a.cmd == "selftest":
        return selftest()
    data = tomllib.loads(Path(a.setup).read_text(encoding="utf-8"))
    if a.cmd == "sync":
        return sync(a, data["setup"])
    try:
        scs = select(data["scenario"], a.only, a.start)
    except ValueError as e:
        ap.error(str(e))
    return Run(a, data["setup"]).run(scs)


if __name__ == "__main__":
    sys.exit(main())
