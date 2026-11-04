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

    def test_an_image_over_its_region_is_reported(self):
        at, size = 0x1000_0000, 0x4000
        img = elf([(1, at, 0x4001)])
        self.assertGreater(stage0_size.flash_bytes(img, at, size), size)
        with self.assertRaises(ValueError):
            stage0_size.flash_bytes(b"MZ" + bytes(60), at, size)


if __name__ == "__main__":
    unittest.main()
