"""Checks of scripts/page.py and of the page it builds: the built page stays
out of git, and the page takes its release key the way firmware does.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import subprocess
import unittest

import page

ROOT = page.ROOT


class BuiltPage(unittest.TestCase):
    """The built page is build output: never in git, so parallel branches never
    meet on it; releases and CI build it from web/."""

    def git(self, *args):
        return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)

    def test_the_built_page_is_not_tracked(self):
        tracked = self.git("ls-files", "--", "crates/dagon-cli/page")
        self.assertEqual(tracked.returncode, 0, tracked.stderr)
        self.assertEqual(tracked.stdout, "")

    def test_the_built_page_is_ignored(self):
        for name in ("crates/dagon-cli/page/index.html", "crates/dagon-cli/page/assets/index-x.js"):
            self.assertEqual(self.git("check-ignore", "-q", "--no-index", name).returncode, 0, name)

    def test_page_py_only_builds(self):
        self.assertFalse(hasattr(page, "check"))
        self.assertFalse(hasattr(page, "SOURCE"))


class PageKey(unittest.TestCase):
    def test_the_page_embeds_its_key_through_the_shared_helper(self):
        build = (ROOT / "crates" / "dagon-wasm" / "build.rs").read_text(encoding="utf-8")
        self.assertIn("dagon_mcu_build::embed_pubkey()", build)


if __name__ == "__main__":
    unittest.main()
