"""Writes one component's changelog section from its commits.

The commits and their kinds are read exactly as scripts/bump_check.py
reads them: non-merge commits in the range that touch the component's
files (longest path in compat.toml wins), kinds from conventional commits.
Offline: reads only git and compat.toml. Prints Markdown; add it to the
release notes or a CHANGELOG by hand.

    python scripts/changelog.py <component> [--from <tag>] [--to <rev>] [--version <x.y.z>]

--from defaults to the component's last final tag below --version among
the tags merged into --to (all history when there is none), --to to HEAD,
--version to compat.toml's.
"""
import argparse
import importlib.util
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("bump_check", HERE / "bump_check.py")
bc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bc)

SECTIONS = [("breaking", "Breaking"), ("feature", "Features"), ("fix", "Fixes")]


def section(name, version, since, kinds):
    """The Markdown section for `kinds` [(kind, subject)]; commits that need
    no release are left out."""
    lines = [f"## {name} {version}", ""] + ([f"Since {since}.", ""] if since else [])
    bare = len(lines)
    for kind, title in SECTIONS:
        subjects = [s for k, s in kinds if k == kind]
        if subjects:
            lines += [f"### {title}", ""] + [f"- {s}" for s in subjects] + [""]
    if len(lines) == bare:
        lines += ["No changes that need a release.", ""]
    return "\n".join(lines)


def main():
    # Commit subjects are UTF-8 (Japanese words); a Windows console defaults to another code page.
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("component")
    ap.add_argument("--from", dest="since")
    ap.add_argument("--to", default="HEAD")
    ap.add_argument("--version")
    a = ap.parse_args()
    comps = bc.load_components()
    c = next((c for c in comps if c["name"] == a.component), None)
    if c is None:
        sys.exit(f"no component {a.component} in compat.toml")
    version = a.version or c["version"]
    since = a.since or bc.last_release(a.component, version, at=a.to)[1]
    kinds = bc.commits_since(since, a.component, comps, c["paths"], to=a.to)
    print(section(a.component, version, since, kinds))


if __name__ == "__main__":
    main()
