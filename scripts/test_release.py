"""Checks of scripts/release.py: every release build builds the page first
and refuses to make a dagon without it.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import unittest
from unittest import mock

import release


class ReleaseBuild(unittest.TestCase):
    def test_the_page_is_built_before_dagon_and_must_be_embedded(self):
        calls = []
        with mock.patch.object(release, "run", lambda cmd, env=None: calls.append((cmd, env))):
            release.build("x86_64-unknown-linux-gnu")
        self.assertEqual(len(calls), 2, calls)
        (page, _), (cargo, env) = calls
        self.assertEqual([str(c) for c in page[-2:]], [str(release.ROOT / "scripts" / "page.py"), "build"])
        self.assertEqual(cargo[:3], ["cargo", "build", "--release"])
        self.assertIn("--locked", cargo)
        self.assertEqual(env["DAGON_REQUIRE_PAGE"], "1")
        self.assertNotIn("DAGON_WEB_DIST", env)


if __name__ == "__main__":
    unittest.main()
