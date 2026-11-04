"""Checks that a component's version is at least what its commits require.

The version is the component's entry in compat.toml; the commits are those
since the component's last release tag (`<component>-v<version>`) that
touch its files (a file belongs to the component with the longest matching
path). Kinds and bumps follow docs/versioning.md. Offline: reads only git
and compat.toml.

    python scripts/bump_check.py <component> [--tag <tag being released>]
    python scripts/bump_check.py --tag <tag being released>

With --tag (the release build passes the pushed tag) the tag must also be
`<component>-v<version>`; without a component, the tag names it. A commit
counts for every component whose files it touches. Exit 0: enough; 1: not
enough, a bad tag or an unknown component.
"""
import argparse
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
KINDS = ["none", "fix", "feature", "breaking"]
HEADER = re.compile(r"^(?P<type>[a-z]+)(?:\([^)]*\))?(?P<bang>!)?:")
NO_RELEASE = {"docs", "test", "ci", "refactor", "chore", "style", "build", "merge"}
SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-rc\.(\d+))?$")


def parse_version(v):
    m = SEMVER.match(v)
    if not m:
        raise ValueError(f"{v}: not x.y.z or x.y.z-rc.N")
    return tuple(int(g) for g in m.groups()[:3]), m.group(4) is not None


def commit_kind(subject, body):
    """The kind a conventional commit asks for; a subject that is not one
    counts as a fix, so an unlabelled change is never released unbumped."""
    m = HEADER.match(subject)
    if m and (m["bang"] or "BREAKING CHANGE" in body):
        return "breaking"
    if not m:
        return "fix"
    t = m["type"]
    if t == "feat":
        return "feature"
    if t in ("fix", "perf"):
        return "fix"
    return "none" if t in NO_RELEASE else "fix"


def owner(path, components):
    """The component owning `path` (longest matching path), or None."""
    best, best_len = None, -1
    for c in components:
        for p in c["paths"]:
            p = p.rstrip("/")
            if (path == p or path.startswith(p + "/")) and len(p) > best_len:
                best, best_len = c["name"], len(p)
    return best


def required(prev, kind):
    """The smallest version after `prev` that `kind` allows."""
    major, minor, patch = prev
    if kind == "none":
        return prev
    if kind == "breaking":
        return (0, minor + 1, 0) if major == 0 else (major + 1, 0, 0)
    if kind == "feature" and major > 0:
        return (major, minor + 1, 0)
    return (major, minor, patch + 1)


def check(name, version, prev, kinds):
    """Problems with releasing `version` after `prev` (None: first release)."""
    base, _ = parse_version(version)
    if prev is None:
        return [] if base == (0, 1, 0) else [f"{name}: the first release is 0.1.0, not {version}"]
    kind = max(kinds, key=KINDS.index, default="none")
    need = required(prev, kind)
    if base < need:
        have = ".".join(map(str, prev))
        want = ".".join(map(str, need))
        return [f"{name}: commits since {have} are {kind}; need at least {want}, have {version}"]
    return []


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True,
                          text=True, encoding="utf-8", cwd=ROOT).stdout


def last_release(name, version, at="HEAD"):
    """The highest final release tag of `name` below `version` among the
    tags merged into `at`, and its version."""
    base, _ = parse_version(version)
    found = []
    for tag in git("tag", "--list", f"{name}-v*", "--merged", at).split():
        try:
            v, rc = parse_version(tag[len(name) + 2:])
        except ValueError:
            continue
        if not rc and v < base:
            found.append((v, tag))
    return max(found, default=(None, None))


def component_of_tag(tag, components):
    """The component a `<component>-v<version>` tag names (longest name), or None."""
    names = [c["name"] for c in components if tag.startswith(c["name"] + "-v")]
    return max(names, key=len, default=None)


def load_components():
    return tomllib.loads((ROOT / "compat.toml").read_text(encoding="utf-8"))["component"]


def commits_since(tag, name, components, paths, to="HEAD"):
    """(kind, subject) of the non-merge commits after `tag` (all when None)
    up to `to` that touch `name`'s files."""
    rng = [f"{tag}..{to}"] if tag else [to]
    out = git("log", "--no-merges", "--name-only", "--format=%x1e%s%x1f%b%x1f", *rng, "--", *paths)
    kinds = []
    for rec in out.split("\x1e")[1:]:
        subject, body, files = rec.split("\x1f", 2)
        if any(owner(f, components) == name for f in files.split()):
            kinds.append((commit_kind(subject, body), subject))
    return kinds


def main():
    # Commit subjects are UTF-8 (Japanese words); a Windows console defaults to another code page.
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("component", nargs="?")
    ap.add_argument("--tag")
    a = ap.parse_args()
    comps = load_components()
    if a.component is None:
        if not a.tag:
            ap.error("give a component, a --tag, or both")
        a.component = component_of_tag(a.tag, comps)
        if a.component is None:
            sys.exit(f"tag {a.tag} names no component of compat.toml")
    c = next((c for c in comps if c["name"] == a.component), None)
    if c is None:
        sys.exit(f"no component {a.component} in compat.toml")
    version = c["version"]
    problems = []
    if a.tag and a.tag != f"{a.component}-v{version}":
        problems.append(f"tag {a.tag} is not {a.component}-v{version} (compat.toml)")
    prev, tag = last_release(a.component, version)
    kinds = commits_since(tag, a.component, comps, c["paths"]) if prev else []
    for k, s in kinds:
        print(f"  {k:8} {s}")
    problems += check(a.component, version, prev, [k for k, _ in kinds])
    for p in problems:
        print(p, file=sys.stderr)
    if problems:
        sys.exit(1)
    print(f"{a.component} {version}: ok" + (f" (since {tag})" if tag else " (first release)"))


if __name__ == "__main__":
    main()
