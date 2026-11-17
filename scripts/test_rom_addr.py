"""A ROM loader's I2C address has one source: its MCU definition's
`[rom_i2c] addr`, which the host and the simulator read from the loaded
definitions and the MCU packages' build generates for the relay.

Production Rust (tests and `#[cfg(test)]` modules left out) under
crates/*/src, the MCU packages (targets/mcu) and the board packages'
sources and templates writes no such address as a literal, hexadecimal
or decimal, in code, nor in hexadecimal in a comment that speaks of a ROM
or its loader; an event declaration (`Name = 0x.. {`) is a number of its
own, not an address.

    python -m unittest discover -s scripts -p "test_rom_addr.py"
"""
import re
import tomllib
import unittest

from rust_source import ROOT, code_only, rust_texts
from test_relay import is_test_path, production

DEFINITIONS = sorted((ROOT / "targets" / "mcu").glob("*/dagon-mcu.toml"))
HEX = re.compile(r"(?<![\w.])0[xX]([0-9A-Fa-f_]+)(?:u8|u16|u32|usize)?(?!\w)")
DEC = re.compile(r"(?<![\w.])([1-9][0-9_]*)(?:u8|u16|u32|usize)?(?![\w.])")
EVENT = re.compile(r"\b[A-Z]\w*\s*=\s*0[xX][0-9A-Fa-f_]+\s*\{")
ABOUT_ROM = re.compile(r"\bROM\b|\bloader\b", re.IGNORECASE)
COMMENT = re.compile(r"//+!?(.*)")


def rom_addrs():
    """Every `[rom_i2c] addr` of the built-in MCU definitions."""
    out = set()
    for p in DEFINITIONS:
        rom = tomllib.loads(p.read_text(encoding="utf-8")).get("rom_i2c")
        if rom:
            out.add(rom["addr"])
    return out


def in_scope(path):
    """crates/*/src, the MCU packages and the board templates, tests left out."""
    parts = path.split("/")
    if is_test_path(path) or parts[-1].startswith("tests_"):
        return False
    if parts[0] == "crates":
        return len(parts) > 2 and parts[2] == "src"
    if parts[:2] == ["targets", "mcu"]:
        return True
    return parts[:2] == ["targets", "boards"] and ("template" in parts or "src" in parts)


def hits(text, addrs, decimal=False):
    """The hexadecimal (and with `decimal`, decimal) literals of `text`
    whose value is one of `addrs`."""
    out = [m.group(0) for m in HEX.finditer(text)
           if int(m.group(1).replace("_", ""), 16) in addrs]
    if decimal:
        out += [m.group(0) for m in DEC.finditer(text)
                if int(m.group(1).replace("_", "")) in addrs]
    return out


def problems(texts, addrs):
    out = []
    for path, text in sorted(texts.items()):
        if not in_scope(path):
            continue
        code = EVENT.sub(" ", production(code_only(text)))
        for lit in hits(code, addrs, decimal=True):
            out.append(f"{path}: a ROM loader address written as a literal: {lit}")
        prod_text = production(text)
        for m in COMMENT.finditer(prod_text):
            if ABOUT_ROM.search(m.group(1)) and (lits := hits(m.group(1), addrs)):
                out.append(f"{path}: a comment restates a ROM loader address: {lits[0]}")
    return out


class RomAddr(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()
        cls.addrs = rom_addrs()

    def planted(self, path, extra):
        """What planting `extra` in `path` adds to what the tree already holds."""
        before = set(problems(self.texts, self.addrs))
        after = problems({**self.texts, path: self.texts.get(path, "") + "\n" + extra}, self.addrs)
        return [p for p in after if p not in before]

    def test_the_definitions_give_an_address(self):
        self.assertTrue(self.addrs)

    def test_the_tree_holds(self):
        self.assertEqual(problems(self.texts, self.addrs), [])

    def test_a_literal_address_in_production_fails(self):
        a = min(self.addrs)
        for path, extra in [
            ("crates/dagon-romlink/src/lib.rs", f"pub const ROM_ADDR: u8 = {a:#04x};\n"),
            ("crates/dagon-relay/src/lib.rs", f"fn f() -> u8 {{ {a:#04X}u8 }}\n"),
            ("crates/dagon-sim/src/rom.rs", f"/// The loader answers at {a:#x}.\nfn g() {{}}\n"),
            ("targets/mcu/rp2040/src/relay.rs", f"const A: u8 = {a:#04x};\n"),
            ("crates/dagon-host/src/rom.rs", f"const A: u8 = {a};\n"),
            ("targets/boards/nucleo-f401re/src/lib.rs", f"const A: u8 = {a:#04x};\n"),
            ("targets/boards/waveshare-rp2040-touch-lcd-1.28/template/src/main.rs",
             f"const A: u8 = {a:#04x};\n"),
        ]:
            with self.subTest(path=path):
                self.assertTrue(self.planted(path, extra))

    def test_tests_events_and_other_numbers_pass(self):
        a = min(self.addrs)
        for path, extra in [
            ("crates/dagon-romlink/src/lib.rs", f"#[cfg(test)]\nmod t {{\n    const A: u8 = {a:#04x};\n}}\n"),
            ("crates/dagon-host/tests/sim.rs", f"const A: u8 = {a:#04x};\n"),
            ("crates/dagon-topology/src/defs/tests_x.rs", f"const A: u8 = {a:#04x};\n"),
            ("crates/dagon-agent/src/lib.rs", f"emits Thing = {a:#04x} {{ slot: u8 }}\n"),
            ("crates/dagon-relay/src/lib.rs", f"const B: u8 = {a + 1:#04x};\n// a byte {a:#04x}\n"),
            ("crates/dagon-relay/src/lib.rs", f"const C: f32 = {a}.5;\nconst D: u8 = 1{a};\n"),
            ("samples/f401-app/src/seg.rs", f"const A: u8 = {a:#04x};\n"),
        ]:
            with self.subTest(path=path):
                self.assertEqual(self.planted(path, extra), [])


if __name__ == "__main__":
    unittest.main()
