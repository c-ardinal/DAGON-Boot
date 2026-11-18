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
# A noun phrase between two の: no hiragana (so no other particle), no punctuation.
_PHRASE = "[^\\sぁ-ゖ、。，．,.:：;；「」『』（）()\\[\\]【】<>\"'|/]{1,15}"
NO_CHAIN = re.compile(f"(?:{_PHRASE}の){{3,}}{_PHRASE}")
POLITE_END = re.compile("(?:です|ます|ません|でした|ました|ましょう|でしょう|ください)$")
CLOSE_OPEN = {"）": "（", ")": "("}
NO_AREA = "(none)"


class Rule(NamedTuple):
    kind: str
    regex: re.Pattern
    suggest: str


class Hit(NamedTuple):
    line: int
    kind: str
    word: str
    suggest: str
    source: str


def load_lexicon(path: Path) -> list[Rule]:
    """The retired terms, the native words, then the conjunctions, in file order."""
    data = tomllib.loads(Path(path).read_text(encoding="utf-8"))
    rules = []
    for kind in ("retired", "native", "joint"):
        for e in data[kind]:
            pattern = e["pattern"] if e.get("raw") else NOT_AFTER_KANJI + "(?:" + e["pattern"] + ")"
            rules.append(Rule(kind, re.compile(pattern), f"{e['word']} -> {e['suggest']}"))
    return rules


def load_allow(path: Path) -> list[dict]:
    """The allow-list; ValueError when an entry lacks its path, text or reason."""
    entries = tomllib.loads(Path(path).read_text(encoding="utf-8")).get("allow", [])
    for n, e in enumerate(entries, 1):
        for key in ("path", "text", "reason"):
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


def _lexicon_hits(line: str, rules: list[Rule]) -> list[tuple[str, int, int, str]]:
    used = [False] * len(line)
    found = []
    for rule in rules:
        for m in rule.regex.finditer(line):
            a, b = m.span()
            if a == b or any(used[a:b]):
                continue
            used[a:b] = [True] * (b - a)
            found.append((rule.kind, a, b, rule.suggest))
    return found


def _without_trailing_note(sentence: str) -> str | None:
    """The sentence without a closing parenthetical (実行します（詳細） -> 実行します);
    None when the parenthetical opened on an earlier line, so the ending is not on this one."""
    sentence = sentence.rstrip("*_ ")
    while sentence and sentence[-1] in CLOSE_OPEN:
        start = sentence.rfind(CLOSE_OPEN[sentence[-1]])
        if start < 0:
            return None
        sentence = sentence[:start].rstrip("*_ ")
    return sentence


def _plain_endings(line: str) -> list[tuple[int, int]]:
    found = []
    for m in re.finditer("。", line):
        if line.startswith("**", m.end()):
            continue  # a bold lead-in (**概要。**) is a heading, not a sentence
        sentence = _without_trailing_note(line[: m.start()])
        if sentence and JA.search(sentence) and not POLITE_END.search(sentence):
            found.append((max(0, len(sentence) - 4), m.end()))
    return found


def _line_hits(line: str, rules: list[Rule], polite: bool) -> list[tuple[str, int, int, str]]:
    found = _lexicon_hits(line, rules)
    found += [("no-chain", *m.span(), "split the chain of three or more の") for m in NO_CHAIN.finditer(line)]
    if polite:
        found += [("plain", a, b, "polite style: 〜です/〜します") for a, b in _plain_endings(line)]
    return found


def hits_in(path: str, text: str, rules: list[Rule], polite: bool) -> list[Hit]:
    """Every finding in one file's text; code in Markdown is left out (masking keeps
    every character's position, so the reported word is cut from the source line)."""
    masked = mask_code(text) if path.endswith(MARKDOWN) else text
    hits = []
    for n, (line, source) in enumerate(zip(masked.split("\n"), text.split("\n")), 1):
        if JA.search(line):
            hits += [
                Hit(n, kind, source[a:b], suggest, source) for kind, a, b, suggest in _line_hits(line, rules, polite)
            ]
    return hits


def unallowed(path: str, hits: list[Hit], allow: list[dict]) -> list[Hit]:
    return [h for h in hits if not any(fnmatch.fnmatchcase(path, e["path"]) and e["text"] in h.source for e in allow)]


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
            print(f"{path}:{h.line}: {h.kind}: {h.word} ({h.suggest})")
        counts[area] += len(hits)
    if args.all:
        for area, n in sorted(counts.items()):
            print(f"{area}: {n}")
    return 1 if sum(counts.values()) else 0


if __name__ == "__main__":
    sys.exit(main())
