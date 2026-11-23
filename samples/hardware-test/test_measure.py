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

    def test_decide(self):
        self.assertEqual(measure.decide_verdict(412_000)["decide_s"], 0.412)
        self.assertEqual(measure.decide_verdict(0)["verdict"], "FAIL")

    def test_groups(self):
        self.assertEqual(measure.groups_us("5 2"), 52_000)
        self.assertEqual(measure.groups_us("1 2 10 5"), 1_205_000)
        self.assertEqual(measure.groups_us("10"), 0)
        for bad in ("", "0", "11", "5 x"):
            with self.assertRaises(ValueError):
                measure.groups_us(bad)

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

    def test_every_boot_core_has_the_measuring_builds(self):
        cores = list(self.mcus_with("bootcore"))
        self.assertTrue(cores)
        for name in ["listen-time", "decide-time"]:
            m = measure.MEASUREMENTS[name]
            for mcu, folder in cores:
                cargo = tomllib.loads((folder / "bootcore/Cargo.toml").read_text(encoding="utf-8"))
                lib, feature = measure.feature_of(m, mcu).split("/")
                self.assertEqual(cargo["package"]["name"], lib)
                self.assertIn(feature, cargo["features"], mcu)
                # Kept compiling by the firmware lint.
                self.assertIn(feature, cargo["package"]["metadata"]["dagon"]["lint"]["features"], mcu)

    def test_symbols_are_kept_by_name(self):
        sources = "".join(
            p.read_text(encoding="utf-8") for p in (REPO / "targets/mcu").rglob("*.rs") if "target" not in p.parts
        )
        for m in measure.MEASUREMENTS.values():
            for s in m["symbols"]:
                self.assertRegex(sources, rf"#\[no_mangle\]\s*(pub )?static {s}:", s)

    def test_boot_crate_template_has_a_features_table(self):
        for mcu, folder in self.mcus_with("bootcore"):
            tpl = (folder / "bootcore/template/Cargo.toml.tpl").read_text(encoding="utf-8")
            self.assertTrue(re.search(r"(?m)^\[features\]$", tpl))


class Runs(unittest.TestCase):
    """measure() with dagon flash, the probe, the wait and the question faked."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def project(self, writer, mcu="stm32f401re"):
        src = self.root / "src"
        (src / "targets/mcu" / mcu / "stage0").mkdir(parents=True)
        self.cargo = src / "targets/mcu" / mcu / "stage0/Cargo.toml"
        self.cargo.write_text('[package]\nname = "s"\n\n[features]\nverify-time = []\n')
        proj = self.root / "proj"
        (proj / "boot/n").mkdir(parents=True)
        (proj / "dagon.toml").write_text('[dagon]\npath = "../src"\n')
        w = ", ".join(f'{k} = "{v}"' for k, v in writer.items())
        (proj / "boot/n/mcu.toml").write_text(f'id = "{mcu}"\nwriter = {{ {w} }}\n')
        rel = proj / "out/flash/target/stage0/thumb/release"
        (rel / "build/s-1/out").mkdir(parents=True)
        (rel / "build/s-1/out/clock.rs").write_text('pub const SOURCE: &str = "crystal";\npub const OUT_HZ: u32 = 125000000;\n')
        (rel / "stage0").write_bytes(elf32({"DAGON_VERIFY_CYCLES": 0x20000008, "DAGON_VERIFY_COUNT": 0x20000004}))
        return proj

    def args(self, proj, **kw):
        base = dict(project=str(proj), dagon="dagon", node="n", key="k", probe=None, chip=None,
                    drive=None, out=str(self.root / "rec"))
        base.update(kw)
        return measure.argparse.Namespace(**base)

    def run_it(self, a, flash_fails=(), words=None, name="stage0-verify", answer="4"):
        self.flashes = []

        def flash(cmd, project):
            self.flashes.append((cmd, self.cargo.read_text()))
            if len(self.flashes) in flash_fails:
                raise SystemExit("dagon flash exited 1")

        self.reads = []

        def read(chip, probe, address, count=1):
            self.reads.append((chip, probe, address))
            return [(words or {}).get(address, 0)], ["probe-rs"]

        return measure.measure(a, name, flash=flash, read=read, wait=lambda s: None,
                               ask=lambda q: answer)

    def test_probe_rs_node_measured_and_written_back(self):
        proj = self.project({"kind": "probe-rs", "chip": "STM32F401RETx"})
        original = self.cargo.read_text()
        rc = self.run_it(self.args(proj, probe="0483:374b"), words={0x20000008: 48_750_000, 0x20000004: 1})
        self.assertEqual(rc, 0)
        (first, during), (second, after) = self.flashes
        self.assertIn('default = ["verify-time"]', during)
        self.assertEqual(after, original)
        self.assertEqual(first[-2:], ["--probe", "0483:374b"])
        self.assertEqual(self.reads[0][:2], ("STM32F401RETx", "0483:374b"))
        self.assertEqual(self.cargo.read_text(), original)

    def test_failed_measurement_and_failed_write_back_exit_2(self):
        proj = self.project({"kind": "probe-rs", "chip": "STM32F401RETx"})
        self.assertEqual(self.run_it(self.args(proj), flash_fails=(1, 2)), 2)

    def test_failed_measurement_with_the_normal_build_back_exits_1(self):
        proj = self.project({"kind": "probe-rs", "chip": "STM32F401RETx"})
        self.assertEqual(self.run_it(self.args(proj), flash_fails=(1,)), 1)

    def test_uf2_node_read_by_a_separate_probe(self):
        proj = self.project({"kind": "uf2-drive"}, mcu="rp2040")
        a = self.args(proj, probe="2e8a:000c", chip="RP2040", drive="E:")
        self.assertEqual(self.run_it(a, words={0x20000008: 48_750_000, 0x20000004: 1}), 0)
        for cmd, _ in self.flashes:
            self.assertNotIn("--probe", cmd)
            self.assertEqual(cmd[-2:], ["--drive", "E:"])
        self.assertEqual(self.reads[0][:2], ("RP2040", "2e8a:000c"))

    def test_uf2_node_without_a_probe_takes_the_blink_code(self):
        proj = self.project({"kind": "uf2-drive"}, mcu="rp2040")
        self.assertEqual(self.run_it(self.args(proj, drive="E:")), 0)
        self.assertEqual(self.reads, [])

    def test_boot_core_without_a_probe_takes_the_led_groups(self):
        proj = self.project({"kind": "uf2-drive"}, mcu="rp2040")
        boot = proj / "boot/n/Cargo.toml"
        boot.write_text('[package]\nname = "n"\n\n[features]\n')
        rel = proj / "out/flash/target/bootcore/thumb/release"
        rel.mkdir(parents=True)
        (rel / "bootcore").write_bytes(elf32({"DAGON_LISTEN_US": 0x20000010}))
        self.assertEqual(self.run_it(self.args(proj, drive="E:"), name="listen-time", answer="5 2"), 0)
        self.assertEqual(self.reads, [])
        self.assertIn(b'"listen_us": 52000', next((self.root / "rec").glob("*/result.json")).read_bytes())

    def test_a_probe_without_its_chip_is_refused_before_any_write(self):
        proj = self.project({"kind": "uf2-drive"}, mcu="rp2040")
        with self.assertRaises(SystemExit):
            self.run_it(self.args(proj, probe="2e8a:000c"))
        self.assertEqual(self.flashes, [])


if __name__ == "__main__":
    unittest.main()
