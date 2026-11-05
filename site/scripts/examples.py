"""Checks the code examples in the site's pages: every TOML and JSON block parses.

    python site/scripts/examples.py   # exit 1 naming each block that does not

A block that shows a fragment on purpose carries `nocheck` in its info string
(```toml nocheck).
"""
import json
import pathlib
import re
import sys
import tomllib

DOCS = pathlib.Path(__file__).resolve().parents[1] / "src" / "content" / "docs"
FENCE = re.compile(r"^```(\w+)([^\n]*)\n(.*?)^```", re.M | re.S)
PARSERS = {"toml": tomllib.loads, "json": json.loads}


def problems(root=DOCS):
    out = []
    for p in sorted(root.rglob("*.md*")):
        text = p.read_text(encoding="utf-8")
        for m in FENCE.finditer(text):
            lang, info, body = m.groups()
            parse = PARSERS.get(lang)
            if not parse or "nocheck" in info:
                continue
            try:
                parse(body)
            except ValueError as e:
                line = text.count("\n", 0, m.start()) + 1
                out.append(f"{p.relative_to(root).as_posix()}:{line}: {lang} block does not parse: {e}")
    return out


if __name__ == "__main__":
    found = problems()
    print("\n".join(found) or "every example parses")
    sys.exit(1 if found else 0)
