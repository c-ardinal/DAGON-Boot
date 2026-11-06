"""Checks of the site's generators: python -m unittest discover -s site/scripts -p "test_*.py"."""
import fnmatch
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
    # Pages where 更新 does not mean writing to a node: the word for that is 書き込む.
    KOUSHIN_ELSEWHERE = {"site/src/content/docs/reference/licenses.md": "the licence text and third-party notes"}

    def test_writing_to_a_node_is_not_called_koushin_in_japanese(self):
        pages = [p for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"]
        pages.append(ROOT / "README.md")
        found = []
        for p in pages:
            rel = p.relative_to(ROOT).as_posix()
            if rel in self.KOUSHIN_ELSEWHERE:
                continue
            found += [f"{rel}:{n}" for n, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1) if "更新" in line]
        self.assertEqual(found, [])

    # Words the Japanese pages do not use for DAGON's own things.
    NOT_SAID = ["ブートコア", "給餌", "見守り", "武装", "運び手", "開始者", "トライアル", "将来の", "マイコンのパッケージ", "ボードのパッケージ", "MCU のパッケージ", "ルールます", "破損していないの記録"]

    def test_the_japanese_pages_do_not_use_these_words(self):
        pages = [p for p in llms.DOCS.rglob("*.md*") if p.relative_to(llms.DOCS).parts[0] != "en"]
        pages += [ROOT / "README.md", ROOT / "site" / "i18n" / "ja.toml", ROOT / "site" / "i18n" / "catalog-ja.toml"]
        found = [f"{p.relative_to(ROOT).as_posix()}: {w}" for p in pages for w in self.NOT_SAID if w in p.read_text(encoding="utf-8")]
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

    def test_every_module_in_the_figure_exists(self):
        for name, mod in gen.architecture.MODULES.items():
            for path in mod["paths"]:
                self.assertTrue((ROOT / path).exists(), f"{name}: {path}")
            if mod["needle"]:
                self.assertIn(mod["needle"], (ROOT / mod["paths"][0]).read_text(encoding="utf-8"), name)
            self.assertTrue(mod["crate"] in self.deps or any(mod["crate"] in d for d in self.deps.values()), f"{name}: {mod['crate']}")

    def test_the_figure_draws_exactly_the_modules_in_the_table(self):
        for lang in gen.LANGS:
            svg = gen.architecture.node_svg(lang)
            for name in gen.architecture.MODULES:
                self.assertIn(f">{name}</text>", svg, name)
            drawn = re.findall(r'class="a-b"[^>]*>([^<]+)</text>', svg)
            self.assertEqual(sorted(drawn), sorted(gen.architecture.MODULES))

    def test_every_mcu_in_the_figure_is_in_targets(self):
        self.assertEqual(len(gen.architecture.mcu_names()), len(list((ROOT / "targets/mcu").glob("*/dagon-mcu.toml"))))

    def test_the_core_names_no_mcu_or_board(self):
        for name in gen.architecture.NO_MCU:
            bad = [d for d in self.deps[name] if d.startswith(("dagon-mcu", "dagon-board")) or d in gen.architecture.HAL]
            self.assertEqual(bad, [], name)

    def test_the_change_table_and_how_to_name_paths_that_exist(self):
        pages = [ROOT / "README.md", ROOT / "README.en.md", llms.DOCS / "how-to/new-mcu-or-link.md", llms.DOCS / "en/how-to/new-mcu-or-link.md"]
        for page in pages:
            text = page.read_text(encoding="utf-8")
            rows = [l for l in text.splitlines() if l.startswith("| ") and "crates/" in l or l.startswith("| ") and "targets/" in l] if page.name.startswith("README") else text.splitlines()
            for line in rows:
                for path in re.findall(r"`((?:crates|targets|plugin)/[^`]+)`", line):
                    self.assertTrue(list(ROOT.glob(path.replace("<id>", "*").rstrip("/"))), f"{page.name}: {path}")


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
        for rel, words in (("reference/cli.md", "コマンドの説明"), ("reference/rust-api.md", "rustdoc の本文")):
            self.assertIn(f"{words}は元のソースのままで、英語です。", (llms.DOCS / rel).read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
