"""measure.py without hardware: the Cargo.toml switch, the ELF symbol
reader, the probe and clock parsers and the verdicts; and that the names it
relies on (features, symbols, the build folders, clock.rs) are still the
firmware's and the CLI's, so a rename there fails here and not on the
owner's bench."""

import re
import struct
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))

import measure  # noqa: E402


def elf32(symbols):
    """A minimal 32-bit little-endian ELF: a null section, .symtab, .strtab."""
    strtab = b"\0"
    syms = [b"\0" * 16]
    for name, value in symbols.items():
        syms.append(struct.pack("<IIIBBH", len(strtab), value, 4, 0x11, 0, 1))
        strtab += name.encode() + b"\0"
    symtab = b"".join(syms)
    ehsize, shentsize = 52, 40
    sym_at = ehsize
    str_at = sym_at + len(symtab)
    shoff = str_at + len(strtab)
    sections = [
        b"\0" * shentsize,
        struct.pack("<IIIIIIIIII", 0, 2, 0, 0, sym_at, len(symtab), 2, 1, 4, 16),
        struct.pack("<IIIIIIIIII", 0, 3, 0, 0, str_at, len(strtab), 0, 0, 1, 0),
    ]
    head = b"\x7fELF" + bytes([1, 1, 1]) + b"\0" * 9
    head += struct.pack("<HHIIIIIHHHHHH", 2, 40, 1, 0, 0, shoff, 0, ehsize, 0, 0, shentsize, 3, 0)
    return head + symtab + strtab + b"".join(sections)


class Switch(unittest.TestCase):
    def test_default_made_when_features_lack_it(self):
        text = '[package]\nname = "x"\n\n[features]\nslot-b = []\n\n[dependencies]\n'
        out = measure.with_default_feature(text, "verify-time")
        self.assertIn('[features]\ndefault = ["verify-time"]\nslot-b = []\n', out)
        self.assertEqual(tomllib.loads(out)["features"]["default"], ["verify-time"])

    def test_added_to_an_existing_default_once(self):
        text = '[features]\ndefault = ["a"]\n'
        out = measure.with_default_feature(text, "b/listen-time")
        self.assertEqual(tomllib.loads(out)["features"]["default"], ["a", "b/listen-time"])
        self.assertEqual(measure.with_default_feature(out, "b/listen-time"), out)

    def test_features_table_made_when_missing(self):
        out = measure.with_default_feature('[package]\nname = "x"', "f")
        self.assertEqual(tomllib.loads(out)["features"]["default"], ["f"])

    def test_original_bytes_back_after_an_error(self):
        with tempfile.TemporaryDirectory() as d:
            f = Path(d) / "Cargo.toml"
            original = b'[features]\r\nslot-b = []\r\n'
            f.write_bytes(original)
            with self.assertRaises(RuntimeError):
                with measure.Switched(f, "verify-time"):
                    self.assertIn(b"verify-time", f.read_bytes())
                    raise RuntimeError
            self.assertEqual(f.read_bytes(), original)
            self.assertFalse((Path(d) / ("Cargo.toml" + measure.BACKUP)).exists())

    def test_restore_puts_back_what_a_stopped_run_left(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)
            (p / "dagon.toml").write_text("[dagon]\nversion = '1'\n")
            boot = p / "boot" / "seg"
            boot.mkdir(parents=True)
            (boot / "Cargo.toml").write_text("switched")
            (boot / ("Cargo.toml" + measure.BACKUP)).write_text("original")
            self.assertEqual(measure.restore(p), [str(boot / "Cargo.toml")])
            self.assertEqual((boot / "Cargo.toml").read_text(), "original")


class Readers(unittest.TestCase):
    def test_symbols_by_name(self):
        data = elf32({"DAGON_VERIFY_CYCLES": 0x2000_0010, "other": 4, "DAGON_VERIFY_COUNT": 0x2000_0014})
        self.assertEqual(
            measure.elf_symbols(data, ["DAGON_VERIFY_CYCLES", "DAGON_VERIFY_COUNT", "absent"]),
            {"DAGON_VERIFY_CYCLES": 0x2000_0010, "DAGON_VERIFY_COUNT": 0x2000_0014},
        )

    def test_not_an_elf(self):
        with self.assertRaises(ValueError):
            measure.elf_symbols(b"MZ\0\0", ["x"])

    def test_probe_words(self):
        self.assertEqual(measure.parse_words("0x20000010: 0x02faf080 0x00000001\n"), [0x02FAF080, 1])
        self.assertEqual(measure.parse_words("02faf080 00000001"), [0x02FAF080, 1])

    def test_clock_as_the_build_writes_it(self):
        src = (REPO / "targets/mcu/build/src/lib.rs").read_text(encoding="utf-8")
        for line in ["pub const SOURCE: &str = {source:?};", "pub const OUT_HZ: u32 = {out_hz};"]:
            self.assertIn(line, src)
        text = 'pub const SOURCE: &str = "crystal";\npub const OUT_HZ: u32 = 125000000;\n'
        self.assertEqual(measure.parse_clock(text), ("crystal", 125_000_000))


class Verdicts(unittest.TestCase):
    def test_stage0(self):
        r = measure.stage0_verdict(48_750_000, 1, 125_000_000)
        self.assertEqual((r["verdict"], r["verify_s"], r["blink_code"]), ("PASS", 0.39, 4))
        self.assertEqual(measure.stage0_verdict(0, 0, 125_000_000)["verdict"], "FAIL")

    def test_blinks(self):
        self.assertEqual(measure.blink_verdict(4, 125_000_000)["verify_s_at_most"], 0.4)
        self.assertEqual(measure.blink_verdict(255, 6_500_000)["verdict"], "FAIL")

    def test_listen(self):
        self.assertEqual(measure.listen_verdict(60_000, 50)["verdict"], "PASS")
        self.assertEqual(measure.listen_verdict(60_001, 50)["verdict"], "FAIL")
        self.assertEqual(measure.listen_verdict(0, 50)["verdict"], "FAIL")


class StillTheFirmwares(unittest.TestCase):
    """The names measure.py relies on, read from this repository."""

    def mcus_with(self, part):
        for d in sorted((REPO / "targets/mcu").glob("*/dagon-mcu.toml")):
            defn = tomllib.loads(d.read_text(encoding="utf-8"))
            if any(p["name"] == part for p in defn.get("part", [])):
                yield defn["id"], d.parent

    def test_part_folders_are_where_dagon_flash_builds(self):
        flash = (REPO / "crates/dagon-cli/src/flash.rs").read_text(encoding="utf-8")
        self.assertIn('cfg.base.join("out/flash/target").join(&part.name)', flash)
        mcus = {m["part"] for m in measure.MEASUREMENTS.values()}
        for part in mcus:
            self.assertTrue(list(self.mcus_with(part)), part)

    def test_every_stage0_has_the_measuring_build(self):
        m = measure.MEASUREMENTS["stage0-verify"]
        stage0s = list(self.mcus_with("stage0"))
        self.assertTrue(stage0s)
        for mcu, folder in stage0s:
            cargo = tomllib.loads((folder / "stage0/Cargo.toml").read_text(encoding="utf-8"))
            self.assertIn(m["feature"], cargo["features"], mcu)

    def test_listen_time_boot_cores(self):
        m = measure.MEASUREMENTS["listen-time"]
        for mcu in m["mcus"]:
            cargo = tomllib.loads((REPO / f"targets/mcu/{mcu}/bootcore/Cargo.toml").read_text(encoding="utf-8"))
            lib, feature = measure.feature_of(m, mcu).split("/")
            self.assertEqual(cargo["package"]["name"], lib)
            self.assertIn(feature, cargo["features"])

    def test_symbols_are_kept_by_name(self):
        sources = "".join(
            p.read_text(encoding="utf-8") for p in (REPO / "targets/mcu").rglob("*.rs") if "target" not in p.parts
        )
        for m in measure.MEASUREMENTS.values():
            for s in m["symbols"]:
                self.assertRegex(sources, rf"#\[no_mangle\]\s*(pub )?static {s}:", s)

    def test_boot_crate_template_has_a_features_table(self):
        for mcu in measure.MEASUREMENTS["listen-time"]["mcus"]:
            tpl = (REPO / f"targets/mcu/{mcu}/bootcore/template/Cargo.toml.tpl").read_text(encoding="utf-8")
            self.assertTrue(re.search(r"(?m)^\[features\]$", tpl))


if __name__ == "__main__":
    unittest.main()
