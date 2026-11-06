"""Checks of the site's generators: python -m unittest discover -s site/scripts -p "test_*.py"."""
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import examples  # noqa: E402
import gen  # noqa: E402
import llms  # noqa: E402
import pages  # noqa: E402

ROOT = gen.ROOT
sys.path.insert(0, str(ROOT / "scripts"))
import architecture  # noqa: E402
from ja_spacing import lines_with_spaces, spaces  # noqa: E402
from test_markers import NOTICE_EN_LINES, NOTICE_JA_LINES  # noqa: E402

NOTICE_LINES = (*NOTICE_JA_LINES, *NOTICE_EN_LINES)


class Generated(unittest.TestCase):
    def test_committed_pages_match_their_sources(self):
        self.assertEqual(gen.main(["--check"]), 0, "run: python site/scripts/gen.py")

    def test_a_stale_page_fails_the_check(self):
        with tempfile.TemporaryDirectory() as d:
            saved = gen.OUT
            gen.OUT = pathlib.Path(d)
            # The hand-written pages with generated regions come along.
            for name in (f"{folder}{n}" for n in {n for n, _, _ in gen.REGIONS} for folder in gen.LANGS.values()):
                (gen.OUT / name).parent.mkdir(parents=True, exist_ok=True)
                (gen.OUT / name).write_text((saved / name).read_text(encoding="utf-8"), encoding="utf-8")
            try:
                self.assertEqual(gen.main([]), 0)
                page = gen.OUT / "reference" / "boards.md"
                page.write_text(page.read_text(encoding="utf-8").replace("rp2040", "rp2041"), encoding="utf-8")
                self.assertEqual(gen.main(["--check"]), 1)
                page.unlink()
                self.assertEqual(gen.main(["--check"]), 1)
                self.assertEqual(gen.main([]), 0)
                hand = gen.OUT / "reference" / "extension-manifest.md"
                hand.write_text(hand.read_text(encoding="utf-8").replace("| `trusted` |", "| `trust` |"), encoding="utf-8")
                self.assertEqual(gen.main(["--check"]), 1, "a generated region edited by hand")
            finally:
                gen.OUT = saved

    def test_each_language_has_its_own_reference_page(self):
        for name in gen.PAGES:
            ja = (gen.OUT / "reference" / name).read_text(encoding="utf-8")
            en = (gen.OUT / "en" / "reference" / name).read_text(encoding="utf-8")
            self.assertIn("から生成しています", ja, name)
            self.assertIn("Generated from", en, name)

    def test_every_route_of_the_router_is_listed(self):
        router = (gen.SERVE / "api.rs").read_text(encoding="utf-8").split("pub fn handle(", 1)[1].split("\n    };", 1)[0]
        arms = re.findall(r'^\s*\("(GET|POST|PUT|DELETE)",', router, re.M)
        rows = re.findall(r"^\| `(GET|POST|PUT|DELETE)` \|", gen.serve_api(), re.M)
        self.assertEqual(len(rows), len(arms))

    def test_every_board_and_mcu_definition_is_listed(self):
        page = gen.boards()
        for kind, file in (("mcu", "dagon-mcu.toml"), ("boards", "dagon-board.toml")):
            for f in (ROOT / "targets" / kind).glob(f"*/{file}"):
                ident = tomllib.loads(f.read_text(encoding="utf-8"))["id"]
                self.assertIn(f"| `{ident}` |", page)

    def test_every_key_of_the_agent_types_is_listed(self):
        page = gen.agent_definition()
        for path, name in ((gen.AGENTS / "mod.rs", "AgentDef"), (gen.AGENTS / "mod.rs", "WayDef"),
                           (gen.AGENTS / "command.rs", "Command"), (gen.AGENTS / "http.rs", "MessagesHttp")):
            fields = gen.rust_struct(path, name)
            self.assertTrue(fields, name)
            for field, *_ in fields:
                self.assertIn(f"| `{field}` |", page)

    def test_every_command_has_its_section(self):
        text = gen.cli_markdown()
        top = re.search(r"Commands:\n(.*?)\n\n", text, re.S).group(1)
        for name in re.findall(r"^  (\w[\w-]*) ", top, re.M):
            if name != "help":
                self.assertIn(f"## dagon {name}\n", text, name)

    def test_every_workspace_crate_is_listed(self):
        page = gen.rust_api()
        for manifest in ROOT.glob("crates/*/Cargo.toml"):
            name = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["name"]
            self.assertIn(f"[`{name}`]", page)

    def test_every_route_and_agent_key_says_what_it_is(self):
        # A blank cell means the source has no doc comment for it: write one there.
        tables = [make("en") for _, _, make in gen.REGIONS]
        for page in (gen.serve_api(), gen.agent_definition(), *tables):
            blank = [l for l in page.splitlines() if l.startswith("| `") and l.rstrip().endswith("|  |")]
            self.assertEqual(blank, [])

    def test_angle_brackets_in_a_source_comment_stay_text(self):
        self.assertEqual(gen.cell("<dir>/x `<y>`"), "&lt;dir&gt;/x `<y>`")


class Llms(unittest.TestCase):
    def test_lists_every_default_locale_page_and_the_demo(self):
        text = llms.render("https://example.invalid/site")
        self.assertIn("https://example.invalid/site/demo/", text)
        for p in llms.DOCS.rglob("*.md*"):
            self.assertIn(f"[{llms.frontmatter(p)[0]}]", text, p)
        self.assertIn("https://example.invalid/site/doc/getting-started/install/", text)
        self.assertIn("https://example.invalid/site/doc/en/getting-started/install/", text)


class Examples(unittest.TestCase):
    def test_site_examples_parse(self):
        self.assertEqual(examples.problems(), [])

    def test_a_broken_example_is_named(self):
        with tempfile.TemporaryDirectory() as d:
            (pathlib.Path(d) / "x.md").write_text("```toml\nid = \n```\n\n```toml nocheck\nid =\n```\n", encoding="utf-8")
            found = examples.problems(pathlib.Path(d))
            self.assertEqual(len(found), 1)
            self.assertTrue(found[0].startswith("x.md:1:"))


class BuiltPages(unittest.TestCase):
    def test_the_served_page_carries_none_of_the_demo(self):
        self.assertEqual(pages.served_problems(), [])

    def test_a_fixture_a_stand_in_hub_or_an_agent_name_is_found(self):
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            (root / "assets").mkdir()
            (root / "assets" / "sample-full-1.1.0-x.dgb").write_bytes(b"\0")
            (root / "assets" / "index-x.js").write_bytes(b'devHub(new URLSearchParams("sim")); "Claude Code"')
            self.assertEqual(len(pages.served_problems(root)), 3)
            self.assertEqual(len(pages.demo_problems(root)), 1)


class MovedDocuments(unittest.TestCase):
    """Every `docs/<name>.md` the repository mentions exists: documents moved into the site are named there."""

    MENTION = re.compile(r"(?<![\w/])(?:\.\./)*docs/[\w./-]+\.md")
    SAMPLES = {"scripts/test_markers.py", "site/scripts/test_site.py"}

    def test_no_mention_of_a_document_that_is_not_there(self):
        import subprocess
        names = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                               check=True, capture_output=True).stdout.decode("utf-8").split("\0")
        missing = []
        for name in names:
            path = ROOT / name
            # The built page, and the checks whose own samples name such paths on purpose.
            if not name or name.startswith("crates/dagon-cli/page/") or name in self.SAMPLES or not path.is_file():
                continue
            data = path.read_bytes()
            if b"\0" in data:
                continue
            for no, line in enumerate(data.decode("utf-8", errors="replace").splitlines(), 1):
                for m in self.MENTION.finditer(line):
                    rel = m.group(0).lstrip("./")
                    if not (ROOT / rel).is_file() and not (path.parent / m.group(0)).is_file():
                        missing.append(f"{name}:{no}: {m.group(0)}")
        self.assertEqual(missing, [])

    def test_the_pattern_finds_old_paths_and_leaves_site_paths(self):
        self.assertTrue(self.MENTION.search("follow `docs/versioning.md` and"))
        self.assertTrue(self.MENTION.search("(../../docs/extension-manifest.md#boot)"))
        self.assertIsNone(self.MENTION.search("site/src/content/docs/reference/events.md"))


class Wording(unittest.TestCase):
    def test_writing_to_a_node_is_not_called_koushin_in_japanese(self):
        pages = [p for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"]
        pages.append(ROOT / "README.md")
        found = []
        for p in pages:
            rel = p.relative_to(ROOT).as_posix()
            for n, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
                # The README's opening calls what is written after the first time 更新用イメージ: a kind of image, not the act.
                if "更新" in (line.replace("更新用イメージ", "") if p.name == "README.md" else line):
                    found.append(f"{rel}:{n}")
        self.assertEqual(found, [])

    # Words the Japanese pages do not use for DAGON's own things.
    NOT_SAID = ["ブートコア", "給餌", "見守り", "武装", "運び手", "開始者", "トライアル", "将来の", "マイコンのパッケージ", "ボードのパッケージ", "MCUのパッケージ", "ルールます", "破損していないの記録",
                # パッケージ is the .dgb file and nothing else; the verb is 書き込む, in full.
                "npmパッケージ", "Cargoパッケージ", "書込み", "初回書", "書く前",
                # One word for one thing.
                "クレート",
                # A sentence says what it refers to, and who does it.
                "同様です", "だけで扱えます",
                # Names of the English pages' columns.
                "Up links", "Down links"]

    def test_the_japanese_pages_do_not_use_these_words(self):
        pages = [p for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"]
        pages += [ROOT / "README.md", ROOT / "site" / "i18n" / "ja.toml", ROOT / "site" / "i18n" / "catalog-ja.toml"]
        found = [f"{p.relative_to(ROOT).as_posix()}: {w}" for p in pages for w in self.NOT_SAID if w in p.read_text(encoding="utf-8")]
        self.assertEqual(found, [])

    def test_plugin_is_only_the_claude_code_plugin_in_japanese(self):
        # DAGON's own word is 拡張.
        pages = [p for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"]
        found = [f"{p.name}:{n}" for p in pages for n, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1)
                 if "プラグイン" in line.replace("Claude Codeのプラグイン", "")]
        self.assertEqual(found, [])

    def test_the_english_design_pages_use_one_word_for_each_thing(self):
        for rel in ("en/explanation/lifecycle-events.md", "en/explanation/retained-formats.md", "en/reference/ports.md"):
            text = (llms.DOCS / rel).read_text(encoding="utf-8")
            self.assertEqual(re.findall(r"(?i)plug-in|\bboot\s+core\b", text), [], rel)

    # Status of the code against the design is said once, on the limitations page.
    STATUS = re.compile(r"未確認|まだ|現在|今は|実測|確かめていません|するまでは|測定で確かめ"
                        r"|\byet\b|\bcurrently\b|\bfor now\b|\btoday\b|\bnot measured\b|\buntil then\b|\bto be confirmed\b|\bonce\b.*\bmeasured\b")
    # What the status words may still appear in, and why.
    STATUS_QUOTES = [
        *NOTICE_LINES,  # the alpha notice, word for word
        "まだ使えない",  # how-to/swap-link quotes the Console's label and problem name
        "not available yet on rp2040",  # how-to/swap-link quotes dagon gen's refusal
    ]

    def test_status_words_only_on_the_limitations_page(self):
        pages = [ROOT / "README.md", ROOT / "README.en.md", *llms.DOCS.rglob("*.md*")]
        found = []
        for p in pages:
            rel = p.relative_to(llms.DOCS).as_posix() if p.is_relative_to(llms.DOCS) else p.name
            if p.name == "limitations.md":
                continue
            for n, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
                for quote in self.STATUS_QUOTES:
                    line = line.replace(quote, "")
                if self.STATUS.search(line):
                    found.append(f"{rel}:{n}")
        self.assertEqual(found, [])

    def test_no_page_says_the_case_is_never_opened(self):
        # One connection writes every node; whether the case opens depends on where that port is.
        flat = re.compile(r"ケースを開ける必要|ケースを開けることも|(?i:nothing is opened|need to open the case)")
        found = [p.name for p in (ROOT / "README.md", ROOT / "README.en.md", *llms.DOCS.rglob("*.md*")) if flat.search(p.read_text(encoding="utf-8"))]
        self.assertEqual(found, [])

    # Pages state requirements and facts of the design, never what was run, measured or confirmed.
    REPORTED = re.compile(r"\dで確認|確かめていません|確かめていない|確認済|実機で確かめ|実機で測定|シミュレータでだけ|CIで|ボードごとには|モデルは.*見つけ"
                          r"|(?i:\bin CI\b|\bCI runs\b|tested on|checked on \d|confirmed on \d|not verified|not confirmed by|not run on hardware|the model found|measured on hardware|\d+ of \d+ pass)")
    # What the words describe here is a rule for a definition's data, not something that was run.
    REPORTED_QUOTES = ["出典を確かめていない", "ボードで確かめていない", "グラフを確かめていない", "確かめていないと報告"]

    def test_no_page_reports_what_was_run(self):
        found = []
        for p in (ROOT / "README.md", ROOT / "README.en.md", *llms.DOCS.rglob("*.md*")):
            for n, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
                for quote in self.REPORTED_QUOTES:
                    line = line.replace(quote, "")
                if self.REPORTED.search(line):
                    found.append(f"{p.name}:{n}")
        self.assertEqual(found, [])

    def test_no_page_names_a_dagon_release(self):
        found = [p.name for p in llms.DOCS.rglob("*.md*") if re.search(r"cli-v\d", p.read_text(encoding="utf-8"))]
        self.assertEqual(found, [])

    def test_console_goes_by_its_full_name_on_the_pages(self):
        found = []
        for p in llms.DOCS.rglob("*.md*"):
            if p.name == "cli.md":  # the command help, quoted from the clap definitions
                continue
            found += [f"{p.relative_to(ROOT).as_posix()}: {m}" for m in re.findall(r".{0,20}(?<!DAGON )Console.{0,20}", p.read_text(encoding="utf-8"))]
        self.assertEqual(found, [])

    def test_a_japanese_page_links_a_page_by_its_japanese_title(self):
        found = []
        for p in llms.DOCS.rglob("*.md*"):
            if p.relative_to(llms.DOCS).parts[0] == "en":
                continue
            for label, target in re.findall(r"\[([^\]]+)\]\(/([\w/-]+)/(?:#[^)]*)?\)", p.read_text(encoding="utf-8")):
                en = next((f for f in (llms.DOCS / "en" / f"{target}{e}" for e in (".md", ".mdx", "/index.md")) if f.is_file()), None)
                if en and llms.frontmatter(en)[0] == label != llms.frontmatter(llms.DOCS / en.relative_to(llms.DOCS / "en"))[0]:
                    found.append(f"{p.relative_to(ROOT).as_posix()}: {label}")
        self.assertEqual(found, [])

    def test_every_page_is_in_both_languages(self):
        root = {p.relative_to(llms.DOCS).as_posix() for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"}
        en = {p.relative_to(llms.DOCS / "en").as_posix() for p in (llms.DOCS / "en").rglob("*.md*")}
        self.assertEqual(root ^ en, set())

    def test_the_english_fallback_notice_does_not_promise_a_translation(self):
        notice = (llms.DOCS.parent / "i18n" / "en.json").read_text(encoding="utf-8")
        self.assertNotIn("yet", notice)


def cargo_deps():
    """Package name -> its dependencies' names, from `cargo metadata` of the root and the packages outside it."""
    manifests = [ROOT / "Cargo.toml", ROOT / "samples/f401-app/Cargo.toml", ROOT / "samples/hub-boot-screen/Cargo.toml", ROOT / "targets/rp2040-hub/Cargo.toml"]
    for sub in ("rp2040", "rp2040/stage0", "rp2040/bootcore", "stm32f401re", "stm32f401re/stage0", "stm32f401re/bootcore", "esp32s3", "common"):
        manifests.append(ROOT / "targets/mcu" / sub / "Cargo.toml")
    manifests += sorted((ROOT / "targets/boards").glob("*/Cargo.toml"))
    deps = {}
    for m in manifests:
        out = subprocess.run(["cargo", "metadata", "--no-deps", "--offline", "--format-version", "1", "--manifest-path", str(m)],
                             cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True).stdout
        for p in json.loads(out)["packages"]:
            deps.setdefault(p["name"], set()).update(d["name"] for d in p["dependencies"] if d["kind"] in (None, "build"))
    return deps


class Architecture(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.deps = cargo_deps()

    def test_the_core_names_no_mcu_or_board(self):
        for name in gen.architecture.NO_MCU:
            bad = [d for d in self.deps[name] if d.startswith(("dagon-mcu", "dagon-board")) or d in gen.architecture.HAL]
            self.assertEqual(bad, [], name)

    def test_the_figure_draws_exactly_the_components_in_the_table(self):
        a = gen.architecture
        for name, lines in a.COMPONENTS.items():
            self.assertEqual(len(lines), 2, name)
            for line in lines:
                self.assertNotIn(line.strip(), ("", "—", "-", "–"), f"{name} has no line")
        for lang in gen.LANGS:
            svg = a.node_svg(lang).split('<g class="legend">')[0]
            # a long name is drawn over two lines
            drawn = "".join(re.findall(r">([^<>]+)</text>", svg)).replace(" ", "")
            for name in a.COMPONENTS:
                self.assertIn(name.replace(" ", ""), drawn, name)
        named = [n for row in a.PC_ROWS for cell in (row[2] if row[1] == "full" else [x for c in row[2] for x in c]) for n in ([cell] if isinstance(cell, str) else cell)]
        named += [*a.CONTRACTS, *a.BOARD, a.MCU_STRIP, *a.SILICON, *a.LIBRARIES, *a.PROOF, *a.DIRECT]
        named += [n for _, names in a.DEVICE_TOP for n in names]
        named += [n for _, cells in a.DEVICE_COLS for c in cells.values() for n in c] + [n for c in a.MCU.values() for n in c]
        self.assertEqual(sorted(named), sorted(a.COMPONENTS))

    def test_the_colours_are_the_consoles_tokens(self):
        tokens = (ROOT / "web/src/styles/tokens.css").read_text(encoding="utf-8")
        for colour in gen.architecture.COLOURS:
            self.assertIn(colour, tokens)
        for colour in re.findall(r"#[0-9a-f]{6}", gen.architecture.node_svg("en")):
            self.assertIn(colour, gen.architecture.COLOURS)

    def test_the_change_table_and_how_to_name_paths_that_exist(self):
        pages = [ROOT / "README.md", ROOT / "README.en.md", llms.DOCS / "how-to/new-mcu-or-link.md", llms.DOCS / "en/how-to/new-mcu-or-link.md"]
        for page in pages:
            text = page.read_text(encoding="utf-8")
            rows = [l for l in text.splitlines() if l.startswith("| ") and "crates/" in l or l.startswith("| ") and "targets/" in l] if page.name.startswith("README") else text.splitlines()
            for line in rows:
                for path in re.findall(r"`((?:crates|targets|plugin)/[^`]+)`", line):
                    self.assertTrue(list(ROOT.glob(path.replace("<id>", "*").rstrip("/"))), f"{page.name}: {path}")


class JapaneseSpacing(unittest.TestCase):
    """Japanese takes no half-width space beside Latin letters, digits or code."""

    def test_the_japanese_pages_have_no_such_space(self):
        pages = [p for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"]
        found = {p.relative_to(llms.DOCS).as_posix(): lines_with_spaces(p.read_text(encoding="utf-8")) for p in pages}
        self.assertEqual({k: v for k, v in found.items() if v}, {})

    def test_the_japanese_sources_have_no_such_space(self):
        values = [*gen.i18n().values(), *tomllib.loads((ROOT / "site/i18n/catalog-ja.toml").read_text(encoding="utf-8")).values(),
                  *(ja for ja, _ in architecture.COMPONENTS.values())]
        self.assertEqual([v for v in values if spaces(v)], [])

    def test_what_the_rule_finds_and_leaves(self):
        for text in ("DAGON とは", "1 か所", "`dagon new` と", "は `x`", "[Rust](https://rustup.rs) だけ", "**DAGON** は", "---\ntitle: DAGON を試す\n---\n"):
            self.assertTrue(spaces(text), text)
        for text in ("DAGON Console", "`a の b`", "```\nDAGON を\n```\n", "### 1.1 名前", "| 親機 |", "- 名前", "**名前。** DAGON", "**`x`** の", "50 ms"):
            self.assertEqual(spaces(text), [], text)


class Translation(unittest.TestCase):
    """The design and format pages are normative in both languages, so a translation may not drop a fact."""
    PAGES = ["explanation/lifecycle-events.md", "explanation/retained-formats.md", "explanation/versioning.md", "reference/agent-api.md",
             "reference/definitions.md", "reference/extension-manifest.md"]

    @staticmethod
    def facts(rel, folder):
        text = (llms.DOCS / folder / rel).read_text(encoding="utf-8").split("---\n", 2)[2]
        fences = re.findall(r"^( *)```([^\n]*)\n(.*?)^\1```$", text, re.M | re.S)
        body = re.sub(r"^( *)```[^\n]*\n.*?^\1```$", "", text, flags=re.M | re.S)
        # Tables generated from sources are checked by gen.py; a Mermaid chart's labels are translated.
        body = re.sub(r"<!-- generated from.*?<!-- end of generated table -->", "", body, flags=re.S)
        return {
            "headings": [len(h) for h in re.findall(r"^(#+) ", body, re.M)],
            "fences": [f if lang.startswith("mermaid") is False else "mermaid" for _, lang, f in fences],
            "rows": [line.count("|") for line in body.splitlines() if line.startswith("|")],
            "code": sorted(" ".join(c.split()) for c in re.findall(r"`[^`]+`", body)),
            "links": sorted(re.sub(r"^/en/|#.*$", lambda m: "/" if m.group().startswith("/en") else "", u)
                            for u in re.findall(r"\]\(([^)]+)\)", body)),
        }

    def test_a_translated_page_keeps_every_heading_fence_row_code_span_and_link(self):
        for rel in self.PAGES:
            ja, en = self.facts(rel, ""), self.facts(rel, "en")
            for what in en:
                with self.subTest(page=rel, what=what):
                    self.assertEqual(ja[what], en[what])


class ToolKinds(unittest.TestCase):
    def test_every_doctor_entry_has_a_kind(self):
        gen.NO_KIND.clear()
        for lang in gen.LANGS:
            gen.tools(lang)
        self.assertEqual(gen.NO_KIND, set())

    def test_an_entry_without_a_kind_fails_the_generator(self):
        gen.NO_KIND.clear()
        self.assertEqual(gen.tool_kind("en", {"what": "new tool", "probe": ["new-tool"]}), "")
        self.assertEqual(gen.main(["--check"]), 1)
        gen.NO_KIND.clear()


class GeneratedJapanese(unittest.TestCase):
    def test_a_description_without_a_japanese_key_fails_the_generator(self):
        gen.MISSING.clear()
        self.assertEqual(gen.say("ja", "no such description"), "no such description")
        self.assertEqual(gen.MISSING, {"no such description"})
        self.assertEqual(gen.main(["--check"]), 1)
        gen.MISSING.clear()
        self.assertEqual(gen.say("en", "no such description"), "no such description")
        self.assertEqual(gen.MISSING, set())

    def test_a_translated_description_keeps_its_code_spans(self):
        spans = lambda t: sorted(re.findall(r"`[^`]*`", t))
        for en, ja in gen.i18n().items():
            self.assertEqual(spans(en), spans(ja), en)

    def test_the_japanese_pages_say_what_stays_english(self):
        for rel, words in (("reference/cli.md", "コマンドの説明"), ("reference/rust-api.md", "rustdocの本文")):
            self.assertIn(f"{words}は元のソースのままで、英語です。", (llms.DOCS / rel).read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
