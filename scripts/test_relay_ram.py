"""relay_ram.py: the budget comes from the MCU definition's `[relay]`, the
firmware is measured by its `.bss` and `.data` sections, and one over its
budget fails.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import pathlib
import struct
import tempfile
import tomllib
import unittest

import relay_ram
import stage0_size


def elf(sections: list[tuple[str, int]]) -> bytes:
    """An ELF32 little-endian file with only section headers: (name, size)
    each, after a null section and with the name table last."""
    names = b"\0" + b"".join(n.encode() + b"\0" for n, _ in sections) + b".shstrtab\0"
    head_len, entry = 52, 40
    names_at = head_len
    shoff = names_at + len(names)
    headers = [struct.pack("<10I", *[0] * 10)]
    at = 1
    for n, size in sections:
        headers.append(struct.pack("<10I", at, 8, 3, 0x2000_0000, 0, size, 0, 0, 4, 0))
        at += len(n) + 1
    headers.append(struct.pack("<10I", at, 3, 0, 0, names_at, len(names), 0, 0, 1, 0))
    count = len(headers)
    head = (b"\x7fELF\x01\x01\x01" + b"\0" * 9
            + struct.pack("<HHIIIIIHHHHHH", 2, 40, 1, 0, 0, shoff, 0,
                          head_len, 0, 0, entry, count, count - 1))
    return head + names + b"".join(headers)


class RelayRam(unittest.TestCase):
    def test_the_budget_is_the_definitions_relay_ram_bytes(self):
        for mcu in relay_ram.mcus():
            want = stage0_size.definition(mcu).get("relay", {}).get("ram_bytes")
            self.assertEqual(relay_ram.budget(mcu), want, mcu)

    def test_every_mcu_that_relays_has_a_budget_and_a_firmware_to_build(self):
        for mcu in relay_ram.relay_mcus():
            self.assertIsNotNone(relay_ram.budget(mcu), mcu)
            pkg = relay_ram.package(mcu)
            self.assertTrue((pkg / "examples" / "relay.rs").is_file(), mcu)
            self.assertTrue(relay_ram.target_of(mcu), mcu)
            manifest = tomllib.loads((pkg / "Cargo.toml").read_text(encoding="utf-8"))
            example = [e for e in manifest.get("example", []) if e["name"] == "relay"]
            self.assertEqual(len(example), 1, mcu)

    def test_an_mcu_that_does_not_relay_has_no_budget(self):
        for mcu in set(relay_ram.mcus()) - set(relay_ram.relay_mcus()):
            self.assertIsNone(relay_ram.budget(mcu), mcu)

    def test_the_static_ram_is_bss_and_data(self):
        image = elf([(".text", 0x4000), (".data", 0x188), (".bss", 0x4550), (".uninit", 0x400)])
        self.assertEqual(relay_ram.ram_bytes(image), 0x188 + 0x4550)
        with self.assertRaises(ValueError):
            relay_ram.ram_bytes(b"\x7fELF\x02\x01" + b"\0" * 58)

    def test_at_the_budget_passes_and_one_byte_over_fails(self):
        mcu = relay_ram.relay_mcus()[0]
        size = relay_ram.budget(mcu)
        self.assertIsNone(relay_ram.verdict(size, size))
        self.assertEqual(relay_ram.verdict(size + 1, size),
                         f"{size + 1} bytes of static RAM, over its {size}-byte budget")
        with tempfile.TemporaryDirectory() as d:
            for used, failed in ((size, []), (size + 1, ["over"])):
                path = pathlib.Path(d) / "relay"
                path.write_bytes(elf([(".data", 8), (".bss", used - 8)]))
                why = relay_ram.check(path, mcu)
                self.assertEqual(len(why), len(failed), why)
                self.assertTrue(all("over its" in w for w in why), why)

    def test_an_mcu_without_a_budget_is_refused(self):
        mcu = next(m for m in relay_ram.mcus() if relay_ram.budget(m) is None)
        self.assertEqual(relay_ram.check(pathlib.Path("unused"), mcu),
                         [f"{mcu} relay firmware: the definition has no [relay] ram_bytes"])


if __name__ == "__main__":
    unittest.main()
