"""The READMEs: Japanese (README.md) and English (README.en.md) in step, and true to the repository.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import sys
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
from test_markers import NOTICE_EN, NOTICE_JA, NOTICE_FILES  # noqa: E402

JA = ROOT / "README.md"
EN = ROOT / "README.en.md"


def structure(path):
    """The README's shape, line by line: headings by level, tables, lists, code, admonitions, pictures."""
    out = []
    code = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("```"):
            out.append("fence")
            code = not code
        elif code:
            out.append("code")
        elif m := re.match(r"(#+) ", line):
            out.append(f"h{len(m.group(1))}")
        elif m := re.match(r"> \[!(\w+)\]", line):
            out.append(m.group(1))
        elif line.startswith("|"):
            out.append("row")
        elif re.match(r"- ", line):
            out.append("bullet")
        elif re.match(r"\d+\. ", line):
            out.append("step")
        elif line.startswith("!["):
            out.append("picture")
    return out


def definitions(kind, file):
    return [tomllib.loads(p.read_text(encoding="utf-8")) for p in sorted((ROOT / "targets" / kind).glob(f"*/{file}"))]


class Readmes(unittest.TestCase):
    def test_both_languages_have_the_same_structure(self):
        self.assertEqual(structure(JA), structure(EN))

    def test_the_notice_is_word_for_word_where_it_belongs(self):
        for rel, lines in NOTICE_FILES.items():
            # Next to each other, in order: the notice's lines are one notice in a block quote.
            self.assertIn("\n> ".join(lines), (ROOT / rel).read_text(encoding="utf-8"), rel)

    def test_the_japanese_readme_says_microcontroller(self):
        self.assertNotIn("ノード", JA.read_text(encoding="utf-8"))

    def test_console_goes_by_its_full_name(self):
        for path in (JA, EN):
            self.assertEqual(re.findall(r".{0,20}(?<!DAGON )Console.{0,20}", path.read_text(encoding="utf-8")), [], path.name)

    def test_requirements_come_before_the_first_command_that_needs_them(self):
        for path, heading in ((JA, "## 必要なもの"), (EN, "## Prerequisites")):
            text = path.read_text(encoding="utf-8")
            self.assertLess(text.index(heading), text.index("cargo install"), path.name)

    def test_requirements_say_what_running_dagon_needs(self):
        for path, heading, next_heading, line in ((JA, "## 必要なもの", "## まず試す", "- Chrome か Edge 89 以降\n"), (EN, "## Prerequisites", "## Try it", "- Chrome or Edge 89 or later\n")):
            text = path.read_text(encoding="utf-8")
            section = text[text.index(heading):text.index(next_heading)]
            self.assertIn(line, section, path.name)
            self.assertNotIn("dagon doctor", section, path.name)

    def test_the_figure_says_word_for_word_what_it_shows(self):
        self.assertIn("次の図は DAGON のソフトウェアの設計です。", JA.read_text(encoding="utf-8"))
        self.assertIn("The figure below is DAGON's software design.", EN.read_text(encoding="utf-8"))

    def test_the_notice_says_how_many_microcontrollers_there_are(self):
        mcus = definitions("mcu", "dagon-mcu.toml")
        self.assertEqual(len(mcus), 3, "the notice says 3種類のマイコン / three kinds")
        self.assertIn("3種類のマイコン", NOTICE_JA)
        self.assertIn("three kinds of microcontroller", NOTICE_EN)

    def test_every_mcu_and_board_is_named(self):
        names = [m["name"] for m in definitions("mcu", "dagon-mcu.toml")]
        names += [b["product"] for b in definitions("boards", "dagon-board.toml")]
        for path in (JA, EN):
            text = path.read_text(encoding="utf-8")
            for name in names:
                self.assertIn(f"{name} ", text.replace("|", " "), f"{path.name}: {name}")

    def test_the_install_tag_is_the_cli_version(self):
        compat = tomllib.loads((ROOT / "compat.toml").read_text(encoding="utf-8"))
        version = next(c["version"] for c in compat["component"] if c["name"] == "cli")
        for path in (JA, EN):
            self.assertIn(f"--tag cli-v{version} dagon-cli", path.read_text(encoding="utf-8"), path.name)

    def test_files_they_point_at_exist(self):
        for path in (JA, EN):
            text = path.read_text(encoding="utf-8")
            for target in re.findall(r'(?:\]\(|(?:src|srcset|href)=")([^)"]+)', text):
                if not target.startswith("http"):
                    self.assertTrue((ROOT / target).exists(), f"{path.name}: {target}")


if __name__ == "__main__":
    unittest.main()
