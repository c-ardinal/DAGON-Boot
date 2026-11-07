"""Writes llms.txt (https://llmstxt.org): the site's pages with their addresses and summaries.

    python site/scripts/llms.py <site URL> <output file>

The site URL is where the site lives (the Pages address); Japanese pages are
under <URL>/doc/ and English pages under <URL>/doc/en/.
"""
import pathlib
import re
import sys

DOCS = pathlib.Path(__file__).resolve().parents[1] / "src" / "content" / "docs"
ORDER = ["", "getting-started", "how-to", "explanation", "reference"]


def frontmatter(path):
    m = re.match(r"\A---\n(.*?)\n---\n", path.read_text(encoding="utf-8"), re.S)
    fields = dict(re.findall(r"^(title|description): (.*)$", m.group(1), re.M)) if m else {}
    clean = lambda k, d: fields.get(k, d).strip().strip('"')
    return clean("title", path.stem), clean("description", "")


def pages(root=DOCS, skip=("en",)):
    """(section, slug, title, description) of every page under `root`, in section order."""
    out = []
    for p in sorted(root.rglob("*.md*")):
        rel = p.relative_to(root)
        if rel.parts[0] in skip:
            continue
        slug = "/".join(rel.with_suffix("").parts)
        slug = "" if slug == "index" else re.sub(r"/index$", "", slug) + "/"
        title, description = frontmatter(p)
        out.append((rel.parts[0] if len(rel.parts) > 1 else "", slug, title, description))
    return sorted(out, key=lambda x: ORDER.index(x[0]) if x[0] in ORDER else len(ORDER))


def block(heading, base, items):
    lines = ["", f"## {heading}", ""]
    for _, slug, title, description in items:
        lines.append(f"- [{title}]({base}{slug}): {description}".rstrip(": "))
    return lines


def render(url):
    site = url.rstrip("/")
    lines = [
        "# DAGON",
        "",
        "> DAGON is the boot stage and the update path for groups of microcontrollers: signed images,",
        "> trial boots that go back by themselves, and one 親機 (hub) that writes every node.",
        "",
        f"- [DAGON Console demo]({site}/demo/): the Console on simulated nodes in the browser",
    ]
    lines += block("日本語", f"{site}/doc/", pages())
    lines += block("English", f"{site}/doc/en/", pages(DOCS / "en", skip=()))
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    pathlib.Path(sys.argv[2]).write_text(render(sys.argv[1]), encoding="utf-8", newline="\n")
