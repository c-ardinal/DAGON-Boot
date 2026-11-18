"""The repository states only what a developer using DAGON needs: no
planning, scheduling or review-process words, no project status, and no
plan references a reader outside the project cannot follow.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The one word list, applied to file contents and file names alike. Each
# entry: what it catches, and the pattern.
MARKERS = [
    ("shortcut marker", r"(?i)ponytail"),
    ("event name", r"(?i)hackathon|ハッカソン|built with claude"),
    # "desk" as its own word or a name part (desk_x, deskX, SimDesk), not inside helpdesk or desktop.
    ("desk wording", r"(?i:(?<![a-z])desk(?!top))|(?<=[a-z])Desk|机|デスク"),
    # PM as the role word, not a clock time (3 PM, 11:30 PM).
    ("project role", r"オーナー|(?<![\d:])(?<!\d )\bPM\b"),
    (
        "owner as a person",
        r"(?i)\(owner \d|\bowner[- ](?:approved|decision|instruction|trials)"
        r"|\bowner's (?:OK|source)|\bonly the owner\b"
        r"|\b(?:the|an?) owner (?:asked|said|wants?|wanted|decided|approved|instructed|takes|declares|has not)\b",
    ),
    # Project status in durable text. "pending", "in progress" and "not ... yet" stay
    # allowed: they name states of the product (a pending slot, a transfer in
    # progress, a link kind not available yet), not the state of the project.
    (
        "project status",
        r"(?i)\btoday(?:'s)?\b|\bfor now\b|\bstill in progress\b|\bstill pending for\b"
        r"|\bopen (?:\w+ )?decision\b|進行中|当面|今のところ|いまのところ|現在できること"
        # Work under way on the project: "still being validated", まだ確認中 (a bare 確認中, or "being checked", is a product state).
        r"|\bstill being\b|\bbeing validated\b|まだ(?:確認|検証)中",
    ),
    ("plan screen id", r"\bSC-[A-Z0-9]"),
    ("plan section", r"§"),
    ("requirement number", r"\b(?:spec \d+|C\d{2}):"),
    ("plan state id", r"\bL\d{2}\b"),
    ("plan document", r"\bui-design\b|\bdev-flow\b"),
    ("tracker reference", r"\bC-\d{3}\b"),
    ("review round", r"(?i)\bround[ -]?\d+(?![\d×x])"),
    ("process trailer", r"\b(?:Lane|Refs):"),
    ("process note", r"Superseded[:( ]|(?i:same-class sweep)|案[:,]|\(案|案 \("),
    ("month/day date", r"(?<![\d/.])1[0-2]/[1-3]?\d(?![\d/.])"),
    # A calendar date in prose; a timestamp (date, then T or space and a time) is data.
    ("calendar date", r"\b20\d\d-[01]\d(?:-[0-3]\d)?(?![\d-])(?![T ]\d\d:)"),
]
COMPILED = [(what, re.compile(p)) for what, p in MARKERS]

# The one notice naming the event, word for word, in the READMEs only (the site carries no dated notice);
# no pattern is loosened for it. Its two sentences sit on two lines.
NOTICE_JA_LINES = (
    '2026/11/8現在、"Built with Claude: Japan 2026"向けに3種類のマイコンだけがサポートされているα版の状態です。',
    "正式リリースまでに破壊的変更が行われるリスクが有ることをご理解下さい。",
)
NOTICE_EN_LINES = (
    'As of November 8, 2026, this is an alpha that supports only three kinds of microcontroller, made for "Built with Claude: Japan 2026".',
    "Please understand that breaking changes may be made before the official release.",
)
NOTICE_JA = "".join(NOTICE_JA_LINES)
NOTICE_EN = " ".join(NOTICE_EN_LINES)
NOTICE_FILES = {
    "README.md": NOTICE_JA_LINES,
    "README.en.md": NOTICE_EN_LINES,
}

# Values that look like a marker but are fixed by someone else.
ALLOWED = [
    "2023-06-01",  # the Anthropic API's `anthropic-version` header value
]

# Tracked files that are not written here: vendored, generated or built.
SKIP = [
    "Cargo.lock",
    "web/package-lock.json",
    "targets/mcu/esp32s3/vendor/",
    "scripts/test_markers.py",
]


def files():
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        check=True,
        capture_output=True,
    ).stdout
    for name in sorted(set(out.decode("utf-8").split("\0"))):
        if name and not any(name == s or (s.endswith("/") and name.startswith(s)) for s in SKIP):
            yield name


def line_hits(line):
    for a in ALLOWED:
        line = line.replace(a, "")
    for what, rx in COMPILED:
        m = rx.search(line)
        if m:
            yield what, m.group(0)


def hits():
    for name in files():
        for what, word in line_hits(name):
            yield f"{name}: file name: {what} '{word}'"
        path = ROOT / name
        if not path.is_file():
            continue
        data = path.read_bytes()
        if b"\0" in data:
            continue
        text = data.decode("utf-8", errors="replace")
        for no, line in enumerate(text.splitlines(), 1):
            for text in NOTICE_FILES.get(name, ()):
                line = line.replace(text, "")
            for what, word in line_hits(line):
                yield f"{name}:{no}: {what} '{word}'"


# `dagon serve` has no simulator: the browser simulator runs in the page itself,
# and `dagon sim` is the CLI's. These names belonged to a serve-side simulator;
# file names and contents are both checked.
SERVE_SIM_NAMES = re.compile(
    r"server\.sim|/v1/sim|demo_uplink|serveSim|simHub|simWrite|simRead|simUplink"
    r"|demoLink|DEMO_REFUSED|DEMO_HIDDEN|demo_refuses"
)
SERVE_SIM_DIRS = ["crates/", "web/src/", "site/src/"]
# The one exception: the browser simulator's module is named simHub.ts. Only these
# files may carry that name, and in their contents only as the module path `simHub.ts`.
SIMHUB_MODULE = {
    "web/src/serial/simHub.ts": "the browser simulator (the demo page, development stand-in hubs)",
    "web/src/serial/simHub.test.ts": "its own tests",
    "web/src/serial/devHub.ts": "the stand-in hubs run on the browser simulator",
    "web/src/app/recover.test.ts": "recovery is tested against the browser simulator",
}


def serve_sim_text_hits(name, text):
    """The serve simulator names in `text`, a line of file `name` or the name itself."""
    for m in SERVE_SIM_NAMES.finditer(text):
        is_module = m.group(0) == "simHub" and name in SIMHUB_MODULE
        if is_module and (text == name or text.startswith(".ts", m.end())):
            continue
        yield m.group(0)


def serve_sim_file_hits(name, text):
    for word in serve_sim_text_hits(name, name):
        yield f"{name}: file name: '{word}'"
    for no, line in enumerate(text.splitlines(), 1):
        for word in serve_sim_text_hits(name, line):
            yield f"{name}:{no}: '{word}'"


def serve_sim_hits():
    out = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", *SERVE_SIM_DIRS],
        check=True,
        capture_output=True,
    ).stdout
    for name in sorted(set(out.decode("utf-8").split("\0"))):
        path = ROOT / name
        if name and path.is_file():
            yield from serve_sim_file_hits(name, path.read_text("utf-8", errors="replace"))


class Markers(unittest.TestCase):
    def test_serve_has_no_simulator(self):
        found = list(serve_sim_hits())
        self.assertEqual(found, [], f"{len(found)} serve simulator name(s):\n" + "\n".join(found))

    def test_the_serve_simulator_check_allows_only_the_browser_simulator_module(self):
        refused = [
            ("web/src/serial/serveSimHub.ts", ""),
            ("web/src/serial/simHubServe.ts", ""),
            ("crates/dagon-cli/src/serve/api.rs", "// the page's simHub"),
            ("crates/dagon-cli/src/serve/api.rs", "include_str!(\"web/src/serial/simHub.ts\")"),
            ("web/src/App.svelte", "import { simulator } from './serial/simHub.ts';"),
            ("web/src/serial/devHub.ts", "function simHub() {}"),
            ("web/src/serial/simHub.ts", "export const simHub = 1;"),
        ]
        for name, text in refused:
            self.assertTrue(list(serve_sim_file_hits(name, text)), (name, text))
        allowed = [
            ("web/src/serial/simHub.ts", "export function simulator() {}"),
            ("web/src/serial/simHub.test.ts", "import { simulator } from './simHub.ts';"),
            ("web/src/serial/devHub.ts", "import { simulator } from './simHub.ts';"),
            ("web/src/app/recover.test.ts", "import { simulator } from '../serial/simHub.ts';"),
        ]
        for name, text in allowed:
            self.assertEqual(list(serve_sim_file_hits(name, text)), [], (name, text))

    def test_no_internal_process_markers(self):
        found = list(hits())
        self.assertEqual(found, [], f"{len(found)} marker(s):\n" + "\n".join(found))

    def test_the_notice_is_refused_anywhere_but_where_it_belongs(self):
        self.assertTrue(list(line_hits(NOTICE_JA)))
        self.assertTrue(list(line_hits(NOTICE_EN)))
        self.assertTrue(list(line_hits(NOTICE_JA + "。")))

    def test_the_patterns_catch_what_they_are_for(self):
        for s in ["// ponytail: x", "(owner 11/3)", "SC-S0", "§7.6", "L21", "11/5",
                  "checked 2026-11-03", "Lane: x", "C-389", "desk", "desk_demo", "SimDesk", "デスクのデモ",
                  "and the owner asked for this", "the PM decided", "## What works today",
                  "checks are still in progress (see", "skips it for now", "an open design decision",
                  "## 現在できること", "docs/SC-S0.md", "display are still being validated",
                  "the flow is being validated", "通信と表示はまだ確認中で"]:
            self.assertTrue(list(line_hits(s)), s)
        for s in ["the round LCD", "round trip", "Math.round(x)", "one owner per peripheral",
                  "the owner process of the next test", "at: 2026-11-04T04:14:35Z", "package_dir(owner)",
                  "the round 466×466 screen", "変更の案です", "PMP", "L0", "desktop", "helpdesk", "3 PM",
                  "11:30 PM", "7/12 PASS", "anthropic-version: 2023-06-01", "the pending slot",
                  "a transfer in progress", "not available yet on rp2040", "a clear request still pending clear",
                  "The received body is being checked.", "… 確認中"]:
            self.assertFalse(list(line_hits(s)), s)


if __name__ == "__main__":
    unittest.main()
