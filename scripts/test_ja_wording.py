"""Tests for scripts/ja_wording.py: each rule catches its planted case and leaves the right form alone."""

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import ja_wording  # noqa: E402
from test_markers import NOTICE_FILES  # noqa: E402

RULES = ja_wording.load_lexicon(ja_wording.LEXICON)
DOC = "site/src/content/docs/how-to/a.md"
CODE = "crates/x/src/lib.rs"


def kinds(path, text):
    return [
        (h.kind, h.word)
        for h in ja_wording.hits_in(path, text, RULES, ja_wording.is_polite(path, ["site/src/content/docs/*"]))
    ]


class Rules(unittest.TestCase):
    def test_native_word_is_caught_and_compound_is_not(self):
        self.assertIn(("native", "使い"), kinds(CODE, "// 設定を使います"))
        self.assertEqual(kinds(CODE, "// 設定を使用します"), [])

    def test_native_word_inside_a_kanji_compound_is_not_caught(self):
        self.assertEqual(kinds(CODE, "// 印刷を開始します"), [])

    def test_retired_terms_are_caught(self):
        self.assertIn(("retired", "書き込み"), kinds(CODE, "// ノードに書き込みます"))
        self.assertIn(("retired", "親機"), kinds(CODE, "// 親機に接続します"))
        self.assertIn(("retired", "作り"), kinds(CODE, "// フォルダを作ります"))
        self.assertEqual(kinds(CODE, "// 書込中 書込済 書込不可"), [])

    def test_three_chained_no_are_caught_and_two_are_not(self):
        self.assertIn("no-chain", [k for k, _ in kinds(CODE, "// ノードの設定の一覧の項目")])
        self.assertEqual(kinds(CODE, "// ノードの設定一覧"), [])
        self.assertEqual(kinds(CODE, "// ノードの設定の一覧"), [])

    def test_nagara_and_tekara_are_caught(self):
        self.assertIn(("joint", "ながら"), kinds(CODE, "// 表示しながら待機します"))
        self.assertIn(("joint", "てから"), kinds(CODE, "// 確認してから実行します"))
        self.assertIn(("joint", "でから"), kinds(CODE, "// 読込んでから実行します"))

    def test_plain_style_is_caught_only_in_docs(self):
        self.assertEqual([k for k, _ in kinds(DOC, "設定は有効だ。")], ["plain"])
        self.assertEqual(kinds(DOC, "設定は有効です。実行します（詳細は[参照](x.md)）。"), [])
        self.assertEqual(kinds(CODE, "// 設定は有効だ。"), [])

    def test_a_bold_lead_in_is_a_heading_not_a_sentence(self):
        self.assertEqual(kinds(DOC, "**stage0の上限。** 上限を記録します。"), [])

    def test_a_sentence_whose_parenthetical_opened_on_an_earlier_line_is_left_alone(self):
        self.assertEqual(kinds(DOC, "- 配列です（`a`、昇順、\n  合計で16バイトまで）。"), [])

    def test_code_in_markdown_is_left_alone(self):
        self.assertEqual(kinds(DOC, "`使う`を指定します。\n```\n使う\n```\n"), [])

    def test_line_numbers_follow_the_text(self):
        hits = ja_wording.hits_in(CODE, "a\n// 使います\n", RULES, False)
        self.assertEqual([h.line for h in hits], [2])


class AllowList(unittest.TestCase):
    def tmp(self):
        d = tempfile.TemporaryDirectory()
        self.addCleanup(d.cleanup)
        return d.name

    def write(self, text):
        p = Path(self.tmp()) / "allow.toml"
        p.write_text(text, encoding="utf-8")
        return p

    def test_an_allow_listed_line_passes(self):
        allow = ja_wording.load_allow(
            self.write('[[allow]]\npath = "a.rs"\ntext = "使います"\nreason = "quoted name"\n')
        )
        hits = ja_wording.hits_in("a.rs", "// 使います\n// 送ります\n", RULES, False)
        self.assertEqual([h.word for h in ja_wording.unallowed("a.rs", hits, allow)], ["送り"])

    def test_an_entry_without_a_reason_fails(self):
        with self.assertRaises(ValueError):
            ja_wording.load_allow(self.write('[[allow]]\npath = "a.rs"\ntext = "使います"\nreason = " "\n'))
        bad = self.write('[[allow]]\npath = "a.rs"\ntext = "x"\n')
        d = self.tmp()
        (Path(d) / "a.rs").write_text("// 設定\n", encoding="utf-8")
        self.assertEqual(ja_wording.main(["--root", d, "--allow", str(bad), "a.rs"]), 1)

    def test_a_file_with_a_hit_fails_and_a_clean_file_passes(self):
        d = self.tmp()
        (Path(d) / "a.rs").write_text("// 使います\n", encoding="utf-8")
        (Path(d) / "b.rs").write_text("// 使用します\n", encoding="utf-8")
        self.assertEqual(ja_wording.main(["--root", d, "a.rs"]), 1)
        self.assertEqual(ja_wording.main(["--root", d, "b.rs"]), 0)


class Data(unittest.TestCase):
    def test_the_repository_data_loads(self):
        self.assertTrue(RULES)
        ja_wording.load_allow(ja_wording.ALLOW)
        areas = ja_wording.load_scopes(ja_wording.SCOPES)["area"]
        self.assertEqual(len({a["name"] for a in areas}), len(areas))

    def test_the_alpha_notice_passes_word_for_word(self):
        allow = ja_wording.load_allow(ja_wording.ALLOW)
        for path, lines in NOTICE_FILES.items():
            text = "\n".join(lines)
            hits = ja_wording.hits_in(path, text, RULES, True)
            self.assertEqual(ja_wording.unallowed(path, hits, allow), [], path)


if __name__ == "__main__":
    unittest.main()
