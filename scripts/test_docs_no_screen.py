"""DAGON's docs and Console text do not say DAGON draws a screen, writes a
topology copy to a node or has `topo push`. What an app receives (progress,
phase, trial, events, wiring) is documented so the app draws its own display;
only samples/rp2040-round-lcd describes a screen.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Hand-written and generated pages, READMEs, the Console's text, CLI help.
SCANNED = (
    "site/src/content",
    "site/i18n",
    "site/scripts",
    "web/src/app/copy.ts",
    "web/README.md",
    "README.md",
    "README.en.md",
    "targets",
    "crates/dagon-cli/src/cli.rs",
    "crates/dagon-cli/src/cli",
    "crates",
    "samples",
)
# Under these, only the Markdown files are pages; the code is the other units' concern.
MARKDOWN_ONLY = {"crates", "targets"}
SAMPLE_SUFFIXES = {".md", ".py"}
SKIP_DIRS = {"target", "node_modules", ".git"}
SCREEN_SAMPLE_README = "samples/rp2040-round-lcd/README.md"
PATTERNS = (
    r"topo(?:logy)? push",
    r"push_topology",
    r"\"topo\", \"push\"",
    r"send (?:a|the|new) topology",
    r"sends the topology",
    r"構成を[^。、\s]{0,10}ノードへ送",
    r"構成の送信",
    r"topology (?:display )?copy",
    r"topology transfer",
    r"トポロジの転送",
    r"構成の転送",
    r"display copy",
    r"Topology Copy",
    r"host\.topology\.pushed",
    r"relay\.children_set",
    r"relay\.show(?:_expired)?",
    r"ノードへ構成を送",
    r"構成の(?:表示用の)?コピー",
    r"表示用のコピー",
    r"progress screen",
    r"進捗の画面",
    r"rp2040-hub",
    r"hub-boot-screen",
    r"TOPOLOGY (?:region|slot)",
    r"TOPOLOGYの領域",
    r"target TOPOLOGY",
    r"ターゲットTOPOLOGY",
)
WORDS = re.compile("|".join(PATTERNS), re.IGNORECASE)


def found(texts):
    """(path, line number, text) of every screen or topology-copy word."""
    out = []
    for path, text in sorted(texts.items()):
        for n, line in enumerate(text.splitlines(), 1):
            out.extend((path, n, m.group(0)) for m in WORDS.finditer(line))
    return out


def tree():
    texts = {}
    for top in SCANNED:
        base = ROOT / top
        files = [base] if base.is_file() else base.rglob("*") if base.is_dir() else []
        for p in files:
            rel = p.relative_to(ROOT)
            if not p.is_file() or SKIP_DIRS.intersection(rel.parts):
                continue
            if top in MARKDOWN_ONLY and p.suffix != ".md":
                continue
            if top == "samples" and p.suffix not in SAMPLE_SUFFIXES:
                continue
            if rel.as_posix() == SCREEN_SAMPLE_README:
                continue
            try:
                texts[rel.as_posix()] = p.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
    return texts


class DocsNoScreen(unittest.TestCase):
    def test_docs_and_console_text_have_no_screen_or_topology_copy_word(self):
        self.assertEqual(found(tree()), [])

    def test_the_sample_readme_is_where_the_screen_is_described(self):
        self.assertTrue((ROOT / SCREEN_SAMPLE_README).is_file())

    def test_planted_words_are_found(self):
        for word in (
            "dagon topo push",
            "the hub's topology copy",
            "ノードへ構成を送信",
            "`relay.show`",
            "The TOPOLOGY region is two banks",
            "`written` with target TOPOLOGY",
            "TOPOLOGYの領域は",
        ):
            with self.subTest(word=word):
                self.assertTrue(found({"site/x.md": word}))

    def test_plain_topology_words_are_not_found(self):
        self.assertEqual(
            found({"site/x.md": "dagon topo check; the topology-bin body; target 3 is reserved"}), []
        )


if __name__ == "__main__":
    unittest.main()
