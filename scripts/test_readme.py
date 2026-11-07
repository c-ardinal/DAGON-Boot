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
from ja_spacing import lines_with_spaces  # noqa: E402
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

    def test_the_japanese_readme_has_no_space_beside_latin_digits_or_code(self):
        self.assertEqual(lines_with_spaces(JA.read_text(encoding="utf-8")), [])

    def test_console_goes_by_its_full_name(self):
        for path in (JA, EN):
            self.assertEqual(re.findall(r".{0,20}(?<!DAGON )Console.{0,20}", path.read_text(encoding="utf-8")), [], path.name)

    def test_requirements_come_before_the_first_command_that_needs_them(self):
        for path, browser, rust in ((JA, "ChromeかEdge 89以降", "- Rust 1.95.0以降"), (EN, "Chrome or Edge 89 or later", "- Rust 1.95.0 or later")):
            text = path.read_text(encoding="utf-8")
            self.assertLess(text.index(browser), text.index("](https://c-ardinal.github.io/DAGON-Boot/demo/)"), path.name)
            self.assertLess(text.index(rust), text.index("cargo install"), path.name)

    def test_building_it_yourself_stays_out_of_trying_it(self):
        for path, start, end, heading, next_heading in (
            (JA, "## 試してみる", "## 自分のプロジェクトを始める", "### DAGONを自分でビルドする", "## 依存しているものと外部のデータ"),
            (EN, "## Try it", "## Start your own project", "### Building DAGON yourself", "## Dependencies and external data"),
        ):
            text = path.read_text(encoding="utf-8")
            section = text[text.index(start):text.index(end)]
            for word in ("Rust", "コンパイラ", "compiler", "cargo", "ST-LINK", "rustup"):
                self.assertNotIn(word, section, path.name)
            self.assertLess(text.index(heading), text.index(next_heading), path.name)
            self.assertEqual(text.count(next_heading), 1, path.name)
            self.assertLess(text[text.index(heading):].index(next_heading), text[text.index(heading):].index("\n## ", 3) + 2, path.name)

    def test_doctor_runs_once_the_setup_is_drawn(self):
        for path in (JA, EN):
            self.assertNotIn("cd my-fleet\ndagon doctor", path.read_text(encoding="utf-8"), path.name)

    def test_the_figure_sits_under_a_heading_that_says_what_it_shows(self):
        self.assertIn("### ソフトウェアアーキテクチャ\n\n<img src=\"assets/architecture/node-ja.svg\"", JA.read_text(encoding="utf-8"))
        self.assertIn("### Software architecture\n\n<img src=\"assets/architecture/node-en.svg\"", EN.read_text(encoding="utf-8"))

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

    def test_no_dagon_release_number(self):
        # Which version to install is the user's choice.
        for path in (JA, EN):
            self.assertEqual(re.findall(r"cli-v\d[\w.]*|--tag (?!cli-v<version>)\S+|dagon-cli[ -]\d[\w.]*", path.read_text(encoding="utf-8")), [], path.name)

    def test_the_stated_versions_are_the_ones_the_workflows_build_with(self):
        rust = tomllib.loads((ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        compilers = set(re.findall(r'cc: "([^"]+)"', release))
        self.assertTrue(compilers)
        self.assertEqual(len(compilers), len(set(re.findall(r"\{ os: ([a-z]+)", release))))
        docs = ROOT / "site/src/content/docs"
        for path, word in ((JA, f"Rust {rust}以降"), (EN, f"Rust {rust} or later"), (docs / "getting-started/install.md", f"Rust {rust}以降"), (docs / "en/getting-started/install.md", f"Rust {rust} or later")):
            text = path.read_text(encoding="utf-8")
            self.assertIn(word, text, path.name)
            for cc in compilers:
                self.assertIn(cc, text, path.name)
        for name in ("ci.yml", "release.yml", "pages.yml"):
            self.assertNotIn("-latest", (ROOT / ".github/workflows" / name).read_text(encoding="utf-8"), name)
        ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        for image in re.findall(r"os: \[([^\]]+)\]", ci)[0].split(", "):
            self.assertIn(f"os: {image},", release, image)

    def test_files_they_point_at_exist(self):
        for path in (JA, EN):
            text = path.read_text(encoding="utf-8")
            for target in re.findall(r'(?:\]\(|(?:src|srcset|href)=")([^)"]+)', text):
                if not target.startswith("http"):
                    self.assertTrue((ROOT / target).exists(), f"{path.name}: {target}")


if __name__ == "__main__":
    unittest.main()
