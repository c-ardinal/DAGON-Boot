"""stage0_size.py: the limit comes from the MCU definition and an image is
measured by what its loadable segments put into flash."""
import struct
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
