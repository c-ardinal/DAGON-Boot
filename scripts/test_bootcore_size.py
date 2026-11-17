"""bootcore_size.py: the budget comes from the MCU definition's boot core
slots, an image is measured as the signer flattens it, and one over its
slot fails."""
import pathlib
import struct
import tempfile
import tomllib
import unittest

import bootcore_size
import stage0_size
from test_stage0_size import elf


def loaded(data):
    """An ELF32 image of one loadable segment holding `data`."""
    head = bytearray(elf([(1, 0x1000_0000, len(data))]))
    struct.pack_into("<I", head, 52 + 4, len(head))
    return bytes(head) + data


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

    def test_the_catalog_names_come_from_the_events_page(self):
        names = bootcore_size.catalog_names()
        # A machine, a state, a kind and a transition method of the page.
        for n in ("sequence", "WatchdogArmed", "trial_started", "pending_set"):
            self.assertIn(n, names)
        self.assertTrue(all(len(n) >= bootcore_size.NAME_MIN for n in names))

    def test_an_image_that_links_a_catalog_name_fails(self):
        mcu = bootcore_size.bootcore_mcus()[0]
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "boot"
            path.write_bytes(loaded(bytes(64)))
            self.assertEqual(bootcore_size.check(path, mcu), [])
            path.write_bytes(loaded(b"xxtrial_started".ljust(64, b"\0")))
            self.assertEqual(bootcore_size.check(path, mcu),
                             [f"{mcu} boot core: links event catalog names trial_started"])

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

    def test_a_package_check_finding_fails_the_image(self):
        with tempfile.TemporaryDirectory() as d:
            pkg = pathlib.Path(d)
            self.assertEqual(bootcore_size.package_check(pkg, pkg / "e", pkg, "x"), [])
            (pkg / bootcore_size.PACKAGE_CHECK).write_text(
                "import sys\nprint('vector A is DefaultHandler')\nsys.exit(1)\n", encoding="utf-8")
            self.assertEqual(bootcore_size.package_check(pkg, pkg / "e", pkg, "x"),
                             ["x: vector A is DefaultHandler"])
            (pkg / bootcore_size.PACKAGE_CHECK).write_text("", encoding="utf-8")
            self.assertEqual(bootcore_size.package_check(pkg, pkg / "e", pkg, "x"), [])


def symtab_elf(syms):
    """An ELF32 image holding only a symbol table of `syms` (name, value)."""
    strtab = b"\0" + b"".join(n.encode() + b"\0" for n, _ in syms)
    entries, at = b"\0" * 16, 1
    for n, v in syms:
        entries += struct.pack("<IIIBBH", at, v, 0, 0, 0, 1)
        at += len(n) + 1
    body = 52
    shoff = body + len(strtab) + len(entries)
    head = bytearray(52)
    head[:6] = b"\x7fELF\x01\x01"
    struct.pack_into("<I", head, 0x20, shoff)
    struct.pack_into("<HHH", head, 0x2E, 40, 3, 0)
    secs = struct.pack("<10I", *[0] * 10)
    secs += struct.pack("<10I", 0, 3, 0, 0, body, len(strtab), 0, 0, 1, 0)
    secs += struct.pack("<10I", 0, 2, 0, 0, body + len(strtab), len(entries), 1, 0, 4, 16)
    return bytes(head) + strtab + entries + secs


class F401Vectors(unittest.TestCase):
    """The STM32F401RE boot core's image check: the link's I2C event and
    error vectors are its own handlers, never DefaultHandler."""

    def check(self, syms, board):
        import importlib.util
        path = stage0_size.MCU / "stm32f401re" / "bootcore" / bootcore_size.PACKAGE_CHECK
        spec = importlib.util.spec_from_file_location("f401_image_check", path)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        with tempfile.TemporaryDirectory() as d:
            if board:
                (pathlib.Path(d) / "board.toml").write_text(board, encoding="utf-8")
            return mod.findings(symtab_elf(syms), pathlib.Path(d))

    BOARD = '[[bus]]\nid = "link"\nkind = "i2c"\ncontroller = "i2c3"\ndagon = true\n'

    def test_the_link_vectors_must_not_be_the_default_handler(self):
        default = [("DefaultHandler_", 0x101), ("DefaultHandler", 0x101)]
        self.assertEqual(
            self.check(default + [("I2C3_EV", 0x101), ("I2C3_ER", 0x201)], self.BOARD),
            ["vector I2C3_EV is DefaultHandler: a wake-up taken as an exception hangs the boot core"])
        self.assertEqual(
            self.check(default + [("I2C3_EV", 0x301), ("I2C3_ER", 0x201)], self.BOARD), [])
        # No board, or no DAGON I2C bus: no link to wake on.
        self.assertEqual(self.check(default, None), [])


if __name__ == "__main__":
    unittest.main()
