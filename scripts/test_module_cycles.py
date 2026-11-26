"""No crate's top-level modules depend on each other in a cycle.

A module `a` depends on `b` when code in `a` (its file and its
submodules, test code left out) names `crate::b`, or names an item the
crate root re-exports from `b` (`pub use b::Item` in lib.rs, then
`crate::Item`). The crate root itself is no module here: it only
declares and re-exports."""
import re
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rust_source import ROOT, code_only  # noqa: E402

MOD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;", re.M)
CRATE_PATH = re.compile(r"\bcrate::(\w+)")
CRATE_GROUP = re.compile(r"\bcrate::\{([^}]*)\}")
ROOT_USE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+(?:self::)?(\w+)::([^;]+);", re.M | re.S)
# Cycles not yet untangled (GitHub issue #61): each must stay as it is or
# shrink; a new cycle anywhere, or a module joining one of these, fails.
KNOWN = {
    "crates/dagon-cli": [
        ["agents", "consent", "keystore"],
        ["conform", "demo", "deploy", "device", "doctor", "flash", "kept", "serve"],
        ["extgen", "scaffold"],
    ],
}

TEST_MOD = re.compile(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")


def crates():
    """Each crate's src directory with a lib.rs or main.rs."""
    for toml in sorted(ROOT.glob("**/Cargo.toml")):
        rel = toml.relative_to(ROOT).parts
        if "target" in rel or "node_modules" in rel:
            continue
        src = toml.parent / "src"
        for root in ("lib.rs", "main.rs"):
            if (src / root).is_file():
                yield src, src / root
                break


def strip_tests(code):
    """`code` without its inline `#[cfg(test)] mod x { ... }` blocks."""
    out, i = [], 0
    for m in TEST_MOD.finditer(code):
        if m.start() < i:
            continue
        out.append(code[i:m.start()])
        depth, j = 1, m.end()
        while j < len(code) and depth:
            depth += {"{": 1, "}": -1}.get(code[j], 0)
            j += 1
        i = j
    out.append(code[i:])
    return "".join(out)


def names(group):
    """The first segment of each path in a `{a::b, c, d as e}` group."""
    return [p.strip().split("::")[0].split(" ")[0] for p in group.split(",") if p.strip()]


def module_files(src, name):
    """The files of top-level module `name`, test files left out."""
    files = []
    one = src / f"{name}.rs"
    if one.is_file():
        files.append(one)
    folder = src / name
    if folder.is_dir():
        files += sorted(folder.rglob("*.rs"))
    return [f for f in files if not f.stem.startswith("tests") and f.stem != "test"]


def graph(src, root_file):
    root = code_only(root_file.read_text(encoding="utf-8"))
    mods = set(MOD.findall(root))
    # crate::Item -> the module the root re-exports it from.
    owner = {}
    for m, rest in ROOT_USE.findall(root):
        if m not in mods:
            continue
        rest = rest.strip()
        items = names(rest[1:-1]) if rest.startswith("{") else [rest.split(" as ")[-1].split("::")[-1]]
        for alias in re.findall(r"\bas\s+(\w+)", rest):
            items.append(alias)
        for it in items:
            owner.setdefault(it, m)
    edges = {m: set() for m in mods}
    for m in mods:
        for f in module_files(src, m):
            code = strip_tests(code_only(f.read_text(encoding="utf-8")))
            used = CRATE_PATH.findall(code)
            for g in CRATE_GROUP.findall(code):
                used += names(g)
            for u in used:
                to = u if u in mods else owner.get(u)
                if to and to != m:
                    edges[m].add(to)
    return edges


def cycles(edges):
    """Each strongly connected group of two or more modules, sorted."""
    index, low, stack, on, out = {}, {}, [], set(), []
    counter = [0]

    def visit(v):
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        on.add(v)
        for w in sorted(edges[v]):
            if w not in index:
                visit(w)
                low[v] = min(low[v], low[w])
            elif w in on:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            group = []
            while True:
                w = stack.pop()
                on.discard(w)
                group.append(w)
                if w == v:
                    break
            if len(group) > 1:
                out.append(sorted(group))

    for v in sorted(edges):
        if v not in index:
            visit(v)
    return sorted(out)


def all_cycles():
    found = {}
    for src, root in crates():
        c = cycles(graph(src, root))
        if c:
            found[src.parent.relative_to(ROOT).as_posix()] = c
    return found


class ModuleCycles(unittest.TestCase):
    def test_no_crate_has_a_module_cycle(self):
        for crate, groups in all_cycles().items():
            known = KNOWN.get(crate, [])
            for g in groups:
                self.assertTrue(
                    any(set(g) <= set(k) for k in known),
                    f"{crate}: modules {g} depend on each other in a cycle",
                )

    def test_a_cycle_is_found(self):
        self.assertEqual(cycles({"a": {"b"}, "b": {"c"}, "c": {"a"}, "d": {"a"}}), [["a", "b", "c"]])
        self.assertEqual(cycles({"a": {"b"}, "b": set()}), [])

    def test_test_blocks_are_left_out(self):
        code = "fn f() { crate::a::x(); }\n#[cfg(test)]\nmod tests { fn g() { crate::b::y(); } }\n"
        self.assertNotIn("crate::b", strip_tests(code))
        self.assertIn("crate::a", strip_tests(code))


if __name__ == "__main__":
    for crate, groups in all_cycles().items():
        print(crate, groups)
