#!/usr/bin/env python3
"""The consent lists of the page's development stand-in are the CLI's.

`web/src/app/agentMock.ts` answers the page in place of `dagon serve` and
shows what a linked DAGON sends and never sends. Those words are
`crates/dagon-cli/src/consent.rs`'s `SENDS` and `NEVER_SENDS`, which the
real serve returns; a stand-in with other words would let the page be
checked against a consent text no user is asked."""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST = ROOT / "crates" / "dagon-cli" / "src" / "consent.rs"
PAGE = ROOT / "web" / "src" / "app" / "agentMock.ts"


def rust_list(text, name):
    """The string literals of `pub const <name>: [&str; N] = [...]`."""
    m = re.search(rf"pub const {name}: \[&str; (\d+)\] = \[(.*?)\];", text, re.S)
    if not m:
        raise ValueError(f"no {name} in consent.rs")
    items = re.findall(r'"((?:[^"\\]|\\.)*)"', m[2])
    if len(items) != int(m[1]):
        raise ValueError(f"{name}: {len(items)} literals for [&str; {m[1]}]")
    return items


def page_list(text, name):
    """The string literals of `const <name> = [...]`."""
    m = re.search(rf"^const {name} = \[(.*?)\];", text, re.S | re.M)
    if not m:
        raise ValueError(f"no {name} in agentMock.ts")
    return re.findall(r"'((?:[^'\\]|\\.)*)'", m[1])


class ConsentCopy(unittest.TestCase):
    def test_the_stand_in_shows_the_lists_the_cli_shows(self):
        rust, page = RUST.read_text(encoding="utf-8"), PAGE.read_text(encoding="utf-8")
        self.assertEqual(page_list(page, "SENDS"), rust_list(rust, "SENDS"))
        self.assertEqual(page_list(page, "NEVER"), rust_list(rust, "NEVER_SENDS"))

    def test_a_changed_word_is_seen(self):
        rust = 'pub const SENDS: [&str; 2] = [\n    "a（b）",\n    "c",\n];\n'
        self.assertEqual(rust_list(rust, "SENDS"), ["a（b）", "c"])
        self.assertEqual(page_list("const SENDS = ['a（b）', 'c'];\n", "SENDS"), ["a（b）", "c"])
        self.assertNotEqual(page_list("const SENDS = ['a', 'c'];\n", "SENDS"), rust_list(rust, "SENDS"))
        with self.assertRaises(ValueError):
            rust_list('pub const SENDS: [&str; 3] = ["a", "c"];', "SENDS")
        with self.assertRaises(ValueError):
            page_list("const OTHER = [];", "SENDS")


if __name__ == "__main__":
    unittest.main()
