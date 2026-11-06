"""Checks of scripts/bump_check.py (no git needed).

    python -m unittest discover -s scripts -p "test_*.py"
"""
import importlib.util
import pathlib
import shutil
import subprocess
import tempfile
import tomllib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("bump_check", HERE / "bump_check.py")
bc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bc)


class Kinds(unittest.TestCase):
    def test_conventional_types_map_to_kinds(self):
        self.assertEqual(bc.commit_kind("feat(cli): new subcommand", ""), "feature")
        self.assertEqual(bc.commit_kind("fix(boot-rp2040): CRC", ""), "fix")
        self.assertEqual(bc.commit_kind("perf: faster", ""), "fix")
        self.assertEqual(bc.commit_kind("docs: typo", ""), "none")
        self.assertEqual(bc.commit_kind("feat(cli)!: rename --port", ""), "breaking")
        self.assertEqual(bc.commit_kind("fix: x", "BREAKING CHANGE: y"), "breaking")

    def test_unlabelled_commits_count_as_fixes(self):
        self.assertEqual(bc.commit_kind("Update things", ""), "fix")
        self.assertEqual(bc.commit_kind("wip(cli): odd type", ""), "fix")


class Bumps(unittest.TestCase):
    def test_zero_x_follows_cargo(self):
        self.assertEqual(bc.required((0, 1, 0), "breaking"), (0, 2, 0))
        self.assertEqual(bc.required((0, 1, 0), "feature"), (0, 1, 1))
        self.assertEqual(bc.required((0, 1, 0), "fix"), (0, 1, 1))
        self.assertEqual(bc.required((0, 1, 0), "none"), (0, 1, 0))

    def test_from_one_follows_semver(self):
        self.assertEqual(bc.required((1, 2, 3), "breaking"), (2, 0, 0))
        self.assertEqual(bc.required((1, 2, 3), "feature"), (1, 3, 0))
        self.assertEqual(bc.required((1, 2, 3), "fix"), (1, 2, 4))

    def test_a_bump_lower_than_the_commits_need_is_refused(self):
        self.assertEqual(bc.check("cli", "0.2.0", (0, 1, 0), ["fix", "breaking"]), [])
        self.assertTrue(bc.check("cli", "0.1.1", (0, 1, 0), ["fix", "breaking"]))
        self.assertTrue(bc.check("cli", "1.2.4", (1, 2, 3), ["feature"]))
        self.assertEqual(bc.check("cli", "0.2.0-rc.1", (0, 1, 0), ["breaking"]), [])

    def test_first_release_is_0_1_0(self):
        self.assertEqual(bc.check("cli", "0.1.0", None, []), [])
        self.assertTrue(bc.check("cli", "1.0.0", None, []))


class Ownership(unittest.TestCase):
    def test_longest_path_wins(self):
        comps = tomllib.loads((HERE.parent / "compat.toml").read_text(encoding="utf-8"))["component"]
        self.assertEqual(bc.owner("targets/mcu/rp2040/bootcore/src/main.rs", comps), "boot-rp2040")
        self.assertEqual(bc.owner("targets/mcu/rp2040/src/lib.rs", comps), "mcu-rp2040")
        self.assertEqual(bc.owner("web/src/App.svelte", comps), "cli")
        self.assertIsNone(bc.owner("README.md", comps))
        self.assertIsNone(bc.owner("crates/dagon-formatx/x.rs", comps))


    def test_the_tag_names_its_component(self):
        comps = bc.load_components()
        self.assertEqual(bc.component_of_tag("cli-v0.2.0", comps), "cli")
        self.assertEqual(bc.component_of_tag("boot-rp2040-v0.1.1-rc.1", comps), "boot-rp2040")
        self.assertEqual(
            bc.component_of_tag("board-waveshare-rp2040-touch-lcd-1.28-v0.1.0", comps),
            "board-waveshare-rp2040-touch-lcd-1.28",
        )
        self.assertIsNone(bc.component_of_tag("v0.1.0", comps))
        self.assertIsNone(bc.component_of_tag("rp2040-hub-v0.1.0", comps))


cspec = importlib.util.spec_from_file_location("changelog", HERE / "changelog.py")
cl = importlib.util.module_from_spec(cspec)
cspec.loader.exec_module(cl)


class ReleaseNotes(unittest.TestCase):
    def test_the_cli_version_has_its_section_in_the_changelog(self):
        root = HERE.parent
        compat = tomllib.loads((root / "compat.toml").read_text(encoding="utf-8"))
        version = next(c["version"] for c in compat["component"] if c["name"] == "cli")
        text = (root / "CHANGELOG.md").read_text(encoding="utf-8") if (root / "CHANGELOG.md").is_file() else ""
        self.assertIn(f"\n## cli {version}\n", text)


class Changelog(unittest.TestCase):
    def test_sections_follow_the_kinds_and_skip_no_release_commits(self):
        kinds = [("feature", "feat(cli): a"), ("none", "docs: b"), ("breaking", "feat(cli)!: c"),
                 ("fix", "fix(cli): d")]
        text = cl.section("cli", "0.2.0", "cli-v0.1.0", kinds)
        # The heading is the one the CHANGELOG holds; the range is a line of its own.
        self.assertTrue(text.startswith("## cli 0.2.0\n\nSince cli-v0.1.0.\n"), text)
        self.assertLess(text.index("### Breaking"), text.index("### Features"))
        self.assertLess(text.index("### Features"), text.index("### Fixes"))
        self.assertNotIn("docs: b", text)

    def test_nothing_to_release_says_so(self):
        self.assertIn("No changes", cl.section("cli", "0.1.0", None, [("none", "docs: x")]))


class LastRelease(unittest.TestCase):
    def test_the_previous_release_is_found_among_tags_merged_into_the_given_rev(self):
        repo = pathlib.Path(tempfile.mkdtemp(prefix="dagon-bump-"))
        def git(*a):
            return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t",
                                   "-c", "commit.gpgsign=false", *a], cwd=repo, check=True,
                                  capture_output=True, text=True).stdout.strip()
        saved = bc.ROOT
        try:
            git("init", "-q")
            for i, tag in enumerate(["x-v0.1.0", None, "x-v0.2.0"]):
                git("commit", "-q", "--allow-empty", "-m", f"c{i}")
                if tag:
                    git("tag", tag)
            middle = git("rev-parse", "HEAD~1")
            bc.ROOT = repo
            self.assertEqual(bc.last_release("x", "9.0.0")[1], "x-v0.2.0")
            self.assertEqual(bc.last_release("x", "9.0.0", at=middle)[1], "x-v0.1.0")
        finally:
            bc.ROOT = saved
            shutil.rmtree(repo, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
