"""Checks of scripts/ci_crates.py: the list CI tests matches the repository.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import pathlib
import tempfile
import unittest

import ci_crates

ROOT = ci_crates.ROOT


class CiCrates(unittest.TestCase):
    def test_every_test_outside_the_root_workspace_is_in_a_listed_package(self):
        listed = {ROOT / d for d in ci_crates.test_dirs()}
        for f in ROOT.rglob("*.rs"):
            rel = f.relative_to(ROOT).parts
            if ci_crates.SKIP_DIRS.intersection(rel) or rel[0] == "crates":
                continue
            if not ci_crates.TEST_MARK.search(f.read_text(encoding="utf-8", errors="replace")):
                continue
            owner = next(p for p in f.parents if (p / "Cargo.toml").exists())
            self.assertIn(owner, listed, f"{f} has tests but its package is not tested in CI")

    def test_root_workspace_crates_are_not_listed(self):
        self.assertFalse([d for d in ci_crates.test_dirs() if d.startswith("crates/")])

    def test_a_nested_package_does_not_make_its_parent_tested(self):
        with tempfile.TemporaryDirectory() as t:
            parent = pathlib.Path(t)
            child = parent / "child"
            (child / "src").mkdir(parents=True)
            (child / "src" / "main.rs").write_text("#[test]\nfn t() {}\n")
            self.assertTrue(ci_crates.has_host_tests(child, []))
            self.assertFalse(ci_crates.has_host_tests(parent, [child]))


if __name__ == "__main__":
    unittest.main()
