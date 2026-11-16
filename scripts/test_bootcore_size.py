"""bootcore_size.py: the budget comes from the MCU definition's boot core
slots, an image is measured as the signer flattens it, and one over its
slot fails."""
import pathlib
import tempfile
import tomllib
import unittest

import bootcore_size
import stage0_size
from test_stage0_size import elf


class BootcoreSize(unittest.TestCase):
    def test_the_budget_is_the_smallest_boot_core_slot_of_the_definition(self):
        for mcu in bootcore_size.mcus():
            slots = [s["capacity"] for s in stage0_size.definition(mcu).get("slot_layout", [])
                     if s["target"] == "bootcore"]
            self.assertEqual(bootcore_size.budget(mcu), min(slots) if slots else None, mcu)
        # The F401's slot A (sector 4) is half its slot B (sector 7): A binds.
        self.assertEqual(bootcore_size.budget("stm32f401re"), 0xFE00)
        self.assertEqual(bootcore_size.budget("rp2040"), 0x1_FE00)
        # A vendor second stage loads the ESP32-S3's images.
        self.assertIsNone(bootcore_size.budget("esp32s3"))

    def test_an_mcu_without_a_boot_core_slot_has_no_boot_core_to_build(self):
        none = [m for m in bootcore_size.mcus() if bootcore_size.budget(m) is None]
        self.assertEqual(sorted(set(bootcore_size.mcus()) - set(none)),
                         bootcore_size.bootcore_mcus())
        for mcu in none:
            self.assertEqual(bootcore_size.check(pathlib.Path("unused"), mcu),
                             [f"{mcu} boot core: the definition has no boot core slot"])

    def test_every_mcu_with_a_boot_core_slot_has_a_package_to_build(self):
        for mcu in bootcore_size.bootcore_mcus():
            pkg = bootcore_size.package(mcu)
            self.assertTrue((pkg / "examples" / "size.rs").is_file(), mcu)
            meta = tomllib.loads((pkg / "Cargo.toml").read_text(encoding="utf-8"))
            self.assertEqual(meta["package"]["metadata"]["dagon"]["bootcore"], mcu)
            self.assertTrue(bootcore_size.target_of(mcu))
            for board in stage0_size.configurations(mcu):
                self.assertTrue(bootcore_size.folder_of(mcu, board).is_dir(), (mcu, board))

    def test_the_package_builds_as_a_nodes_boot_crate_does(self):
        # The check builds in the package, a node's boot crate with the
        # cargo config `dagon gen` renders from the template: their target
        # and flags (the F401's compact SHA-256 rounds among them) are one.
        for mcu in bootcore_size.bootcore_mcus():
            pkg = bootcore_size.package(mcu)
            own, node = (tomllib.loads((d / ".cargo" / "config.toml").read_text(encoding="utf-8"))
                         for d in (pkg, pkg / "template"))
            for table in ("build", "target"):
                self.assertEqual(own[table], node[table], (mcu, table))

    def test_an_image_is_its_loadable_segments_from_lowest_to_highest(self):
        at = 0x1000_8200
        img = elf([
            (1, at, 0x1000),            # vector table and code
            (1, at + 0x1100, 0x40),     # the initial data image, after a gap
            (1, 0x2000_0000, 0),        # .bss: nothing in the file
            (6, at, 0x9999),            # not PT_LOAD
        ])
        self.assertEqual(bootcore_size.image_bytes(img), 0x1140)
        with self.assertRaises(ValueError):
            bootcore_size.image_bytes(b"MZ" + bytes(60))
        with self.assertRaises(ValueError):
            bootcore_size.image_bytes(elf([]))

    def test_an_image_over_its_slot_fails_and_one_that_fills_it_passes(self):
        mcu = bootcore_size.bootcore_mcus()[0]
        size = bootcore_size.budget(mcu)
        self.assertIsNone(bootcore_size.verdict(size, size))
        self.assertIn("over its", bootcore_size.verdict(size + 1, size))
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "boot"
            path.write_bytes(elf([(1, 0x1000_0000, size)]))
            self.assertEqual(bootcore_size.check(path, mcu), [])
            path.write_bytes(elf([(1, 0x1000_0000, size + 1)]))
            self.assertEqual(bootcore_size.check(path, mcu),
                             [f"{mcu} boot core: {size + 1} bytes, over its {size}-byte slot"])


if __name__ == "__main__":
    unittest.main()
