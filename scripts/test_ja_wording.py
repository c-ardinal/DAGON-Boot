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
# The audited seed (lex1-5 and the Console's lexicon) plus できる, one entry per verb and
# conjugation class; a change here is a lexicon change.
INVENTORY = {"retired": 3, "joint": 3, "native": 211}
PLANTED = {
    "更新できます": "できる",
    "使います": "使う",
    "書き込みます": "書き込む",
    "子機": "親機/子機/孫機",
    "フォルダを作ります": "作る",
    "表示しながら": "ながら",
    "名前": "名前",
    "すべて": "すべて",
    "確かめます": "確かめる",
}


def kinds(path, text):
    return [
        (h.kind, h.word)
        for h in ja_wording.hits_in(path, text, RULES, ja_wording.is_polite(path, ["site/src/content/docs/*"]))
    ]


def lemmas(path, text):
    return [h.lemma for h in ja_wording.hits_in(path, text, RULES, False)]


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

    def test_plain_style_counts_sentences_not_fragments(self):
        self.assertEqual(kinds(DOC, "| `a` | 説明（既定値。）。 |"), [])
        self.assertEqual([k for k, _ in kinds(DOC, "説明（既定値。）。")], ["plain"])
        self.assertEqual(kinds(DOC, "- 説明（既定値。）。"), [])
        self.assertEqual(kinds(DOC, "**遷移**: 前後の状態。"), [])
        self.assertEqual([k for k, _ in kinds(DOC, "- 設定は有効だ。")], ["plain"])
        self.assertEqual(kinds(DOC, "設定です（既定値。範囲は（1から8。）です）。"), [])

    def test_every_conjugation_is_caught(self):
        verbs = {
            "使う": ["使って", "使った", "使わない", "使います", "使えば"],
            "書く": ["書いて", "書いた", "書かない", "書きます", "書けば"],
            "送る": ["送って", "送った", "送らない", "送ります", "送れば"],
            "確かめる": ["確かめて", "確かめた", "確かめない", "確かめます", "確かめれば"],
            "出る": ["出て", "出た", "出ない", "出ます", "出れば"],
            "できる": ["更新できて", "更新できた", "更新できない", "更新できます", "更新できれば"]
            + ["更新出来て", "更新出来た", "更新出来ない", "更新出来ます", "更新出来れば"],
            "新しい": ["新しい", "新しく", "新しかった", "新しければ", "新しさ"],
        }
        for lemma, forms in verbs.items():
            for form in forms:
                self.assertIn(lemma, lemmas(CODE, "// " + form), form)
        self.assertEqual(lemmas(CODE, "// 出力します"), [])

    def test_a_parenthetical_spans_lines(self):
        self.assertEqual(kinds(DOC, "説明です（\n補足だ。\n以上）。"), [])
        self.assertEqual([k for k, _ in kinds(DOC, "説明（\n補足です。\n以上）だ。")], ["plain"])

    def test_a_sentence_and_a_list_item_span_lines(self):
        self.assertEqual([k for k, _ in kinds(DOC, "設定は\n有効だ。")], ["plain"])
        self.assertEqual(kinds(DOC, "- 項目です。\n  補足の値。"), [])
        self.assertEqual([k for k, _ in kinds(DOC, "- 項目です。\n\n本文の値。")], ["plain"])

    def test_a_chain_of_no_spans_a_line_break(self):
        self.assertIn("no-chain", [k for k, _ in kinds(DOC, "ノードの\n設定の一覧の項目")])

    def test_the_longest_match_wins(self):
        self.assertEqual(lemmas(CODE, "// やり直します"), ["やり直す"])
        self.assertEqual(lemmas(CODE, "// 食い違います"), ["食い違う"])
        self.assertEqual(lemmas(CODE, "// 置き換えます"), ["置き換える"])

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

    ENTRY = '[[allow]]\npath = "a.rs"\ntext = "「使い方」"\nkind = "native"\nword = "使い"\n'

    def test_an_allow_entry_covers_only_its_own_finding(self):
        allow = ja_wording.load_allow(self.write(self.ENTRY + 'reason = "quoted title"\n'))
        hits = ja_wording.hits_in("a.rs", "// 「使い方」を参照して使います\n// 「使い方」\n", RULES, False)
        left = ja_wording.unallowed("a.rs", hits, allow)
        self.assertEqual([(h.line, h.word) for h in left], [(1, "使い")])
        self.assertEqual(left[0].start, len("// 「使い方」を参照して"))

    def test_an_entry_without_a_reason_fails(self):
        with self.assertRaises(ValueError):
            ja_wording.load_allow(self.write(self.ENTRY + 'reason = " "\n'))
        with self.assertRaises(ValueError):
            ja_wording.load_allow(self.write('[[allow]]\npath = "a.rs"\ntext = "x"\nreason = "r"\n'))
        bad = self.write(self.ENTRY)
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

    def test_the_seeded_inventory(self):
        data = ja_wording.load_scopes(ja_wording.LEXICON)
        self.assertEqual({k: len(data[k]) for k in ("retired", "joint", "native")}, INVENTORY)
        for text, lemma in PLANTED.items():
            self.assertEqual(lemmas(CODE, "// " + text)[:1], [lemma], text)

    def test_every_entry_wins_over_the_entries_inside_it(self):
        for rule in RULES:
            for form in rule.lemma.split("（")[0].split("/"):
                own = [m.group() for m in rule.regex.finditer("// " + form) if m.group()]
                if own:
                    hits = ja_wording.hits_in(CODE, "// " + form, RULES, False)
                    longest = max(len(h.word) for h in hits)
                    self.assertGreaterEqual(longest, max(map(len, own)), form)

    def test_the_alpha_notice_passes_word_for_word(self):
        allow = ja_wording.load_allow(ja_wording.ALLOW)
        for path, lines in NOTICE_FILES.items():
            text = "\n".join(lines)
            hits = ja_wording.hits_in(path, text, RULES, True)
            self.assertEqual(ja_wording.unallowed(path, hits, allow), [], path)


if __name__ == "__main__":
    unittest.main()
