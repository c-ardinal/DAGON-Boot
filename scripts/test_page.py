"""Checks of scripts/page.py: the page's source hash covers what its
WebAssembly embeds, the definitions under targets/ included.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import unittest
from unittest import mock

import page

ROOT = page.ROOT


class PageSources(unittest.TestCase):
    def test_every_embedded_definition_file_is_a_source(self):
        sources = set(page.source_files())
        embedded = page.embedded_files()
        self.assertIn(ROOT / "targets" / "mcu" / "esp32s3" / "partitions.csv", embedded)
        for f in embedded:
            self.assertTrue(f.is_file(), f)
            self.assertIn(f, sources)

    def test_a_definition_change_makes_the_page_stale(self):
        target = ROOT / "targets" / "boards" / "nucleo-f401re" / "dagon-board.toml"
        before = page.source_hash()
        real = type(target).read_bytes

        def read_bytes(p):
            data = real(p)
            return data + b"\n# changed\n" if p == target else data

        with mock.patch.object(type(target), "read_bytes", read_bytes):
            self.assertNotEqual(page.source_hash(), before)

    def test_paths_sort_with_regard_to_case_on_every_system(self):
        names = ["web/a.ts", "web/README.md", "web/Z.ts", "web/e2e/b.ts"]
        order = page.in_order(ROOT / n for n in names)
        self.assertEqual([p.relative_to(ROOT).as_posix() for p in order], ["web/README.md", "web/Z.ts", "web/a.ts", "web/e2e/b.ts"])

    def test_the_sources_are_listed_in_that_order(self):
        web = [p for p in page.source_files() if p.relative_to(ROOT).parts[0] == "web"]
        self.assertEqual(web, page.in_order(web))


if __name__ == "__main__":
    unittest.main()
