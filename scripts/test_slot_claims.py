"""What the user-facing text says about slots stays true to the design.

An MCU definition gives each image kind one slot or two, and the design
has single-slot layouts. This check fails on text that says otherwise:
that every image has two slots, that a kind always takes two, or that
single-slot layouts do not exist, that a write stopped part-way always
leaves the running version, and on the stage0 choice that tried
slots no log record names. It reads the docs (both languages, generated
pages included), every README, the site's translation catalogs, the CLI
help (`crates/dagon-cli/src/cli.rs`) and the Console's text
(`web/src/app/copy.ts`). It also keeps the limitations page's table of
what one slot cannot do in step with the image kinds a definition knows.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import os
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "site" / "src" / "content" / "docs"
SOURCES = [
    ROOT / "site" / "i18n",
    ROOT / "web" / "src" / "app" / "copy.ts",
    ROOT / "crates" / "dagon-cli" / "src" / "cli.rs",
]
# Build output and installed packages hold no text written here.
PRUNE = {".git", "node_modules", "target", "dist", "pkg", "vendor", ".astro"}

# Each entry: what the text claims, and the pattern. Line breaks in a
# paragraph read as spaces.
CLAIMS = [
    ("single-slot layouts do not exist",
     r"(?i)\bno (?:single|one)[- ]slot (?:layouts?|configurations?|nodes?|images?)\b"
     r"|(?i:\b(?:single|one)[- ]slot (?:layouts?|configurations?) (?:do(?:es)? not|don't|cannot|can't) (?:exist|be used|load)\b)"
     r"|スロットが[1一]つの(?:配置|構成|レイアウト|ノード|イメージ)は(?:ありません|無い|ない|ありえません|使えません)"
     r"|(?:単一|[1一]つの)スロット(?:の配置|の構成)?は(?:ありません|無い|ない|使えません)"),
    ("every image has two slots",
     r"(?i)\b(?:every|each|all)\b[^.,|。]{0,40}\b(?:images?|kinds?|MCUs?|nodes?)\b[^.,|。]{0,30}"
     r"\b(?:has|have|lives? in|takes?|uses?|gets?) (?:exactly )?(?:two|2|both) slots\b"
     r"|(?i:\blives? in two slots\b|\btwo for every MCU\b|\bthe two APP slots\b|\balways (?:has |have |takes |uses )?two slots\b)"
     r"|(?:すべて|どれ|どの)の?(?:イメージ|種類|マイコン|ノード)[^。|]{0,20}[もは][^。|]{0,10}(?:[2二]つのスロット|スロットを[2二]つ)"
     r"|[2二]つのスロットに置|(?:持つ|ある)マイコンでは(?:どれも|すべて)[2二]つ|[2二]つのAPPのスロット"),
    ("a stopped write always leaves the running version",
     r"(?i)\bstops? part-way, the (?:running|previous|old) version (?:stays|remains|keeps running)\b"
     r"|(?:途中で止まっても|切断すると)、?[^。、]{0,20}(?:動いているバージョンはそのまま|前のバージョンのまま)"),
    ("stage0 tries slots no record names",
     r"(?i)\btries the other slots on their own\b|\bslot A while (?:the log names none|empty)\b"
     r"|ほかのスロットを単独で試|ログがどれも示さないあいだのスロットA"),
]
COMPILED = [(what, re.compile(p)) for what, p in CLAIMS]
# A claim limited to images with two slots is true: these claims are left
# alone when the limit sits in the claim's own clause, or in a conditional
# clause attached to it (before it: "With two slots, ...", 「…なら、」; after
# it: "..., when ..."). A two-slot clause that is merely in the same sentence
# does not limit the claim. No two-slot wording makes the stage0 claim true,
# so it is always flagged.
QUALIFIABLE = {"single-slot layouts do not exist", "every image has two slots",
               "a stopped write always leaves the running version"}
TWO_SLOTS = re.compile(r"(?i)\btwo[- ]slots?\b|\b2 slots\b|スロット[がを][2二]つ|[2二]つのスロット")
SENTENCE_ENDS = ".。|"
CLAUSE_ENDS = ",;:、；："
# A clause that sets a condition: led by a conditional word in English, or
# ending in a conditional particle in Japanese.
CONDITION_BEFORE = re.compile(r"(?i)^(?:if|when|where|with|on|for|in)\b|(?:なら|では|ときは|とき|場合は|場合)$")
CONDITION_AFTER = re.compile(r"(?i)^(?:if|when|where|with|on|for|in)\b")


def last(flat, chars, lo, hi):
    """Index of the last of `chars` in flat[lo:hi], else lo - 1."""
    return max([flat.rfind(c, lo, hi) for c in chars] + [lo - 1])


def first(flat, chars, lo, hi):
    """Index of the first of `chars` in flat[lo:hi], else hi."""
    return min([i for i in (flat.find(c, lo, hi) for c in chars) if i >= 0] + [hi])


def qualified(flat, m):
    """Whether match `m` is limited to two slots by its own clause or an attached conditional one."""
    s_start = last(flat, SENTENCE_ENDS, 0, m.start()) + 1
    s_end = first(flat, SENTENCE_ENDS, m.end(), len(flat))
    c_start = last(flat, CLAUSE_ENDS, s_start, m.start()) + 1
    c_end = first(flat, CLAUSE_ENDS, m.end(), s_end)
    if TWO_SLOTS.search(flat[c_start:m.start()] + " " + flat[m.end():c_end]):
        return True
    if c_start > s_start:
        before = flat[last(flat, CLAUSE_ENDS, s_start, c_start - 1) + 1:c_start - 1].strip()
        if TWO_SLOTS.search(before) and CONDITION_BEFORE.search(before):
            return True
    if c_end < s_end:
        after = flat[c_end + 1:first(flat, CLAUSE_ENDS, c_end + 1, s_end)].strip()
        if TWO_SLOTS.search(after) and CONDITION_AFTER.search(after):
            return True
    return False


def texts():
    """Path (relative, `/`-separated) -> text of every file the check reads."""
    paths = []
    for top, dirs, names in os.walk(ROOT):
        dirs[:] = [d for d in dirs if d not in PRUNE]
        paths += [Path(top) / n for n in names if n.startswith("README") and n.endswith(".md")]
    paths += [p for p in DOCS.rglob("*") if p.suffix in (".md", ".mdx")]
    for s in SOURCES:
        paths += sorted(s.glob("*.toml")) if s.is_dir() else [s]
    return {p.relative_to(ROOT).as_posix(): p.read_text(encoding="utf-8") for p in paths}


def claims(text):
    """(line, what, words) of each claim in `text`."""
    flat = text.replace("\n", " ")
    for what, rx in COMPILED:
        for m in rx.finditer(flat):
            if what in QUALIFIABLE and qualified(flat, m):
                continue
            yield text.count("\n", 0, m.start()) + 1, what, m.group(0)


def kinds():
    """The image kinds a definition knows (`ImageKind::as_str`)."""
    src = (ROOT / "crates" / "dagon-topology" / "src" / "defs" / "mod.rs").read_text(encoding="utf-8")
    body = re.search(r"impl ImageKind \{.*?pub fn as_str\(self\) -> &'static str \{(.*?)\n    \}", src, re.S).group(1)
    return re.findall(r'Self::\w+ => "(\w+)"', body)


class SlotClaims(unittest.TestCase):
    maxDiff = None

    def test_no_text_claims_every_image_has_two_slots(self):
        found = [f"{rel}:{n}: {what}: '{words}'" for rel, text in texts().items() for n, what, words in claims(text)]
        self.assertEqual(found, [], "text says what the design does not: an image kind has one slot or two")

    def test_the_limitations_table_has_a_row_for_each_image_kind(self):
        for rel in ("explanation/limitations.md", "en/explanation/limitations.md"):
            text = (DOCS / rel).read_text(encoding="utf-8")
            rows = re.findall(r"^\| `(\w+)` \|", text, re.M)
            with self.subTest(page=rel):
                self.assertEqual(sorted(rows), sorted(kinds()))

    def test_what_the_check_finds_and_leaves(self):
        for text in ("There are no single-slot layouts.", "Every image has two slots.",
                     "The boot loader itself lives in two slots", "entries (two for every MCU\nthat has them)",
                     "stage0 tries the other slots on their own", "スロットが1つの配置はありません。",
                     "ブートローダ自体も2つのスロットに置き", "（持つマイコンではどれも2つです）",
                     "すべてのイメージはスロットを2つ持ちます",
                     "If a write stops part-way, the running version stays.",
                     "書き込みが途中で止まっても、動いているバージョンはそのまま残ります。",
                     "途中で止まっても、ノードは前のバージョンのまま動きます。",
                     # A two-slot clause that does not limit the claim, before or after it.
                     "There are no single-slot layouts, but an image with two slots can roll back.",
                     "An image with two slots can roll back, and there are no single-slot layouts.",
                     "スロットが1つの配置はありませんが、スロットが2つのイメージは戻せます。",
                     "スロットが2つのイメージは戻せ、スロットが1つの配置はありません。",
                     "Every image has two slots, but an image with two slots can roll back.",
                     "An image with two slots can roll back, and every image has two slots.",
                     "すべてのイメージはスロットを2つ持ちますが、スロットが2つのイメージは戻せます。",
                     "スロットが2つのイメージは戻せ、すべてのイメージはスロットを2つ持ちます。",
                     "If a write stops part-way, the running version stays, and an image with two slots can roll back.",
                     "An image with two slots can roll back, and if a write stops part-way, the running version stays.",
                     "途中で止まっても、ノードは前のバージョンのまま動き、スロットが2つのイメージは戻せます。",
                     "スロットが2つのイメージは戻せ、途中で止まっても、ノードは前のバージョンのまま動きます。"):
            self.assertTrue(list(claims(text)), text)
        for text in ("A kind has one slot (`a`) or two (`a` and `b`).", "With two slots, it goes back.",
                     "the other slot was confirmed", "スロットが2つなら前のバージョンに戻ります。",
                     "スロットが1つの種類にできないこと", "An ESP32-S3 image serves both slots",
                     "If a write stops part-way, an image with two slots keeps its running version",
                     "途中で止まっても、ノードはもう一度書き込める状態で動きます。",
                     "If a write stops part-way, the running version stays when the image has two slots.",
                     "With two slots, if a write stops part-way, the running version stays.",
                     "途中で止まっても、スロットが2つなら、ノードは前のバージョンのまま動きます。",
                     "途中で止まっても、ノードは前のバージョンのまま動きます（スロットが2つのとき）。",
                     "スロットが2つなら、途中で止まっても、ノードは前のバージョンのまま動きます。",
                     "With two slots for every kind, there are no single-slot layouts.",
                     "There are no single-slot layouts on an MCU whose definition gives every kind two slots.",
                     "すべての種類にスロットが2つある定義では、スロットが1つの配置はありません。",
                     "On an MCU whose definition gives each kind two slots, every image has two slots.",
                     "Every image has two slots when its MCU definition gives two slots.",
                     "定義がスロットを2つ与えるマイコンでは、すべてのイメージはスロットを2つ持ちます。",
                     "すべてのイメージはスロットを2つ持ちます（定義がスロットを2つ与えるとき）。"):
            self.assertEqual(list(claims(text)), [], text)


if __name__ == "__main__":
    unittest.main()
