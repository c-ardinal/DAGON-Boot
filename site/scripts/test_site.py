"""Checks of the site's generators: python -m unittest discover -s site/scripts -p "test_*.py"."""
import pathlib
import re
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
            for name in {n for n, _, _ in gen.REGIONS}:
                (gen.OUT / name).write_text((saved / name).read_text(encoding="utf-8"), encoding="utf-8")
            try:
                self.assertEqual(gen.main([]), 0)
                page = gen.OUT / "boards.md"
                page.write_text(page.read_text(encoding="utf-8").replace("rp2040", "rp2041"), encoding="utf-8")
                self.assertEqual(gen.main(["--check"]), 1)
                page.unlink()
                self.assertEqual(gen.main(["--check"]), 1)
                self.assertEqual(gen.main([]), 0)
                hand = gen.OUT / "extension-manifest.md"
                hand.write_text(hand.read_text(encoding="utf-8").replace("| `trusted` |", "| `trust` |"), encoding="utf-8")
                self.assertEqual(gen.main(["--check"]), 1, "a generated region edited by hand")
            finally:
                gen.OUT = saved

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
        tables = [make() for _, _, make in gen.REGIONS]
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
            if p.relative_to(llms.DOCS).parts[0] == "ja":
                continue
            self.assertIn(llms.frontmatter(p)[0], text, p)
        self.assertNotIn("/doc/ja/", text)


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
    def test_a_japanese_page_in_english_says_only_that(self):
        # Starlight's own notice says a translation is missing ("まだ"); the site replaces it.
        notice = (llms.DOCS.parent / "i18n" / "ja.json").read_text(encoding="utf-8")
        self.assertIn("このページは英語です。", notice)
        self.assertNotIn("まだ", notice)


if __name__ == "__main__":
    unittest.main()
