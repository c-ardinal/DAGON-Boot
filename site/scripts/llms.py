"""Writes llms.txt (https://llmstxt.org): the site's pages with their addresses and summaries.

    python site/scripts/llms.py <site URL> <output file>

The site URL is where the site lives (the Pages address); pages are under <URL>/doc/.
Only the default locale is listed: every Japanese page has an English one.
"""
import pathlib
import re
import sys

DOCS = pathlib.Path(__file__).resolve().parents[1] / "src" / "content" / "docs"
LOCALES = ("ja",)


def frontmatter(path):
    m = re.match(r"\A---\n(.*?)\n---\n", path.read_text(encoding="utf-8"), re.S)
    fields = dict(re.findall(r"^(title|description): (.*)$", m.group(1), re.M)) if m else {}
    return fields.get("title", path.stem), fields.get("description", "")


def pages():
    """(section, slug, title, description) of every default-locale page, in section order."""
    out = []
    for p in sorted(DOCS.rglob("*.md*")):
        rel = p.relative_to(DOCS)
        if rel.parts[0] in LOCALES:
            continue
        slug = "/".join(rel.with_suffix("").parts)
        slug = "" if slug == "index" else re.sub(r"/index$", "", slug) + "/"
        title, description = frontmatter(p)
        out.append((rel.parts[0] if len(rel.parts) > 1 else "", slug, title, description))
    return out


def render(url):
    base = url.rstrip("/") + "/doc/"
    sections = {}
    for section, slug, title, description in pages():
        sections.setdefault(section, []).append(f"- [{title}]({base}{slug}): {description}".rstrip(": "))
    lines = [
        "# DAGON",
        "",
        "> DAGON is the boot stage and the update path for groups of microcontrollers: signed images,",
        "> trial boots that go back by themselves, and one 親機 (hub) that writes every node.",
        "",
        f"- [DAGON Console demo]({url.rstrip('/')}/demo/): the Console on simulated nodes in the browser",
    ]
    order = ["", "getting-started", "how-to", "explanation", "reference"]
    for section in sorted(sections, key=lambda s: order.index(s) if s in order else len(order)):
        lines += ["", f"## {section.replace('-', ' ').capitalize() or 'Docs'}", ""] + sections[section]
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    pathlib.Path(sys.argv[2]).write_text(render(sys.argv[1]), encoding="utf-8", newline="\n")
