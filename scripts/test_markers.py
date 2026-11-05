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
    ("event name", r"(?i)hackathon|ハッカソン"),
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

# Values that look like a marker but are fixed by someone else.
ALLOWED = [
    "2023-06-01",  # the Anthropic API's `anthropic-version` header value
]

# Tracked files that are not written here: vendored, generated or built.
SKIP = [
    "Cargo.lock",
    "web/package-lock.json",
    "crates/dagon-cli/page/",
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
            for what, word in line_hits(line):
                yield f"{name}:{no}: {what} '{word}'"


class Markers(unittest.TestCase):
    def test_no_internal_process_markers(self):
        found = list(hits())
        self.assertEqual(found, [], f"{len(found)} marker(s):\n" + "\n".join(found))

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
