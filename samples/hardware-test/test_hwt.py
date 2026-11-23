"""hwt.py's own checks without hardware (`hwt.py selftest`), for CI."""
import contextlib
import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import hwt  # noqa: E402


class Selftest(unittest.TestCase):
    def test_selftest_passes(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(hwt.selftest(), 0)


if __name__ == "__main__":
    unittest.main()
