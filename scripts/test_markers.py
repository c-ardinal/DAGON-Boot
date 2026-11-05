"""The repository states only what a developer using DAGON needs: no
planning, scheduling or review-process words, and no plan references a
reader outside the project cannot follow.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The one word list. Each entry: what it catches, and the pattern.
MARKERS = [
    ("shortcut marker", r"(?i)ponytail"),
    ("event name", r"(?i)hackathon|ハッカソン"),
    ("desk wording", r"(?i)desk(?!top)|机"),
    ("project role", r"オーナー|\bPM\b"),
    (
        "owner as a person",
        r"(?i)\(owner \d|\bowner[- ](?:approved|decision|instruction|trials)"
        r"|\bowner's (?:OK|source)|\bthe owner (?:takes|declares|has not)|\bonly the owner\b",
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


def hits():
    compiled = [(what, re.compile(p)) for what, p in MARKERS]
    for name in files():
        if re.search(r"(?i)desk|ponytail", name):
            yield f"{name}: file name ({'desk' if 'desk' in name.lower() else 'ponytail'})"
        path = ROOT / name
        if not path.is_file():
            continue
        data = path.read_bytes()
        if b"\0" in data:
            continue
        text = data.decode("utf-8", errors="replace")
        for no, line in enumerate(text.splitlines(), 1):
            for a in ALLOWED:
                line = line.replace(a, "")
            for what, rx in compiled:
                m = rx.search(line)
                if m:
                    yield f"{name}:{no}: {what} '{m.group(0)}'"


class Markers(unittest.TestCase):
    def test_no_internal_process_markers(self):
        found = list(hits())
        self.assertEqual(found, [], f"{len(found)} marker(s):\n" + "\n".join(found))

    def test_the_patterns_catch_what_they_are_for(self):
        def caught(s):
            return [w for w, p in MARKERS if re.search(p, s)]

        for s in ["// po" "nytail: x", "(own" "er 11/3)", "SC" "-S0", "§" "7.6", "L" "21", "11" "/5",
                  "checked 2026" "-11-03", "Lan" "e: x", "C" "-389", "de" "sk"]:
            self.assertTrue(caught(s), s)
        for s in ["the round LCD", "round trip", "Math.round(x)", "one owner per peripheral",
                  "at: 2026-11-04T04:14:35Z", "package_dir(owner)", "the round 466×466 screen", "変更の案です", "PMP", "L0", "desktop", "7/12 PASS", "2023" "-06-01"]:
            self.assertFalse(caught(s.replace("2023-06-01", "")), s)


if __name__ == "__main__":
    unittest.main()
