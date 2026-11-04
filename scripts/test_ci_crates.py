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
        members = set(ci_crates.root_member_dirs())
        for f in ROOT.rglob("*.rs"):
            if ci_crates.SKIP_DIRS.intersection(f.relative_to(ROOT).parts):
                continue
            if not ci_crates.TEST_MARK.search(f.read_text(encoding="utf-8", errors="replace")):
                continue
            owner = next(p for p in f.parents if (p / "Cargo.toml").exists())
            if owner.resolve() in members:
                continue
            self.assertIn(owner, listed, f"{f} has tests but its package is not tested in CI")

    def test_root_workspace_members_are_not_listed(self):
        members = set(ci_crates.root_member_dirs())
        self.assertTrue(members)
        self.assertFalse([d for d in ci_crates.test_dirs() if (ROOT / d).resolve() in members])

    def test_a_nested_package_does_not_make_its_parent_tested(self):
        with tempfile.TemporaryDirectory() as t:
            parent = pathlib.Path(t)
            child = parent / "child"
            (child / "src").mkdir(parents=True)
            (child / "src" / "main.rs").write_text("#[test]\nfn t() {}\n")
            self.assertTrue(ci_crates.has_host_tests(child, []))
            self.assertFalse(ci_crates.has_host_tests(parent, [child]))


    def test_firmware_lints_cover_every_mcu_package_and_boot_image(self):
        runs = ci_crates.firmware_lints()
        mcu = ROOT / "targets" / "mcu"
        packages = {p.parent for p in mcu.glob("*/dagon-mcu.toml")}
        # Boot images: the binaries inside an MCU package's folder (its
        # `template` is the starting point of a user's app, not DAGON's).
        packages |= {m.parent.parent for m in mcu.glob("*/*/src/main.rs")
                     if m.parent.parent.name != "template"}
        want = {p.relative_to(ROOT).as_posix() for p in packages}
        self.assertEqual({r["dir"] for r in runs}, want)
        for r in runs:
            self.assertIn(r["toolchain"], ("stable", "esp"), r)
            if not r["target"]:
                # The package names its target in its own cargo config.
                config = ROOT / r["dir"] / ".cargo" / "config.toml"
                self.assertIn("target", config.read_text(encoding="utf-8"), r)


if __name__ == "__main__":
    unittest.main()
