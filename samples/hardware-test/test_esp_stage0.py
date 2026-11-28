"""Host tests of esp_stage0.py (no board): the reading of the ROM text
output, the values and the verdict, and the commands it runs."""

import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

import esp_stage0 as e

# The good node's output in Espressif's QEMU (the CI job stage0-qemu,
# esp32s3_stage0_measuring_build_in_qemu).
GOOD = """\
measure: rom 80 MHz, reset 1, path 1
measure: cycles 1148022, systimer 458788, rtc 4290
measure: verify 824669 cycles in 1, hash 85765 cycles for 1200 bytes
measure: earlier 0, changed 1920, 50000000..50001dfc
measure: store 00000000 00000000 00000000 00000000
measure: store 00000000 00000000 00000000 00000000
core: started, cell 00000000 00000000
core: asks for the app
measure: rom 80 MHz, reset 3, path 2
measure: cycles 4902717, systimer 1960807, rtc 18375
measure: verify 787867 cycles in 1, hash 3880775 cycles for 66080 bytes
measure: earlier 1, changed 0, 00000000..00000000
measure: store 00000000 00000000 00000000 00000000
measure: store 00000000 00000005 00000000 00000000
measure-1: rom 80 MHz, reset 1, path 1
measure-1: cycles 1148022, systimer 458788, rtc 4290
measure-1: verify 824669 cycles in 1, hash 85765 cycles for 1200 bytes
measure-1: earlier 0, changed 1920, 50000000..50001dfc
measure-1: store 00000000 00000000 00000000 00000000
measure-1: store 00000000 00000000 00000000 00000000
app: running from flash
""".splitlines()

BAD = """\
core: started, cell 00000000 00000000
core: asks for the app
core: started, cell b95aa52d b95aa52d
core: recovery mode, refusal 2
""".splitlines()


class Reading(unittest.TestCase):
    def test_each_record_is_read_whole_in_order(self):
        rs = e.records(GOOD)
        self.assertEqual([(r["tag"], r["path"]) for r in rs],
                         [("measure", 1), ("measure", 2), ("measure-1", 1)])
        self.assertEqual(rs[0]["changed"], 1920)
        self.assertEqual(rs[0]["changed_last"], 0x5000_1DFC)
        self.assertEqual(rs[1]["hash_bytes"], 66080)
        self.assertEqual(len(rs[1]["store"]), 8)

    def test_store_words_in_use_are_named(self):
        self.assertEqual(e.used_stores(e.records(GOOD)[1]), [5])

    def test_clocks_come_from_the_system_timer(self):
        a = e.analyse(e.records(GOOD)[1])
        # 4,902,717 cycles over 1,960,807 ticks of 16 MHz: 40 MHz in QEMU.
        self.assertEqual(round(a["cpu_hz"], -3), 40_006_000)
        self.assertEqual(a["decision_ms"], 122.55)
        self.assertGreater(a["hash_full_slot_ms"], a["hash_ms"])

    def test_a_timer_that_does_not_count_gives_no_clock(self):
        self.assertIn("why", e.analyse({"systimer": 0}))


class Verdict(unittest.TestCase):
    def test_the_good_run_passes(self):
        v = e.verdict(25_392, {"good": GOOD, "bad-signature": BAD})
        self.assertEqual(v, {"verdict": "PASS", "why": []})

    def test_a_stage0_over_its_budget_fails(self):
        v = e.verdict(30_721, {"good": GOOD, "bad-signature": BAD})
        self.assertEqual(v["verdict"], "FAIL")

    def test_an_app_started_with_a_broken_signature_fails(self):
        v = e.verdict(25_392, {"good": GOOD, "bad-signature": GOOD})
        self.assertIn("the app with a broken signature was not refused", v["why"])

    def test_no_output_fails(self):
        v = e.verdict(25_392, {"good": [], "bad-signature": []})
        self.assertEqual(v["verdict"], "FAIL")


class Commands(unittest.TestCase):
    def test_flash_size_is_read_from_board_info(self):
        self.assertEqual(e.flash_size("Chip type: esp32s3\nFlash size: 16MB\n"), 16 << 20)
        with self.assertRaises(SystemExit):
            e.flash_size("no size")

    def test_images_are_written_raw(self):
        self.assertEqual(e.espflash("write-bin", "COM7", "0x0", "good.bin"),
                         ["espflash", "write-bin", "--chip", "esp32s3", "--port", "COM7",
                          "--non-interactive", "0x0", "good.bin"])

    def test_prepare_names_the_measuring_build_to_the_image_writer(self):
        steps = e.cargo_steps("app.bin", Path("images"))
        self.assertIn("qemu-layout,measure", steps[0][1])
        env = steps[2][2]
        self.assertIn("measure", env["DAGON_STAGE0_ELF"])
        self.assertTrue(env["DAGON_STAGE0_HW_APP"].endswith("app.bin"))

    def test_a_run_backs_up_first_and_ends_with_the_backup(self):
        with tempfile.TemporaryDirectory() as d:
            images = Path(d) / "images"
            images.mkdir()
            for n in ("good.bin", "bad-signature.bin"):
                (images / n).write_bytes(b"\xff")
            (images / "stage0.bin").write_bytes(b"\0" * 25_392)
            ran = []

            def run(cmd, cwd=None, env=None, capture=False):
                ran.append(cmd[1] + " " + cmd[-1])
                if cmd[1] == "read-flash":
                    Path(cmd[-1]).write_bytes(b"\xff")
                return "Flash size: 16MB" if capture else ""

            heard = iter([GOOD, GOOD, BAD])
            a = SimpleNamespace(out=d, port="COM7", manual_reset=False, listen_seconds=0, app_seconds=0)
            rc = e.measure(a, run=run, hear=lambda *x, **k: next(heard))
            self.assertEqual(rc, 0)
            backup = str(Path(d) / "backup.bin")
            self.assertTrue(ran[0].startswith("board-info"))
            self.assertTrue(ran[1].startswith("read-flash"))
            self.assertEqual(ran[-1], "write-bin " + backup)


if __name__ == "__main__":
    unittest.main()
