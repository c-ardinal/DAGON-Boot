"""The repository's Rust sources, and their code without comments or
literals, for the guards that read Rust (test_*.py)."""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST_DIRS = ("crates", "targets", "samples")

RAW_STRING = re.compile(r'b?r(#*)"')
RAW_IDENT = re.compile(r"r#[A-Za-z_]")


def rust_texts():
    """Path (relative to the repository, `/`-separated) -> text of every
    `.rs` file under RUST_DIRS, build output left out."""
    texts = {}
    for d in RUST_DIRS:
        for p in (ROOT / d).rglob("*.rs"):
            if "target" not in p.relative_to(ROOT).parts:
                texts[p.relative_to(ROOT).as_posix()] = p.read_text(encoding="utf-8")
    return texts


def code_only(text):
    """`text` with each comment (line, nested block) and each string, raw
    string, char and byte literal replaced by one space; lifetimes stay, and
    a raw identifier `r#name` reads as `name`."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            i = n if i < 0 else i
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif (m := RAW_STRING.match(text, i)) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            close = '"' + m.group(1)
            j = text.find(close, m.end())
            i = n if j < 0 else j + len(close)
        elif RAW_IDENT.match(text, i) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            i += 2
            continue
        elif c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            i += 1
        elif c == "'" and i + 1 < n and text[i + 1] == "\\":
            j = text.find("'", i + 3)
            i = n if j < 0 else j + 1
        elif c == "'" and i + 2 < n and text[i + 2] == "'":
            i += 3
        else:
            out.append(c)
            i += 1
            continue
        out.append(" ")
    return "".join(out)
