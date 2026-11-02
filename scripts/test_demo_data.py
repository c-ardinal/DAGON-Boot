"""Checks of scripts/demo-data.py that need no firmware build.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import importlib.util
import pathlib
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("demo_data", HERE / "demo-data.py")
demo = importlib.util.module_from_spec(spec)
spec.loader.exec_module(demo)

PUB = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"


class DemoData(unittest.TestCase):
    def test_matching_key_pair_passes(self):
        out = f"public_key: {PUB}\nkey_id:     0011\n"
        demo.check_key_pair(out, PUB.upper() + "\n")

    def test_mismatched_key_pair_stops_the_run(self):
        out = f"public_key: {'00' * 32}\n"
        with self.assertRaises(SystemExit):
            demo.check_key_pair(out, PUB)
        with self.assertRaises(SystemExit):
            demo.check_key_pair("no key here", PUB)

    def test_output_folder_is_rebuilt_but_the_build_cache_kept(self):
        with tempfile.TemporaryDirectory() as d:
            out = pathlib.Path(d)
            (out / "build" / "x").mkdir(parents=True)
            (out / "build" / "x" / "cache").write_text("keep")
            (out / "images" / "0.9.0").mkdir(parents=True)
            (out / "images" / "0.9.0" / "old.img").write_text("stale")
            (out / "stale.dgb").write_text("stale")
            demo.clean_out(out)
            self.assertEqual(sorted(p.name for p in out.iterdir()), ["build"])
            self.assertTrue((out / "build" / "x" / "cache").exists())

    def test_cli_builds_outside_the_repository(self):
        with tempfile.TemporaryDirectory() as d:
            demo.CLI_TARGET = pathlib.Path(d) / "build" / "cli"
            cmd = [str(c) for c in demo.dagon_cmd("key", "pub")]
            target = pathlib.Path(cmd[cmd.index("--target-dir") + 1])
            self.assertNotIn(demo.ROOT, target.parents)


if __name__ == "__main__":
    unittest.main()
