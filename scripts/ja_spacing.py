"""Finds half-width spaces between Japanese and Latin letters, digits or code in Markdown.

Japanese takes no space there: `DAGONとは`, `1か所`, `dagon new`と. Left alone: code fences, code
spans, URLs, inline figures, the space after a heading's section number (`### 1.1 名前`), and a space that CommonMark
needs for bold to render (`**名前。** DAGON`).
"""
import re
import unicodedata

JA = re.compile(r"[　-〿぀-ヿ㐀-鿿＀-￯]")
CODE, LINK_END = "", ""
FENCE = re.compile(r"^( *)```[^\n]*\n.*?^\1```[ \t]*$", re.M | re.S)
SPAN = re.compile(r"(`+)(?!`).+?(?<!`)\1(?!`)", re.S)
URL = re.compile(r"(?<=\])\([^)\n]*\)|<https?://[^>\n]*>")
SECTION_NUMBER = re.compile(r"^#+ [\d.]+ +", re.M)
LATIN_BEFORE, LATIN_AFTER = set(')"%+}'), set('("%/~.${')
FIGURE = re.compile(r"<svg.*?</svg>", re.S)
MARKUP = set("*[]") | {LINK_END}


def _mask(text):
    def blank(fill):
        return lambda m: "".join("\n" if c == "\n" else fill for c in m.group())

    text = FIGURE.sub(blank(CODE), FENCE.sub(blank(CODE), text))
    text = SPAN.sub(blank(CODE), text)
    return URL.sub(blank(LINK_END), text)


def _kind(c, latin):
    if c == CODE or c.isascii() and (c.isalnum() or c in latin):
        return "latin"
    return "ja" if JA.match(c) else None


def _is_punctuation(c):
    return c in (CODE, LINK_END) or unicodedata.category(c)[0] in "PS"


def _bold_needs_it(masked, start, end):
    before, after = masked[start - 1], masked[end]
    if before == "*":
        inner = masked[:start].rstrip("*")
        return bool(inner) and _is_punctuation(inner[-1]) and not _is_punctuation(after)
    if after == "*":
        inner = masked[end:].lstrip("*")
        return bool(inner) and _is_punctuation(inner[0]) and not _is_punctuation(before)
    return False


def spaces(text):
    """The (start, end) of every space the rule forbids in a Japanese Markdown text."""
    masked = _mask(text)
    numbered = {m.end() - 1 for m in SECTION_NUMBER.finditer(masked)}
    found = []
    for m in re.finditer(r"(?<=[^\s]) +(?=[^\s])", masked):
        if m.end() - 1 in numbered:
            continue
        i, j = m.start() - 1, m.end()
        while i > 0 and masked[i] in MARKUP:
            i -= 1
        while j < len(masked) - 1 and masked[j] in MARKUP:
            j += 1
        left, right = _kind(masked[i], LATIN_BEFORE), _kind(masked[j], LATIN_AFTER)
        through_markup = i != m.start() - 1 or j != m.end()
        if {left, right} == {"ja", "latin"} or (left == right == "ja" and through_markup):
            if not _bold_needs_it(masked, m.start(), m.end()):
                found.append((m.start(), m.end()))
    return found


def lines_with_spaces(text):
    """The numbers and text of the lines holding such a space, for a failure message."""
    rows = text.splitlines()
    numbers = sorted({text.count("\n", 0, start) + 1 for start, _ in spaces(text)})
    return [f"{n}: {rows[n - 1][:80]}" for n in numbers]


def without_spaces(text):
    """The text with every such space taken out; nothing else changes."""
    for start, end in reversed(spaces(text)):
        text = text[:start] + text[end:]
    return text
