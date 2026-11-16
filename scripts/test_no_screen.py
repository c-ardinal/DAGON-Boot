"""DAGON draws no screen: no word of a screen implementation, a display
driver, its glyphs and fonts, a display task, the relay's view of the linked
nodes, its HELLO polling or its reader of a stored topology appears under
crates/ or targets/. An app that wants a display draws it from the data
DAGON hands it (samples/ shows one).

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCANNED = ("crates", "targets")
SKIP_DIRS = {"target", "node_modules", ".git"}
# The built Console page: minified, and drawn by the browser.
SKIP_FILES = {"crates/dagon-cli/page"}
WORDS = re.compile(
    r"\b(?:Shade|HOME_ROWS|DISPLAY_TASK|gc9a01|GC9A01|GLYPHS|gen_glyphs|NotoSansJP"
    r"|set_seen|set_seen_at|expire_progress|note_show|hello_poll_ms|new_state"
    r"|from_topology|LinkedNode|LinkedNodes)\b"
)


def found(texts):
    """(path, line number, word) of every screen word in `texts` (path -> text)."""
    out = []
    for path, text in sorted(texts.items()):
        for n, line in enumerate(text.splitlines(), 1):
            out.extend((path, n, m.group(0)) for m in WORDS.finditer(line))
    return out


def tree():
    texts = {}
    for top in SCANNED:
        for p in (ROOT / top).rglob("*"):
            rel = p.relative_to(ROOT)
            if not p.is_file() or SKIP_DIRS.intersection(rel.parts):
                continue
            if any(rel.as_posix().startswith(s + "/") for s in SKIP_FILES):
                continue
            try:
                texts[rel.as_posix()] = p.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
    return texts


class NoScreen(unittest.TestCase):
    def test_no_screen_word_under_crates_or_targets(self):
        self.assertEqual(found(tree()), [])

    def test_a_planted_word_is_found(self):
        for word in ("Shade::Trial", "pub mod gc9a01;", "DISPLAY_TASK + 1", "state.set_seen(i, s)"):
            with self.subTest(word=word):
                self.assertTrue(found({"crates/x/src/lib.rs": word}))

    def test_a_longer_word_is_not_found(self):
        self.assertEqual(found({"crates/x/src/lib.rs": "Shaded MY_DISPLAY_TASKS"}), [])


if __name__ == "__main__":
    unittest.main()
