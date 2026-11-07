"""Checks of scripts/bump_check.py (no git needed).

    python -m unittest discover -s scripts -p "test_*.py"
"""
import importlib.util
import pathlib
import shutil
import subprocess
import sys
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


def cli_version():
    return next(c["version"] for c in bc.load_components() if c["name"] == "cli")


def a_later_version():
    return f"{bc.parse_version(cli_version())[0][0] + 1}.0.0"


class ReleaseTag(unittest.TestCase):
    NOT_A_TAG = "is not"

    def test_a_final_tag_and_a_pre_release_of_the_compat_version_carry_their_version(self):
        self.assertEqual(bc.tag_version("cli-v0.1.0", "cli", "0.1.0"), "0.1.0")
        self.assertEqual(bc.tag_version("cli-v0.1.0-rc.1", "cli", "0.1.0"), "0.1.0-rc.1")
        self.assertEqual(bc.tag_version("cli-v0.1.0-rc.12", "cli", "0.1.0"), "0.1.0-rc.12")

    def test_any_other_tag_is_refused(self):
        for bad in ("cli-v0.2.0", "cli-v0.2.0-rc.1", "cli-v0.1.0-rc", "cli-v0.1.0-rc.", "cli-v0.1.0-rc.01",
                    "cli-v0.1.0-rc.1x", "cli-v0.1.0-beta.1", "cli-v0.1.0-rc.1-rc.2", "cli-v0.1.0-rc.1/../x",
                    "cli-v0x1x0", "boot-rp2040-v0.1.0", "v0.1.0"):
            with self.subTest(bad=bad):
                with self.assertRaisesRegex(ValueError, self.NOT_A_TAG):
                    bc.tag_version(bad, "cli", "0.1.0")

    def bump_check(self, tag):
        return subprocess.run([sys.executable, str(HERE / "bump_check.py"), "--tag", tag],
                              capture_output=True, text=True, encoding="utf-8")

    def test_the_command_takes_both_tags_of_the_compat_version(self):
        # Whether the commits since the last tag need a bump is another check's answer.
        for tag in (f"cli-v{cli_version()}", f"cli-v{cli_version()}-rc.1"):
            with self.subTest(tag=tag):
                self.assertNotIn(self.NOT_A_TAG, self.bump_check(tag).stderr)

    def test_the_command_fails_a_pre_release_of_another_version(self):
        r = self.bump_check(f"cli-v{a_later_version()}-rc.1")
        self.assertEqual(r.returncode, 1)
        self.assertIn(self.NOT_A_TAG, r.stderr)


rspec = importlib.util.spec_from_file_location("release", HERE / "release.py")
rel = importlib.util.module_from_spec(rspec)
rspec.loader.exec_module(rel)


class ReleaseNaming(unittest.TestCase):
    def test_without_a_tag_the_version_is_the_compat_one(self):
        self.assertEqual(rel.release_version(None), cli_version())

    def test_a_final_tag_names_the_compat_version(self):
        self.assertEqual(rel.release_version(f"cli-v{cli_version()}"), cli_version())

    def test_a_pre_release_tag_names_its_own_version(self):
        self.assertEqual(rel.release_version(f"cli-v{cli_version()}-rc.1"), f"{cli_version()}-rc.1")

    def test_archive_names_are_the_ones_the_install_scripts_build(self):
        self.assertEqual(rel.archive_name("1.2.3", "x86_64-unknown-linux-gnu"), "dagon-cli-1.2.3-x86_64-unknown-linux-gnu.tar.gz")
        self.assertEqual(rel.archive_name("1.2.3-rc.1", "x86_64-unknown-linux-gnu"), "dagon-cli-1.2.3-rc.1-x86_64-unknown-linux-gnu.tar.gz")
        self.assertEqual(rel.archive_name("1.2.3-rc.1", "x86_64-pc-windows-msvc"), "dagon-cli-1.2.3-rc.1-x86_64-pc-windows-msvc.zip")

    def test_a_tag_of_another_version_or_component_fails_before_any_build(self):
        for bad in (f"cli-v{a_later_version()}-rc.1", f"boot-rp2040-v{cli_version()}", f"cli-v{cli_version()}-beta.1"):
            with self.subTest(bad=bad):
                r = subprocess.run([sys.executable, str(HERE / "release.py"), "--tag", bad],
                                   capture_output=True, text=True, encoding="utf-8")
                self.assertEqual(r.returncode, 1)
                self.assertIn("is not", r.stderr)
                self.assertNotIn("+ ", r.stdout)


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
