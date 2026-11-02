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

# Repository and home stand-ins that neither contain nor sit in the test folders.
ELSEWHERE = pathlib.Path(tempfile.gettempdir()).resolve() / "dagon-test-elsewhere"

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

    def test_a_marked_folder_loses_only_the_scripts_outputs(self):
        with tempfile.TemporaryDirectory() as d:
            out = pathlib.Path(d).resolve()
            (out / demo.MARKER).write_text("")
            (out / "build" / "x").mkdir(parents=True)
            (out / "build" / "x" / "cache").write_text("keep")
            (out / "images" / "0.9.0").mkdir(parents=True)
            (out / "images" / "0.9.0" / "old.img").write_text("stale")
            (out / "release-1.0.dgb").write_text("stale")
            (out / "SHA256SUMS").write_text("stale")
            (out / "notes.txt").write_text("the user's")
            demo.check_out(out, root=ELSEWHERE / "repo", home=ELSEWHERE / "home")
            demo.clean_out(out)
            left = sorted(p.name for p in out.iterdir())
            self.assertEqual(left, [demo.MARKER, "build", "notes.txt"])
            self.assertTrue((out / "build" / "x" / "cache").exists())

    def test_a_folder_not_made_by_the_script_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            out = pathlib.Path(d).resolve()
            (out / "Documents").mkdir()
            with self.assertRaises(SystemExit):
                demo.check_out(out, root=ELSEWHERE / "repo", home=ELSEWHERE / "home")
            self.assertTrue((out / "Documents").exists())
            demo.check_out(out / "new", root=ELSEWHERE / "repo", home=ELSEWHERE / "home")
            (out / "empty").mkdir()
            demo.check_out(out / "empty", root=ELSEWHERE / "repo", home=ELSEWHERE / "home")

    def test_the_repository_its_parents_and_home_are_refused(self):
        root = demo.ROOT
        for bad in (root, root / "out", root.parent, pathlib.Path(root.anchor)):
            with self.assertRaises(SystemExit, msg=str(bad)):
                demo.check_out(bad.resolve())
        with tempfile.TemporaryDirectory() as d:
            home = pathlib.Path(d).resolve() / "home"
            home.mkdir()
            for bad in (home, home.parent):
                with self.assertRaises(SystemExit, msg=str(bad)):
                    demo.check_out(bad, root=pathlib.Path(d) / "repo", home=home)

    def test_cli_builds_outside_the_repository(self):
        with tempfile.TemporaryDirectory() as d:
            demo.CLI_TARGET = pathlib.Path(d) / "build" / "cli"
            cmd = [str(c) for c in demo.dagon_cmd("key", "pub")]
            target = pathlib.Path(cmd[cmd.index("--target-dir") + 1])
            self.assertNotIn(demo.ROOT, target.parents)


if __name__ == "__main__":
    unittest.main()
