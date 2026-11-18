"""Finds Japanese wording the repository's wording rule replaces.

The rule: explain with Sino-Japanese compounds and katakana loanwords instead of
native Japanese words, use the defined terms (書込, 作成, ノード), split three or
more chained の, replace the conjunctions ja_wording.toml lists, and write documents in the polite style.

The words come from ja_wording.toml, the exceptions from ja_wording_allow.toml
and the areas to check from ja_wording_scopes.toml. Prints path:line, the kind,
the text found and the suggested replacement; exits 1 on any finding that is not
allowed, or on a broken data file.

    python scripts/ja_wording.py            the enabled areas
    python scripts/ja_wording.py --all      every area, with the count per area
    python scripts/ja_wording.py FILE...    the given files
"""

import argparse
import bisect
import fnmatch
import re
import subprocess
import sys
import tomllib
from collections import Counter
from pathlib import Path
from typing import NamedTuple

from ja_spacing import _mask as mask_code

HERE = Path(__file__).resolve().parent
LEXICON = HERE / "ja_wording.toml"
ALLOW = HERE / "ja_wording_allow.toml"
SCOPES = HERE / "ja_wording_scopes.toml"

JA = re.compile("[぀-ヿ㐀-鿿]")
NOT_AFTER_KANJI = "(?<![㐀-鿿々豈-﫿])"
MARKDOWN = (".md", ".mdx")
# A noun phrase between two の: no hiragana (so no other particle), no punctuation;
# a line break may follow an の (a Markdown paragraph wrapped there).
_PHRASE = "[^\\sぁ-ゖ、。，．,.:：;；「」『』（）()\\[\\]【】<>\"'|/]{1,15}"
NO_CHAIN = re.compile(f"(?:{_PHRASE}の(?:\\n[ \\t]*)?){{3,}}{_PHRASE}")
HEADING = re.compile(r"^\s*#")
POLITE_END = re.compile("(?:です|ます|ません|でした|ました|ましょう|でしょう|ください)$")
OPENING, CLOSING = "（(", "）)"
NO_AREA = "(none)"
TABLE_ROW = re.compile(r"^\s*\|")
# A list item, or a label line (**名称**: …, 名称: …): a noun ending there closes a fragment, not a sentence.
FRAGMENT_LINE = re.compile(r"^\s*(?:[-*+]|\d+\.)\s|^\s*(?:\*\*[^*]+\*\*|[^\s。:：]{1,30})[:：]\s")
HIRAGANA = re.compile("[ぁ-ゖ]$")
LINE_BREAK = re.compile(r"\n[ \t]*")
ALLOW_KEYS = ("path", "text", "kind", "word", "reason")


class Rule(NamedTuple):
    kind: str
    regex: re.Pattern
    lemma: str
    suggest: str


class Hit(NamedTuple):
    line: int
    start: int
    end: int
    kind: str
    word: str
    lemma: str
    suggest: str
    text: str
    at: int


def _pattern(entry: dict, conjugation: dict) -> str:
    """An entry's regular expression: its pattern, or its stem and its class's endings."""
    if "stem" in entry:
        if entry.get("conj") not in conjugation:
            raise ValueError(f"lexicon entry {entry['word']}: unknown conj {entry.get('conj')!r}")
        pattern = "(?:" + entry["stem"] + ")" + conjugation[entry["conj"]]
    else:
        pattern = entry["pattern"]
    return pattern if entry.get("raw") else NOT_AFTER_KANJI + "(?:" + pattern + ")"


def load_lexicon(path: Path) -> list[Rule]:
    """The retired terms, the native words, then the conjunctions, in file order."""
    data = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    return [
        Rule(kind, re.compile(_pattern(e, data["conjugation"])), e["word"], e["suggest"])
        for kind in ("retired", "native", "joint")
        for e in data[kind]
    ]


def load_allow(path: Path) -> list[dict]:
    """The allow-list; ValueError when an entry lacks one of ALLOW_KEYS."""
    entries = tomllib.loads(Path(path).read_text(encoding="utf-8")).get("allow", [])
    for n, e in enumerate(entries, 1):
        for key in ALLOW_KEYS:
            if not str(e.get(key, "")).strip():
                raise ValueError(f"{path}: allow entry {n} has no {key}")
    return entries


def load_scopes(path: Path) -> dict:
    return tomllib.loads(Path(path).read_text(encoding="utf-8"))


def matches(path: str, globs: list[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, g) for g in globs)


def is_polite(path: str, globs: list[str]) -> bool:
    return matches(path, globs)


def area_of(path: str, scopes: dict) -> str:
    return next((a["name"] for a in scopes["area"] if matches(path, a["paths"])), NO_AREA)


def _lexicon_hits(line: str, rules: list[Rule]) -> list[tuple[str, int, int, str, str]]:
    """The longest match wins a stretch of text (a compound verb over its last part); on equal
    length, the earlier rule."""
    candidates = [
        (-(m.end() - m.start()), order, m.start(), m.end(), rule)
        for order, rule in enumerate(rules)
        for m in rule.regex.finditer(line)
        if m.end() > m.start()
    ]
    used = [False] * len(line)
    found = []
    for _, _, a, b, rule in sorted(candidates):
        if not any(used[a:b]):
            used[a:b] = [True] * (b - a)
            found.append((rule.kind, a, b, rule.lemma, rule.suggest))
    return sorted(found, key=lambda f: f[1])


def _outside_parentheses(text: str) -> list[bool]:
    """Per character, whether it stands outside every parenthetical. A parenthetical may
    span lines but not paragraphs: a blank line closes whatever is still open."""
    outside, depth = [], 0
    for line in text.split("\n"):
        if not line.strip():
            depth = 0
        for c in line:
            if c in OPENING:
                depth += 1
            elif c in CLOSING:
                depth = max(0, depth - 1)
            outside.append(depth == 0 and c not in OPENING + CLOSING)
        outside.append(False)
    return outside


def _plain_endings(text: str) -> list[tuple[int, int]]:
    """Sentences that end in the plain style. A sentence may span lines; it starts after
    the previous 。 or at a blank line, heading, table row or list item. Table rows,
    parentheticals, bold lead-ins (**概要。**) and noun-ending list items (with their
    indented continuation lines) or labels are not sentences."""
    outside = _outside_parentheses(text)
    found, start, pos, in_item = [], 0, 0, False
    for line in text.split("\n"):
        item = bool(FRAGMENT_LINE.match(line))
        in_item = item or (in_item and line[:1] in (" ", "\t") and bool(line.strip()))
        table = bool(TABLE_ROW.match(line))
        if item or table or not line.strip() or HEADING.match(line):
            start = pos
        for i in range(pos, pos + len(line)):
            if table or text[i] != "。" or not outside[i]:
                continue
            kept = [k for k in range(start, i) if outside[k]]
            while kept and text[kept[-1]] in "*_ \t":
                kept.pop()
            sentence = "".join(text[k] for k in kept)
            start = i + 1
            if text.startswith("**", i + 1) or not JA.search(sentence) or POLITE_END.search(sentence):
                continue
            if in_item and not HIRAGANA.search(sentence):
                continue
            tail = [k for k in kept[-4:] if k >= text.rfind("\n", 0, kept[-1]) + 1]
            found.append((tail[0], kept[-1] + 1))  # the sentence's last characters on its last line
        pos += len(line) + 1
    return found


def _findings(text: str, rules: list[Rule], polite: bool) -> list[tuple[str, int, int, str, str]]:
    found = _lexicon_hits(text, rules)
    found += [("no-chain", *m.span(), "", "split the chain of three or more の") for m in NO_CHAIN.finditer(text)]
    if polite:
        found += [("plain", a, b, "", "polite style: 〜です/〜します") for a, b in _plain_endings(text)]
    return found


def hits_in(path: str, text: str, rules: list[Rule], polite: bool) -> list[Hit]:
    """Every finding in one file's text, on the line where it starts, its word joined onto
    one line; code in Markdown is left out (masking keeps every character's position, so
    the reported word is cut from the source)."""
    masked = mask_code(text) if path.endswith(MARKDOWN) else text
    starts = [0] + [i + 1 for i, c in enumerate(text) if c == "\n"]
    hits = []
    for kind, a, b, lemma, sug in sorted(_findings(masked, rules, polite), key=lambda f: f[1]):
        n = bisect.bisect_right(starts, a) - 1
        word = LINE_BREAK.sub("", text[a:b])
        hits.append(Hit(n + 1, a - starts[n], b - starts[n], kind, word, lemma, sug, text, a))
    return hits


def _covers(entry: dict, path: str, hit: Hit) -> bool:
    """Whether the entry names this finding: its path, kind and word, inside the entry's text
    (searched in the whole file, so a finding across lines is named like any other)."""
    if not fnmatch.fnmatchcase(path, entry["path"]) or (entry["kind"], entry["word"]) != (hit.kind, hit.word):
        return False
    end = hit.at + hit.end - hit.start
    at = hit.text.find(entry["text"])
    while at >= 0:
        if at <= hit.at and end <= at + len(entry["text"]):
            return True
        at = hit.text.find(entry["text"], at + 1)
    return False


def unallowed(path: str, hits: list[Hit], allow: list[dict]) -> list[Hit]:
    return [h for h in hits if not any(_covers(e, path, h) for e in allow)]


def tracked(root: Path) -> list[str]:
    out = subprocess.run(["git", "-C", str(root), "ls-files", "-z"], capture_output=True, check=True)
    return [p for p in out.stdout.decode("utf-8").split("\0") if p]


def read_text(path: Path) -> str | None:
    try:
        return path.read_bytes().decode("utf-8")
    except (UnicodeDecodeError, OSError):
        return None


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("files", nargs="*", help="files to check, relative to --root (default: the enabled areas)")
    ap.add_argument("--all", action="store_true", help="check every area and print the count per area")
    ap.add_argument("--root", type=Path, default=HERE.parent)
    ap.add_argument("--lexicon", type=Path, default=LEXICON)
    ap.add_argument("--allow", type=Path, default=ALLOW)
    ap.add_argument("--scopes", type=Path, default=SCOPES)
    args = ap.parse_args(argv)
    sys.stdout.reconfigure(encoding="utf-8")
    try:
        rules = load_lexicon(args.lexicon)
        allow = load_allow(args.allow)
        scopes = load_scopes(args.scopes)
    except (OSError, ValueError, KeyError, re.error) as e:
        print(f"ja_wording: {e}", file=sys.stderr)
        return 1
    paths = args.files or [p for p in tracked(args.root) if not matches(p, scopes["exclude"])]
    enabled = {a["name"] for a in scopes["area"] if a["enabled"]}
    counts = Counter()
    for path in paths:
        area = area_of(path, scopes)
        if not (args.files or args.all or area in enabled):
            continue
        text = read_text(args.root / path)
        if text is None or not JA.search(text):
            continue
        hits = unallowed(path, hits_in(path, text, rules, is_polite(path, scopes["polite"])), allow)
        for h in hits:
            print(f"{path}:{h.line}: {h.kind}: {h.word} ({h.lemma + ' -> ' if h.lemma else ''}{h.suggest})")
        counts[area] += len(hits)
    if args.all:
        for area, n in sorted(counts.items()):
            print(f"{area}: {n}")
    return 1 if sum(counts.values()) else 0


if __name__ == "__main__":
    sys.exit(main())
