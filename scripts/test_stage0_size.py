"""stage0_size.py: the limit comes from the MCU definition, an image is
measured by what its loadable segments put into flash, and its parts by
the linker map against the package's budget."""
import pathlib
import struct
import tempfile
import unittest

import stage0_size


def elf(segments):
    """A minimal ELF32 little-endian image with program headers
    `segments` as (type, paddr, filesz)."""
    head = bytearray(52)
    head[:6] = b"\x7fELF\x01\x01"
    struct.pack_into("<I", head, 0x1C, 52)
    struct.pack_into("<HH", head, 0x2A, 32, len(segments))
    for t, paddr, filesz in segments:
        head += struct.pack("<IIIIIIII", t, 0, paddr, paddr, filesz, filesz, 5, 4)
    return bytes(head)


# A recorded LLD map, cut down: boot2, the vector table's first word, code
# of three crates (a trait impl's name keeps the legacy escapes) with two
# gaps of alignment padding, an anonymous constant, the flash image of
# .data and .bss in RAM.
OBJ = "C:/t/release/deps/dagon_rp2040_stage0-5d.dagon_rp2040_stage0.4b-cgu.0.rcgu.o"
MAP = f"""     VMA      LMA     Size Align Out     In      Symbol
10000000 10000000      100     1 .boot2
10000000 10000000      100     1         {OBJ}:(.boot2)
10000000 10000000      100     1                 dagon_rp2040_stage0::BOOT2::h762cd4229ad83ba8
10000100 10000100        0     1 __stage0_image_end = LOADADDR(.data) + SIZEOF(.data)
10000100 10000100        4     4 .vector_table
10000100 10000100        4     1         LONG(_stack_start & 0xFFFFFFF8)
10000104 10000104       40     4 .text
10000104 10000104       1e     2         {OBJ}:(.text._ZN10dagon_core8bootcore6decide17hffd9E)
10000104 10000104        0     1                 $t
10000105 10000105       1e     1                 dagon_core::bootcore::decide::hffd978865a2a096a
10000124 10000124        a     2         {OBJ}:(.text._ZN73_$LT$sha2..Sha512$u20$as$u20$digest..Update$GT$6update17h08aE)
10000125 10000125        a     1                 _$LT$sha2..Sha512$u20$as$u20$digest..Update$GT$::update::h08a3dd8cff30c039
10000130 10000130       14     4         {OBJ}:(.text._ZN13dagon_ed255194edwards3add17h0E)
10000131 10000131       14     1                 dagon_ed25519::edwards::add::h0000000000000000
10000144 10000144        b     1 .rodata
10000144 10000144        b     1         {OBJ}:(.rodata..Lanon.e858081fe811408900a0f1ab3150e3df.0)
20000000 1000014f        1     4 .data
20000000 1000014f        1     1         {OBJ}:(.data.FLAG)
20000004 20000004       20     4 .bss
20000004 20000004       20     4         {OBJ}:(.bss.BUF)
"""
BUDGET = """[[part]]
name = "boot2"
match = [".boot2"]
max_bytes = 256
[[part]]
name = "verifier"
match = ["dagon_ed25519"]
max_bytes = 32
[[part]]
name = "sha2"
match = ["sha2"]
max_bytes = 16
[[part]]
name = "mcu-package"
match = ["dagon_rp2040_stage0", ".vector_table"]
max_bytes = 8
[[part]]
name = "other"
max_bytes = 64
"""


def budget(text=BUDGET):
    with tempfile.TemporaryDirectory() as d:
        path = pathlib.Path(d) / "budget.toml"
        path.write_text(text, encoding="utf-8")
        return stage0_size.read_budget(path)


class Stage0Parts(unittest.TestCase):
    def used(self, parts=None):
        parts = parts or budget()
        items = stage0_size.map_items(MAP, 0x1000_0000, 0x4000)
        return stage0_size.attribute(items, parts), parts

    def test_each_section_goes_to_the_first_part_it_fits(self):
        used, _ = self.used()
        # boot2's symbol is the stage0 crate's, but its section is .boot2,
        # and boot2 comes first; dagon_core matches no part here.
        self.assertEqual(used, {"boot2": 0x100, "verifier": 0x14, "sha2": 0xA,
                                "mcu-package": 4, "other": 0x1E + 2 + 2 + 0xB + 1})

    def test_the_parts_add_up_to_the_image_and_each_stays_within_its_max(self):
        used, parts = self.used()
        text, why = stage0_size.parts_report(used, parts, 0x150)
        self.assertEqual(why, [])
        self.assertIn("total", text)
        _, why = stage0_size.parts_report(used, parts, 0x151)
        self.assertEqual(why, ["the parts add up to 336 bytes, the image has 337"])
        tight = budget(BUDGET.replace("max_bytes = 32", "max_bytes = 19"))
        used, parts = self.used(tight)
        _, why = stage0_size.parts_report(used, parts, 0x150)
        self.assertEqual(why, ["part verifier: 20 bytes, over its 19"])

    def test_a_trait_impls_name_is_read_as_its_rust_path(self):
        self.assertEqual(
            stage0_size.demangled("_$LT$sha2..Sha512$u20$as$u20$digest..Update$GT$::update"),
            "<sha2::Sha512 as digest::Update>::update")
        self.assertTrue(stage0_size.fits("sha2", "_$LT$sha2..Sha512$u20$as$u20$x$GT$"))
        self.assertFalse(stage0_size.fits("sha2", "dagon_format::Sha256::new"))

    def test_a_budget_ends_with_other_and_names_each_part_once(self):
        with self.assertRaises(ValueError):
            budget(BUDGET.replace('name = "other"', 'name = "rest"'))
        with self.assertRaises(ValueError):
            budget(BUDGET.replace('name = "sha2"', 'name = "verifier"'))
        with self.assertRaises(ValueError):
            budget(BUDGET.replace('match = ["sha2"]\n', ""))

    def test_every_stage0_package_has_a_budget(self):
        for mcu in stage0_size.stage0_mcus():
            parts = stage0_size.read_budget(stage0_size.MCU / mcu / "stage0" / "budget.toml")
            at, size = stage0_size.region(mcu)
            # The maxima together stay within the hard limit.
            self.assertLessEqual(sum(p["max_bytes"] for p in parts), size - stage0_size.margin(size))


class Stage0Size(unittest.TestCase):
    def test_the_region_is_the_definitions(self):
        self.assertEqual(stage0_size.region("rp2040"), (0x1000_0000, 0x4000))
        self.assertIn("rp2040", stage0_size.stage0_mcus())

    def test_flash_bytes_count_loadable_segments_in_the_region(self):
        at, size = 0x1000_0000, 0x4000
        img = elf([
            (1, at, 0x100),           # boot2
            (1, at + 0x100, 0x3000),  # code and rodata
            (1, at + 0x3100, 0x40),   # the initial data image
            (1, 0x2000_0000, 0x400),  # RAM: not flash
            (6, at, 0x9999),          # not PT_LOAD
        ])
        self.assertEqual(stage0_size.flash_bytes(img, at, size), 0x3140)

    def test_every_board_of_a_stage0_mcu_is_built_besides_the_boardless_one(self):
        # From the board definitions, not a list: a board added for the MCU
        # is measured without touching the script.
        self.assertEqual(
            stage0_size.configurations("rp2040"),
            [None, "waveshare-rp2040-touch-lcd-1.28"],
        )
        self.assertEqual(stage0_size.configurations("stm32f401re"), [None, "nucleo-f401re"])

    def test_the_boot_folder_names_the_board_for_the_build(self):
        import tomllib
        meta = tomllib.loads(stage0_size.boot_folder_toml("rp2040", "waveshare-rp2040-touch-lcd-1.28"))
        boot = meta["package"]["metadata"]["dagon"]["boot"]
        self.assertEqual(boot, {"node": "size", "mcu": "rp2040",
                                "board": "waveshare-rp2040-touch-lcd-1.28"})

    def test_a_stage0_must_leave_the_margin_free(self):
        size = 0x4000
        margin = stage0_size.margin(size)
        self.assertEqual(margin, 1024)
        self.assertIsNone(stage0_size.verdict(size - margin, size))
        self.assertIn("margin", stage0_size.verdict(size - margin + 1, size))
        self.assertIn("over its", stage0_size.verdict(size + 1, size))

    def test_an_image_over_its_region_is_reported(self):
        at, size = 0x1000_0000, 0x4000
        img = elf([(1, at, 0x4001)])
        self.assertGreater(stage0_size.flash_bytes(img, at, size), size)
        with self.assertRaises(ValueError):
            stage0_size.flash_bytes(b"MZ" + bytes(60), at, size)


if __name__ == "__main__":
    unittest.main()
