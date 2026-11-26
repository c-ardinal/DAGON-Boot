"""sample_size.py: the hard limit comes from the MCU definition's app
slots, each sample app has a budget within it, and an image or its static
RAM over the budget fails."""
import pathlib
import struct
import tempfile
import unittest

import sample_size
import stage0_size
from test_bootcore_size import loaded


def with_sections(sections):
    """An ELF32 image with the allocated, writable `sections` (name, size)
    and one section that is neither."""
    table = b"\0.text\0" + b"".join(n.encode() + b"\0" for n, _ in sections)
    heads = [struct.pack("<IIIIII", 1, 1, 0x6, 0, 0, 64)]
    at = len(b"\0.text\0")
    for name, size in sections:
        heads.append(struct.pack("<IIIIII", at, 8, 0x3, 0, 0, size))
        at += len(name) + 1
    strtab = len(heads)
    heads.append(struct.pack("<IIIIII", 0, 3, 0, 0, 52, len(table)))
    head = bytearray(52)
    head[:6] = b"\x7fELF\x01\x01"
    shoff = 52 + len(table)
    struct.pack_into("<I", head, 0x20, shoff)
    struct.pack_into("<HHH", head, 0x2E, 40, len(heads), strtab)
    return bytes(head) + table + b"".join(h.ljust(40, b"\0") for h in heads)


class SampleSize(unittest.TestCase):
    def test_the_slot_is_the_smallest_app_slot_of_the_definition(self):
        self.assertEqual(sample_size.slot_bytes("stm32f401re"), 0x1_FE00)
        self.assertEqual(sample_size.slot_bytes("rp2040"), 0xF_FE00)
        self.assertEqual(sample_size.slot_bytes("esp32s3"), 0x30_0000)
        self.assertFalse(sample_size.in_partition("stm32f401re"))
        self.assertTrue(sample_size.in_partition("esp32s3"))

    def test_every_sample_app_has_a_budget_within_its_slot(self):
        apps = {p.relative_to(sample_size.SAMPLES).as_posix() for p in sample_size.packages()}
        # Every sample that builds firmware: a package with a cargo config
        # naming its target.
        built = {p.parent.parent.relative_to(sample_size.SAMPLES).as_posix()
                 for p in sample_size.SAMPLES.glob("**/.cargo/config.toml")
                 if "target" not in p.relative_to(sample_size.SAMPLES).parts[:-2]}
        self.assertEqual(apps, built)
        for pkg in sample_size.packages():
            b = sample_size.read_budget(pkg)
            slot = sample_size.slot_bytes(b["mcu"])
            self.assertLessEqual(b["image"]["max_bytes"], slot, pkg)
            self.assertGreater(b["ram"]["max_bytes"], 0, pkg)
            self.assertTrue(sample_size.bin_of(pkg) and sample_size.target_of(pkg), pkg)

    def test_an_image_over_its_slot_or_its_budget_fails(self):
        v = sample_size.verdicts
        self.assertEqual(v("app", 100, 200, 150), [])
        self.assertEqual(v("app", 150, 200, 150), [])
        self.assertEqual(v("app", 151, 200, 150), ["app: 151 bytes, over its budget of 150"])
        self.assertEqual(v("app", 201, 200, 200),
                         ["app: 201 bytes, over its 200-byte slot",
                          "app: 201 bytes, over its budget of 200"])
        self.assertEqual(v("app", 1, 200, 201),
                         ["app: its budget of 201 bytes is over its 200-byte slot"])
        self.assertEqual(v("app", 1, 200, None), ["app: its budget.toml has no [image] max_bytes"])

    def test_a_partition_image_is_bounded_from_its_segments(self):
        # Header, two segments padded to words with their headers, the
        # two alignment segments' headers and padding, checksum and hash.
        self.assertEqual(sample_size.partition_bytes([5, 8]),
                         24 + 8 + 8 + 8 * 4 + 48 + 2 * 0x1_0000)
        self.assertEqual(sample_size.segments(loaded(b"\0" * 12)), [12])

    def test_static_ram_is_the_writable_sections_without_those_skipped(self):
        elf = with_sections([(".data", 100), (".bss", 300), (".stack", 5000)])
        self.assertEqual(sample_size.ram_sections(elf),
                         [(".data", 100), (".bss", 300), (".stack", 5000)])
        self.assertEqual(sum(s for _, s in sample_size.ram_sections(elf)),
                         stage0_size.ram_bytes(elf))
        check = sample_size.check_ram
        self.assertEqual(check(elf, {"ram": {"max_bytes": 400, "skip": [".stack"]}}, "app"), [])
        self.assertEqual(check(elf, {"ram": {"max_bytes": 399, "skip": [".stack"]}}, "app"),
                         ["app: 400 bytes of static RAM, over its 399"])
        self.assertEqual(check(elf, {"ram": {"max_bytes": 400}}, "app"),
                         ["app: 5400 bytes of static RAM, over its 400"])
        self.assertEqual(check(elf, {}, "app"), ["app: its budget.toml has no [ram] max_bytes"])
        # A skipped name the image no longer has would hide nothing, but is stale.
        self.assertEqual(check(elf, {"ram": {"max_bytes": 5400, "skip": [".gone"]}}, "app"),
                         ["app: its budget.toml skips .gone, which the image does not have"])

    def test_a_built_image_is_checked_against_its_packages_budget(self):
        pkg = sample_size.SAMPLES / "f401-app"
        with tempfile.TemporaryDirectory() as d:
            small = pathlib.Path(d) / "small"
            small.write_bytes(loaded(b"\0" * 64))
            # No section table: no static RAM.
            self.assertEqual(sample_size.check(small, pkg), [])
            big = pathlib.Path(d) / "big"
            big.write_bytes(loaded(b"\0" * (0x1_FE00 + 4)))
            why = sample_size.check(big, pkg)
            self.assertIn("samples/f401-app: 130564 bytes, over its 130560-byte slot", why)

    def test_only_the_targets_rustup_adds_are_listed(self):
        self.assertEqual(sample_size.rustup_targets(),
                         ["thumbv6m-none-eabi", "thumbv7em-none-eabihf"])


if __name__ == "__main__":
    unittest.main()
